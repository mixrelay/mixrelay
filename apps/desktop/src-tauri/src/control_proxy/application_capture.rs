//! Desktop-owned taps follow the complete engine execution manifest, never UI selection.
use std::sync::{Arc, Weak, atomic::Ordering};
use std::time::Duration;

use mixrelay_control_protocol::{ApplicationCaptureManifest, GraphRuntimeStatus};

use super::{
    ControlProxyClient, ControlProxyResponse, Method, RequestDelivery, Value,
    application_capture_error,
};
use crate::macos_audio::{ApplicationCaptureState, PreparedApplicationCaptures};

const MANIFEST: &str = "/api/v1/application-capture/manifest";
const PREVIEW: &str = "/api/v1/application-capture/preview";

pub(super) async fn synchronize_response(
    client: &ControlProxyClient,
    captures: &Arc<ApplicationCaptureState>,
    response: ControlProxyResponse,
) -> Result<ControlProxyResponse, String> {
    if !response.is_success() {
        return Ok(response);
    }
    let manifest = client.request(Method::GET, MANIFEST, None).await?;
    if !manifest.is_success() {
        return Ok(manifest);
    }
    let manifest = decode_manifest(manifest.body)?;
    let prepared = match prepare(captures, manifest).await {
        Ok(prepared) => prepared,
        Err(error) => return Ok(application_capture_error(&error)),
    };
    stage(client, captures, prepared).await?;
    Ok(response)
}

pub(super) async fn apply(
    client: &ControlProxyClient,
    captures: &Arc<ApplicationCaptureState>,
    document: Value,
) -> Result<ControlProxyResponse, String> {
    let preview = client
        .request(Method::POST, PREVIEW, Some(document.clone()))
        .await?;
    if !preview.is_success() {
        return Ok(preview);
    }
    let manifest = decode_manifest(preview.body)?;
    let revision = manifest.revision;
    if let Err(error) = reserve(captures, manifest).await {
        return Ok(application_capture_error(&error));
    }
    // A cancelled command future or lost PUT response cannot destroy its taps:
    // the reservation belongs to managed state before the request starts.
    start_monitor(client, captures);
    let result = client
        .request(Method::PUT, "/api/v1/graph", Some(document))
        .await;
    if matches!(&result, Ok(response) if definitively_rejected(response)) {
        reject_reserved(captures).await?;
        return result;
    }
    if !matches!(&result, Ok(response) if response.is_success()) {
        return unresolved_result(result);
    }
    let response = result?;
    validate_ack(&response.body, revision).map_err(|error| {
        format!("{error}; application captures retained until engine confirmation")
    })?;
    accept_reserved(captures).await?;
    Ok(response)
}

fn unresolved_result(
    result: Result<ControlProxyResponse, String>,
) -> Result<ControlProxyResponse, String> {
    match result {
        Ok(mut response) => {
            if let Some(parameters) = response
                .body
                .get_mut("parameters")
                .and_then(Value::as_object_mut)
            {
                parameters.insert(
                    "captureOutcome".into(),
                    Value::String("pending_confirmation".into()),
                );
            }
            Ok(response)
        }
        Err(error) => Err(format!(
            "{error}; application captures retained until engine confirmation"
        )),
    }
}

fn definitively_rejected(response: &ControlProxyResponse) -> bool {
    response.delivery == RequestDelivery::NotSent
        || (response.delivery == RequestDelivery::ServerResponse
            && (400..500).contains(&response.status))
}

fn decode_manifest(body: Value) -> Result<ApplicationCaptureManifest, String> {
    serde_json::from_value(body)
        .map_err(|error| format!("invalid application capture manifest: {error}"))
}

fn validate_ack(body: &Value, revision: u64) -> Result<(), String> {
    if body.get("revision").and_then(Value::as_u64) != Some(revision) {
        return Err("graph acceptance does not match prepared capture revision".into());
    }
    Ok(())
}

async fn prepare(
    captures: &Arc<ApplicationCaptureState>,
    manifest: ApplicationCaptureManifest,
) -> Result<PreparedApplicationCaptures, String> {
    let captures = Arc::clone(captures);
    tauri::async_runtime::spawn_blocking(move || captures.prepare(&manifest))
        .await
        .map_err(|error| format!("capture preparation task failed: {error}"))?
}

async fn reserve(
    captures: &Arc<ApplicationCaptureState>,
    manifest: ApplicationCaptureManifest,
) -> Result<(), String> {
    let captures = Arc::clone(captures);
    tauri::async_runtime::spawn_blocking(move || captures.reserve(&manifest))
        .await
        .map_err(|error| format!("capture reservation task failed: {error}"))?
}

