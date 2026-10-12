//! User-requested licensing, outside audio callbacks and graph mutation.
use mixrelay_licensing::{AccessSnapshot, AccessStatus, LicenseError, SecretString};
use secrecy::ExposeSecret as _;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager as _;
mod configuration;
mod recovery;
mod store;
pub(crate) use store::LicensingState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LicensingStatus {
    #[serde(flatten)]
    access: AccessSnapshot,
    configured: bool,
    has_activation: bool,
}

pub(crate) fn now() -> Result<u64, LicenseError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| LicenseError::ClockInvalid)
}
pub(crate) fn allows_audio(snapshot: &AccessSnapshot) -> bool {
    matches!(
        snapshot.status,
        AccessStatus::Free | AccessStatus::Licensed | AccessStatus::ClockInvalid
    )
}
async fn snapshot(state: Arc<LicensingState>) -> Result<AccessSnapshot, LicenseError> {
    tauri::async_runtime::spawn_blocking(move || state.snapshot(now()?))
        .await
        .map_err(|_| LicenseError::StorageUnavailable)?
}

/// Observe paid access for app-owned inventory changes without starting graph audio.
pub(crate) async fn has_paid_access(state: Arc<LicensingState>) -> Result<bool, LicenseError> {
    let _gate = state.mutations.lock().await;
    snapshot(Arc::clone(&state))
        .await
        .map(|snapshot| snapshot.status == AccessStatus::Licensed)
}
fn status(app: &tauri::AppHandle, access: AccessSnapshot) -> Result<LicensingStatus, LicenseError> {
    Ok(LicensingStatus {
        access,
        configured: configuration::configuration()?.is_some(),
        has_activation: app
            .state::<Arc<LicensingState>>()
            .saved_activation()?
            .is_some(),
    })
}
async fn current(
    app: &tauri::AppHandle,
    state: Arc<LicensingState>,
) -> Result<LicensingStatus, LicenseError> {
    match snapshot(state).await {
        Ok(access) => status(app, access),
        Err(error) => Err(error),
    }
}

async fn prepare_request(
    app: &tauri::AppHandle,
    state: Arc<LicensingState>,
) -> Result<(), LicenseError> {
    let observed = current(app, state).await?;
    if observed.access.status == AccessStatus::ClockInvalid {
        return Err(LicenseError::ClockInvalid);
    }
    Ok(())
}
async fn resume_audio(app: &tauri::AppHandle) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let target = app.clone();
    if app
        .run_on_main_thread(move || {
            let _ = sender.send(crate::start_audio(&target));
        })
        .is_err()
    {
        tracing::warn!("audio start dispatch unavailable");
        return;
    }
    match receiver.await {
        Ok(Ok(())) => {}
        result => tracing::warn!(
            ?result,
            "audio service unavailable; license status retained"
        ),
    }
}

/// Report persisted access without contacting the vendor or revealing the saved key.
#[tauri::command]
pub(crate) async fn licensing_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<LicensingState>>,
) -> Result<LicensingStatus, LicenseError> {
    let _gate = state.mutations.lock().await;
    let result = current(&app, Arc::clone(state.inner())).await?;
    if allows_audio(&result.access) {
        resume_audio(&app).await;
    }
    Ok(result)
}

/// The hosted link is publisher configuration, never supplied by the embedded browser.
#[tauri::command]
pub(crate) fn licensing_checkout() -> Result<Option<String>, LicenseError> {
    Ok(configuration::configuration()?.map(|(_, checkout)| checkout))
}
/// Open the publisher's fixed HTTPS checkout only on explicit user action.
#[tauri::command]
pub(crate) async fn licensing_open_checkout() -> Result<(), LicenseError> {
    let url = licensing_checkout()?.ok_or(LicenseError::InvalidConfiguration)?;
    tauri::async_runtime::spawn_blocking(move || open::that_detached(url))
        .await
        .map_err(|_| LicenseError::Unavailable)?
        .map_err(|_| LicenseError::Unavailable)
}

/// Activate and save one seat; a repeated key validates its existing seat instead.
#[tauri::command]
pub(crate) async fn licensing_activate(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<LicensingState>>,
    key: String,
    instance_name: String,
) -> Result<LicensingStatus, LicenseError> {
    let _gate = state.mutations.lock().await;
    prepare_request(&app, Arc::clone(state.inner())).await?;
    state.preflight_remote_change()?;
    let client = configuration::client()?;
    let key = SecretString::from(key);
    let activation = match state.saved_activation()? {
        Some((saved, seat)) if saved.expose_secret() == key.expose_secret() => {
            match client.validate(&key, seat.instance_id()).await {
                Ok(activation) => activation,
                Err(error @ (LicenseError::Rejected | LicenseError::WrongProduct)) => {
                    clear_paid(&app, Arc::clone(state.inner())).await?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
        Some(_) => return Err(LicenseError::InvalidInput),
        None => match state.recoverable_activation(&key)? {
            Some(seat) => match client.validate(&key, seat.instance_id()).await {
                Ok(activation) => activation,
                Err(error @ (LicenseError::Rejected | LicenseError::WrongProduct)) => {
                    clear_paid(&app, Arc::clone(state.inner())).await?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            },
            None => client.activate(&key, &instance_name).await?,
        },
    };
    let store = Arc::clone(state.inner());
    let access =
        tauri::async_runtime::spawn_blocking(move || store.verified(key, activation, now()?))
            .await
            .map_err(|_| LicenseError::StorageUnavailable)??;
    let result = status(&app, access)?;
    resume_audio(&app).await;
    Ok(result)
}

/// Confirm the saved seat; a failed request does not renew or erase offline access.
#[tauri::command]
pub(crate) async fn licensing_validate(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<LicensingState>>,
) -> Result<LicensingStatus, LicenseError> {
    let _gate = state.mutations.lock().await;
    prepare_request(&app, Arc::clone(state.inner())).await?;
    state.preflight_remote_change()?;
    let (key, seat) = state
        .saved_activation()?
        .ok_or(LicenseError::InvalidInput)?;
    let result = configuration::client()?
        .validate(&key, seat.instance_id())
        .await;
    let activation = match result {
        Ok(activation) => activation,
        Err(error @ (LicenseError::Rejected | LicenseError::WrongProduct)) => {
            clear_paid(&app, Arc::clone(state.inner())).await?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    let store = Arc::clone(state.inner());
    let access =
        tauri::async_runtime::spawn_blocking(move || store.verified(key, activation, now()?))
            .await
            .map_err(|_| LicenseError::StorageUnavailable)??;
    let result = status(&app, access)?;
    resume_audio(&app).await;
    Ok(result)
}
async fn clear_paid(
    app: &tauri::AppHandle,
    state: Arc<LicensingState>,
) -> Result<LicensingStatus, LicenseError> {
    let access = tauri::async_runtime::spawn_blocking(move || state.clear_paid(now()?))
        .await
        .map_err(|_| LicenseError::StorageUnavailable)??;
    status(app, access)
}

/// Remove only the paid credential after confirmed seat release; retain trial history.
#[tauri::command]
pub(crate) async fn licensing_deactivate(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<LicensingState>>,
) -> Result<LicensingStatus, LicenseError> {
    let _gate = state.mutations.lock().await;
    prepare_request(&app, Arc::clone(state.inner())).await?;
    state.preflight_remote_change()?;
    let (key, seat) = state
        .saved_activation()?
        .ok_or(LicenseError::InvalidInput)?;
    configuration::client()?
        .deactivate(&key, seat.instance_id())
        .await?;
    clear_paid(&app, Arc::clone(state.inner())).await
}
