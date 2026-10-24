//! Classify native initialization failures using the process's microphone consent.
use mixrelay_audio_io::HostAudioBackendError;
use std::sync::mpsc::RecvTimeoutError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputPermission {
    Pending,
    Denied,
    Authorized,
    Unknown,
}

pub(super) fn current() -> InputPermission {
    unsafe extern "C" {
        fn mr_microphone_authorization_status() -> std::ffi::c_long;
    }
    // SAFETY: the shim takes no pointers and only reads this process's public
    // AVFoundation authorization status. It neither requests nor changes access.
    match unsafe { mr_microphone_authorization_status() } {
        0 => InputPermission::Pending,
        1 | 2 => InputPermission::Denied,
        3 => InputPermission::Authorized,
        _ => InputPermission::Unknown,
    }
}

pub(super) fn initialization(
    result: Result<Result<(), String>, RecvTimeoutError>,
    permission: Option<InputPermission>,
    latest: impl FnOnce() -> InputPermission,
) -> Result<(), HostAudioBackendError> {
    if matches!(result, Ok(Ok(()))) {
        return Ok(());
    }
    // First-use consent can consume the readiness budget even when approval
    // arrives just before expiry. A new denial still takes precedence.
    let permission = permission.map(|initial| match latest() {
        InputPermission::Authorized if initial == InputPermission::Pending => {
            InputPermission::Pending
        }
        current => current,
    });
    match result {
        Err(RecvTimeoutError::Timeout) if permission == Some(InputPermission::Pending) => {
            Err(HostAudioBackendError::PermissionPending {
                capability: "microphone".into(),
            })
        }
        error if permission == Some(InputPermission::Denied) => {
            tracing::warn!(
                ?error,
                "CoreAudio initialization failed with microphone access denied"
            );
            Err(HostAudioBackendError::PermissionDenied {
                capability: "microphone".into(),
            })
        }
        error => Err(super::backend_error(format!(
            "CoreAudio device initialization failed: {error:?}"
        ))),
    }
}

#[cfg(test)]
mod tests;
