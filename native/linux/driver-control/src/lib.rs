//! Independent per-user virtual-device ownership and native-clock loopback.
//! No graph processor, plugin, recorder or application-capture lifetime is retained.
#![cfg(target_os = "linux")]

mod device;
mod loopback;
#[cfg(test)]
mod tests;

use device::{DevicePreparation, DeviceRuntime};
use mixrelay_audio_io::{VirtualEndpointId, VirtualEndpointSettings};
use mixrelay_driver_control::{DriverBackend, DriverDeviceConfig, DriverError, DriverState};
use mixrelay_pipewire::Connection;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The installed user-session service's unique native device owner.
/// Drop drains every loopback callback before its streams/connections disappear.
pub struct LinuxDriver {
    devices: BTreeMap<String, DeviceRuntime>,
    unavailable: bool,
    // Release all per-device callbacks before the control connection.
    _connection: Connection,
}

/// Stable canonical node-name root, retaining the existing native device identity.
#[must_use]
pub fn native_name(id: &VirtualEndpointId) -> String {
    format!(
        "io.mixrelay.endpoint.{:x}",
        Sha256::digest(id.as_str().as_bytes())
    )
}

/// Published directional identities, never graph-node IDs or implicit defaults.
#[must_use]
pub fn native_names(spec: &mixrelay_audio_io::VirtualEndpointSpec) -> Vec<String> {
    let name = native_name(&spec.id);
    spec.render
        .iter()
        .map(|_| format!("{name}.render"))
        .chain(spec.capture.iter().map(|_| format!("{name}.capture")))
        .collect()
}

impl LinuxDriver {
    /// Connects to the current user's `PipeWire` session without starting a graph engine.
    /// # Errors
    /// Returns the actual native connection/start failure.
    pub fn open() -> Result<Self, DriverError> {
        let mut connection = Connection::new(c"MixRelay Driver").map_err(backend)?;
        connection.start().map_err(backend)?;
        Ok(Self {
            devices: BTreeMap::new(),
            unavailable: false,
            _connection: connection,
        })
    }

    fn restore(&mut self, previous: Vec<DevicePreparation>) -> Result<(), DriverError> {
        for preparation in previous {
            let key = preparation.config.spec.id.as_str().to_owned();
            match DeviceRuntime::open(preparation) {
                Ok(runtime) => {
                    self.devices.insert(key, runtime);
                }
                Err(error) => {
                    self.unavailable = true;
                    return Err(DriverError::Unavailable(format!(
                        "native rollback failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl DriverBackend for LinuxDriver {
    fn settings(&self) -> Result<VirtualEndpointSettings, DriverError> {
        let mut settings = VirtualEndpointSettings::configurable(&device::capabilities());
        settings.devices = self
            .devices
            .values()
            .map(DeviceRuntime::settings)
            .collect::<Result<_, _>>()?;
        Ok(settings)
    }

    fn replace(&mut self, desired: &[DriverDeviceConfig]) -> Result<(), DriverError> {
        let mut wanted = BTreeMap::new();
        for config in desired {
            let key = config.spec.id.as_str().to_owned();
            if wanted.insert(key, config).is_some() {
                return Err(DriverError::Invalid(
                    "duplicate native device identity".into(),
                ));
            }
            device::validate(config)?;
        }
        let mut prepared = Vec::new();
        let mut previous = Vec::new();
        for (key, config) in &wanted {
            if self
                .devices
                .get(key)
                .is_some_and(|runtime| runtime.matches_config(config) && !runtime.failed())
            {
                continue;
            }
            prepared.push(DevicePreparation::new(config)?);
            if let Some(runtime) = self.devices.get(key) {
                previous.push(DevicePreparation::new(&runtime.config)?);
            }
        }
        // All strings, buffers and rollback intent exist before draining changed owners.
        // Never export two live acquisitions with the same canonical node name.
        for preparation in &prepared {
            self.devices.remove(preparation.config.spec.id.as_str());
        }
        let mut staged = BTreeMap::new();
        for preparation in prepared {
            let key = preparation.config.spec.id.as_str().to_owned();
            match DeviceRuntime::open(preparation) {
                Ok(runtime) => {
                    staged.insert(key, runtime);
                }
                Err(error) => {
                    drop(staged); // drain desired owners before recreating previous identities
                    self.restore(previous)?;
                    return Err(error);
                }
            }
        }
        self.devices.extend(staged);
        self.devices.retain(|id, _| wanted.contains_key(id));
        self.unavailable = false;
        Ok(())
    }

    fn recover(&mut self, desired: &[DriverDeviceConfig]) -> Result<bool, DriverError> {
        if !self.unavailable && !self.devices.values().any(DeviceRuntime::failed) {
            return Ok(false);
        }
        self.replace(desired)?;
        Ok(true)
    }

    fn state(&self) -> DriverState {
        if self.unavailable || self.devices.values().any(|device| !device.ready()) {
            DriverState::Unavailable
        } else {
            DriverState::Ready
        }
    }
}

fn backend(error: impl std::fmt::Display) -> DriverError {
    DriverError::Backend(error.to_string())
}
