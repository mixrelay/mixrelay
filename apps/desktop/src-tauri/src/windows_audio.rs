use mixrelay_audio_io::{AudioDeviceDescriptor, CapturableApplication};

#[tauri::command]
pub(crate) fn list_capturable_applications() -> Result<Vec<CapturableApplication>, String> {
    mixrelay_audio_io::windows::applications().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn list_audio_devices() -> Result<Vec<AudioDeviceDescriptor>, String> {
    mixrelay_audio_io::windows::devices().map_err(|error| error.to_string())
}
