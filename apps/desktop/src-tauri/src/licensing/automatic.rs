//! Startup and first editor connection share one persisted background attempt.

use super::{LicensingState, configuration, operations};
use mixrelay_licensing::{LicenseClient, LicenseError};
use std::sync::{Arc, atomic::Ordering};
use tauri::Manager as _;

pub(crate) fn start(app: &tauri::AppHandle) {
    let state = Arc::clone(app.state::<Arc<LicensingState>>().inner());
    spawn(state);
}

pub(crate) fn first_connection(app: &tauri::AppHandle) {
    let state = Arc::clone(app.state::<Arc<LicensingState>>().inner());
    // The runtime owns this flag. Repeated hello probes are local reads, not new attempts.
    if !state.first_connection.swap(true, Ordering::Relaxed) {
        spawn(state);
    }
}

fn spawn(state: Arc<LicensingState>) {
    tauri::async_runtime::spawn(async move {
        let Ok(client) = configuration::client() else {
            return;
        };
        // Keychain work runs outside the UI and audio threads. There is no retry or timer.
        let _ = tauri::async_runtime::spawn_blocking(move || {
            tauri::async_runtime::block_on(check(&state, &client))
        })
        .await;
    });
}

pub(super) async fn check(
    state: &LicensingState,
    client: &LicenseClient,
) -> Result<(), LicenseError> {
    let _gate = state.mutations.lock().await;
    if state.begin_automatic_check()? {
        state.preflight_remote_change()?;
        // Transient failures leave the saved lease untouched and produce no UI event/log.
        let _ = operations::validate_saved(state, client).await;
    }
    Ok(())
}
