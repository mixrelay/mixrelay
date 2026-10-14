use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{
        StatusCode,
        header::{HOST, ORIGIN},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use crate::{
    DriverCommand, DriverError, DriverManager, DriverSnapshot, SNAPSHOT_ROUTE, contract::JSON_LIMIT,
};

#[derive(Clone)]
struct Control {
    manager: Arc<Mutex<DriverManager>>,
    paid_access: Arc<dyn Fn() -> bool + Send + Sync>,
    ingress: Arc<Semaphore>,
    operations: Arc<Semaphore>,
}

/// Builds the independent Driver GET/PUT API with bounded ingress and no browser origins.
/// The binary must use a peer-authorized Unix listener and own its lifetime. No license or
/// graph state is consulted. Native/file work is serialized outside the async reactor.
/// Call from a Tokio runtime; startup inventory is initialized without client requests.
pub fn router(manager: DriverManager) -> Router {
    router_with_access(manager, Arc::new(|| false))
}

/// Applies publisher-provided paid access at the actual mutation boundary.
/// Free geometry, activation and basic loopback never require a paid credential.
/// The provider runs on a blocking control worker, never an audio callback.
pub fn router_with_access(
    manager: DriverManager,
    paid_access: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Router {
    let state = Control {
        manager: Arc::new(Mutex::new(manager)),
        paid_access,
        ingress: Arc::new(Semaphore::new(8)),
        operations: Arc::new(Semaphore::new(8)),
    };
    tokio::spawn(maintain_inventory(Arc::downgrade(&state.manager)));
    Router::new()
        .route(SNAPSHOT_ROUTE, get(snapshot).put(mutate))
        .layer(middleware::from_fn_with_state(state.clone(), admit))
        .with_state(state)
}

async fn maintain_inventory(manager: Weak<Mutex<DriverManager>>) {
    let mut previous_error = None;
    loop {
        let owner = manager.clone();
        // Only an in-flight native transaction retains the manager. Waiting between
        // recovery attempts cannot extend router ownership after service shutdown.
        let result = tokio::task::spawn_blocking(move || {
            let owner = owner.upgrade()?;
            let result = owner.lock().maintain();
            Some(result)
        })
        .await;
        let delay = match result {
            Ok(Some(Ok(true))) => {
                tracing::info!("Driver native inventory restored");
                previous_error = None;
                Duration::from_secs(1)
            }
            Ok(None) => return,
            Err(error) => {
                tracing::error!(%error, "Driver inventory maintenance worker failed");
                return;
            }
            Ok(Some(Ok(false))) => {
                previous_error = None;
                Duration::from_secs(1)
            }
            // Native transactions already have bounded ACK/readback waits. Keep
            // their failure observable once and avoid rapid reapplication.
            Ok(Some(Err(error))) => {
                let detail = error.to_string();
                if previous_error.as_ref() != Some(&detail) {
                    tracing::error!(%error, "Driver inventory maintenance failed");
                    previous_error = Some(detail);
                }
                Duration::from_secs(5)
            }
        };
        tokio::time::sleep(delay).await;
    }
}

async fn snapshot(State(control): State<Control>) -> Result<Json<DriverSnapshot>, ApiError> {
    operate(control, None).await.map(Json)
}

async fn mutate(
    State(control): State<Control>,
    Json(command): Json<DriverCommand>,
) -> Result<Json<DriverSnapshot>, ApiError> {
    operate(control, Some(command)).await.map(Json)
}

async fn operate(
    control: Control,
    command: Option<DriverCommand>,
) -> Result<DriverSnapshot, ApiError> {
    let permit = control
        .operations
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError(DriverError::Unavailable(
                "Driver control queue is full".into(),
            ))
        })?;
    tokio::task::spawn_blocking(move || {
        let mut manager = control.manager.lock();
        let paid = (control.paid_access)();
        let result = match command {
            Some(command) => {
                let current = manager.snapshot()?;
                authorize(&command, &current, paid)?;
                manager.mutate(command)
            }
            None => manager.snapshot(),
        }
        .map(|mut snapshot| {
            snapshot.settings.can_create &= paid;
            snapshot.settings.can_rename &= paid;
            snapshot
        });
        // Admission follows the blocking operation even if its HTTP caller disconnects.
        drop(permit);
        result
    })
    .await
    .map_err(|error| {
        ApiError(DriverError::Backend(format!(
            "Driver control worker: {error}"
        )))
    })?
    .map_err(ApiError)
}

fn authorize(
    command: &DriverCommand,
    snapshot: &DriverSnapshot,
    paid: bool,
) -> Result<(), DriverError> {
    let restricted = match command {
        DriverCommand::Create { .. } | DriverCommand::Remove { .. } => true,
        DriverCommand::Configure { spec, .. } => snapshot
            .settings
            .devices
            .iter()
            .find(|device| device.spec.id == spec.id)
            .is_some_and(|device| device.spec.display_name != spec.display_name),
        _ => false,
    };
    if restricted && !paid {
        return Err(DriverError::AccessDenied);
    }
    Ok(())
}

async fn admit(State(control): State<Control>, request: Request, next: Next) -> Response {
    if request.headers().contains_key(ORIGIN) {
        return (StatusCode::FORBIDDEN, "untrusted Driver origin").into_response();
    }
    if request
        .headers()
        .get(HOST)
        .is_some_and(|host| host != "mixrelay-driver")
    {
        return (StatusCode::FORBIDDEN, "untrusted Driver host").into_response();
    }
    let Ok(permit) = control.ingress.try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Driver control ingress is full",
        )
            .into_response();
    };
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(Duration::from_secs(2), to_bytes(body, JSON_LIMIT)).await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Driver body exceeds 1 MiB").into_response();
        }
        Err(_) => {
            return (StatusCode::REQUEST_TIMEOUT, "Driver body read timed out").into_response();
        }
    };
    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    drop(permit);
    response
}

struct ApiError(DriverError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            DriverError::Invalid(_) | DriverError::Json(_) => StatusCode::BAD_REQUEST,
            DriverError::RevisionMismatch { .. } => StatusCode::CONFLICT,
            DriverError::NotFound(_) => StatusCode::NOT_FOUND,
            DriverError::AccessDenied => StatusCode::FORBIDDEN,
            DriverError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            DriverError::Backend(_)
            | DriverError::Io(_)
            | DriverError::Http(_)
            | DriverError::Remote { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(serde_json::json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests;