async fn reject_reserved(captures: &Arc<ApplicationCaptureState>) -> Result<(), String> {
    let captures = Arc::clone(captures);
    tauri::async_runtime::spawn_blocking(move || captures.reject_reserved())
        .await
        .map_err(|error| format!("capture rejection task failed: {error}"))?
}

async fn accept_reserved(captures: &Arc<ApplicationCaptureState>) -> Result<(), String> {
    let captures = Arc::clone(captures);
    tauri::async_runtime::spawn_blocking(move || captures.accept_reserved())
        .await
        .map_err(|error| format!("capture acceptance task failed: {error}"))?
}

async fn stage(
    client: &ControlProxyClient,
    captures: &Arc<ApplicationCaptureState>,
    prepared: PreparedApplicationCaptures,
) -> Result<(), String> {
    stage_owner(captures, prepared).await?;
    start_monitor(client, captures);
    Ok(())
}

fn start_monitor(client: &ControlProxyClient, captures: &Arc<ApplicationCaptureState>) {
    // The caller owns capture_gate. The monitor clears this flag under the same
    // gate, so a new stage cannot lose its wake-up while the previous monitor exits.
    if !captures.monitor_active.swap(true, Ordering::AcqRel) {
        tauri::async_runtime::spawn(monitor(client.clone(), Arc::downgrade(captures)));
    }
}

async fn stage_owner(
    captures: &Arc<ApplicationCaptureState>,
    prepared: PreparedApplicationCaptures,
) -> Result<(), String> {
    let owner = Arc::clone(captures);
    tauri::async_runtime::spawn_blocking(move || owner.stage(prepared))
        .await
        .map_err(|error| format!("capture staging task failed: {error}"))?
}

async fn monitor(client: ControlProxyClient, weak: Weak<ApplicationCaptureState>) {
    let mut last_error = None;
    loop {
        {
            let _guard = client.capture_gate.lock().await;
            let Some(captures) = weak.upgrade() else {
                return;
            };
            if !captures.has_pending().unwrap_or(true) {
                captures.monitor_active.store(false, Ordering::Release);
                return;
            }
        }
        let error = observe_ack(&client, &weak).await.err();
        if error != last_error
            && let Some(error) = &error
        {
            tracing::warn!(%error, "application capture activation is awaiting engine acknowledgement");
        }
        let interval = if error.is_some() { 1000 } else { 100 };
        last_error = error;
        tokio::time::sleep(Duration::from_millis(interval)).await;
    }
}

async fn observe_ack(
    client: &ControlProxyClient,
    weak: &Weak<ApplicationCaptureState>,
) -> Result<(), String> {
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        client.request(Method::GET, "/api/v1/graph/status", None),
    )
    .await
    .map_err(|_| "capture acknowledgement request timed out".to_owned())??;
    if !response.is_success() {
        return Err(format!("capture acknowledgement HTTP {}", response.status));
    }
    let status: GraphRuntimeStatus = serde_json::from_value(response.body)
        .map_err(|error| format!("invalid capture acknowledgement: {error}"))?;
    let _guard = client.capture_gate.lock().await;
    let Some(captures) = weak.upgrade() else {
        return Ok(());
    };
    if captures.requires_manifest(status.submitted_revision)? {
        // Another controller may have advanced execution before this ACK. Its
        // authoritative manifest supersedes stale prepared sources, not UI state.
        let manifest = client.request(Method::GET, MANIFEST, None).await?;
        if !manifest.is_success() {
            return Err(format!("capture manifest HTTP {}", manifest.status));
        }
        let prepared = prepare(&captures, decode_manifest(manifest.body)?).await?;
        stage_owner(&captures, prepared).await?;
    }
    tauri::async_runtime::spawn_blocking(move || captures.acknowledge(status.active_revision))
        .await
        .map_err(|error| format!("capture acknowledgement task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_must_match_prepared_revision_exactly() {
        for revision in [0, 6, 8] {
            assert!(validate_ack(&serde_json::json!({"revision":revision}), 7).is_err());
        }
        assert!(validate_ack(&serde_json::json!({"revision":7}), 7).is_ok());
        assert!(validate_ack(&serde_json::json!({}), 7).is_err());
    }

    #[test]
    fn authored_document_is_not_a_capture_manifest() {
        assert!(
            decode_manifest(serde_json::json!({
                "schemaVersion":2,"revision":7,"nodes":[],"edges":[]
            }))
            .is_err()
        );
    }
}
