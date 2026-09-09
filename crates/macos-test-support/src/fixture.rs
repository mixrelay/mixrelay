//! Process-private aggregate fixture with Rust-owned native lifetime.
#[cfg(test)]
pub(super) use crate::interactive_fixture::InteractiveFixture;
use crate::private_device::PrivateDevice;

pub struct DeviceFixture {
    device: PrivateDevice,
}
impl DeviceFixture {
    /// Creates a process-private aggregate of the installed `ToneNari` endpoint.
    /// # Panics
    /// Native creation or readiness failure fails the test.
    #[must_use]
    pub fn start() -> (Self, String) {
        let device = PrivateDevice::new().unwrap_or_else(|error| panic!("{error}"));
        let uid = device.uid().to_owned();
        (Self { device }, uid)
    }
    /// Connects or removes only the owned private aggregate.
    /// # Panics
    /// Invalid commands or failed native transitions fail the test.
    pub fn command(&mut self, command: &str) {
        let result = match command {
            "connect" => self.device.connect(),
            "disconnect" => self.device.disconnect(),
            _ => panic!("unsupported fixture command"),
        };
        result.unwrap_or_else(|error| panic!("{error}"));
    }
}
