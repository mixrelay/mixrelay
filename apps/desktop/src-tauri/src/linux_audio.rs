use mixrelay_audio_io::{AudioDeviceDescriptor, CapturableApplication};

/// Lists `PipeWire` application output streams using the same normalized inventory as the engine.
///
/// # Errors
///
/// Returns a diagnostic when the active user `PipeWire` registry cannot be queried.
/// Apps using each `MixRelay` device. `PipeWire` stream ownership is not mapped yet, so no
/// device reports any; the UI then shows the activity as unknown rather than empty.
#[tauri::command]
pub(crate) fn list_device_app_activity()
-> Result<std::collections::BTreeMap<String, Vec<serde_json::Value>>, String> {
    Err("app activity is not available on Linux yet".to_owned())
}

#[tauri::command]
pub(crate) fn list_capturable_applications() -> Result<Vec<CapturableApplication>, String> {
    mixrelay_audio_io::linux::applications().map_err(|error| error.to_string())
}

/// Lists current `PipeWire` audio sources and sinks with their negotiated channel capacities.
///
/// # Errors
///
/// Returns a diagnostic when the active user `PipeWire` registry cannot be queried.
#[tauri::command]
pub(crate) fn list_audio_devices() -> Result<Vec<AudioDeviceDescriptor>, String> {
    mixrelay_audio_io::linux::devices().map_err(|error| error.to_string())
}
