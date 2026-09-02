use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverState {
    NotInstalled,
    Installing,
    RestartRequired,
    InstalledNotLoaded,
    Blocked,
    InvalidPermissions,
    VersionMismatch,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub protocol_version: u16,
    pub driver_state: DriverState,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub underruns: u64,
    pub overruns: u64,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            driver_state: DriverState::NotInstalled,
            sample_rate: None,
            buffer_frames: None,
            underruns: 0,
            overruns: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    DriverNotInstalled,
    DriverNotLoaded,
    DriverPermissionInvalid,
    DriverVersionMismatch,
    AudioDeviceDisconnected,
    BufferUnderrun,
    BufferOverrun,
    PairingRequired,
    Unauthorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_contract_uses_stable_machine_values() {
        let json = serde_json::to_value(AppStatus::default()).expect("status should serialize");

        assert_eq!(json["protocolVersion"], 1);
        assert_eq!(json["driverState"], "not_installed");
        assert!(json["sampleRate"].is_null());
        assert!(json["bufferFrames"].is_null());
    }
}
