//! Independent macOS virtual-device inventory and native format control.
//!
//! The HAL owns both PCM ring writers and its existing native clock. This crate
//! only publishes bounded device inventory over XPC and observes native ACKs on
//! control; it has no graph, DSP, recording, or plugin lifetime dependency.
#![cfg(target_os = "macos")]

mod device;
mod dispatch;
mod endpoint_registry;
mod xpc;

use endpoint_registry::{CallbackEndpoint, EndpointRegistry, EndpointSpec, OwnedInventory};
use mixrelay_audio_io::{
    NativeDeviceFormats, VirtualEndpointCapabilities, VirtualEndpointDeviceSettings,
    VirtualEndpointId, VirtualEndpointSettings, VirtualEndpointSpec,
};
use mixrelay_driver_control::{DriverBackend, DriverDeviceConfig, DriverError, DriverState};
use mixrelay_macos_transport::{
    endpoint_identity::{endpoint_uid, parse_endpoint_id},
    endpoint_transport::{EndpointClockSnapshot, mr_endpoint_transport_clock_snapshot},
};
use parking_lot::Mutex;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

const DRIVER_BUNDLE: &str = "/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver";
const ACK_TIMEOUT: Duration = Duration::from_secs(6);
static SERVICE: OnceLock<Service> = OnceLock::new();
static START: Mutex<()> = Mutex::new(());

struct PreviousDeviceFormat {
    id: [u8; 16],
    formats: NativeDeviceFormats,
    rate: Option<u32>,
}

struct Service {
    registry: EndpointRegistry,
    published: Mutex<Option<OwnedInventory>>,
    generation: Mutex<u64>,
    peers: Mutex<xpc::Peers>,
    dispatch: Mutex<Option<dispatch::Dispatch>>,
    configuration: Mutex<Vec<DriverDeviceConfig>>,
    transaction: Mutex<()>,
}

impl Service {
    fn publish(&self, mut snapshot: OwnedInventory) -> Result<u64, DriverError> {
        let mut generation = self.generation.lock();
        *generation = generation
            .checked_add(1)
            .ok_or_else(|| DriverError::Backend("inventory generation exhausted".into()))?;
        snapshot.set_generation(*generation);
        *self.published.lock() = Some(snapshot);
        self.peers.lock().send_inventory(self);
        Ok(*generation)
    }
}

/// One control-plane adapter in the independently launched Driver service.
/// Dropping an adapter never removes devices; only an explicit desired-inventory
/// replacement can do so. XPC resources live for the service process lifetime.
pub struct MacDriver {
    service: &'static Service,
}

impl MacDriver {
    /// Starts the dedicated `io.mixrelay.audio.driver` XPC inventory owner.
    /// No empty inventory is sent before the manager loads persisted settings.
    /// # Errors
    /// Returns listener/timer errors. An installed HAL may connect after startup.
    pub fn open() -> Result<Self, DriverError> {
        let _start = START.lock();
        let service = SERVICE.get_or_init(|| Service {
            registry: EndpointRegistry::new(),
            published: Mutex::new(None),
            generation: Mutex::new(0),
            peers: Mutex::new(xpc::Peers::default()),
            dispatch: Mutex::new(None),
            configuration: Mutex::new(Vec::new()),
            transaction: Mutex::new(()),
        });
        let mut dispatch = service.dispatch.lock();
        if dispatch.is_none() {
            *dispatch = Some(
                dispatch::Dispatch::start()
                    .map_err(|error| DriverError::Unavailable(error.to_string()))?,
            );
        }
        drop(dispatch);
        Ok(Self { service })
    }

    fn bootstrap(&mut self) -> Result<(), DriverError> {
        let observed = device::observed_inventory()?;
        if observed.is_empty() {
            // A genuinely empty HAL remains unpublished until the manager's
            // default or explicit stored inventory is applied.
            return Ok(());
        }
        let service = self.service;
        let _transaction = service.transaction.lock();
        let specs = observed
            .iter()
            .map(native_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let prepared = service.registry.prepare_inventory_change(&specs, None)?;
        let pins = prepared.callback_pins();
        let generation = service.publish(prepared.snapshot()?)?;
        if let Err(error) = wait_for(|| {
            service.peers.lock().acknowledged(generation)
                && inventory_applied(&pins, &specs, &specs)
        }) {
            // Same actual IDs/name/geometry only: bootstrap never removes a
            // device or writes its format/rate. Preserve that HAL configuration
            // even if attaching this service's storage cannot be acknowledged.
            service.published.lock().take();
            return Err(error);
        }
        prepared.commit()?;
        *service.configuration.lock() = observed;
        service.registry.collect();
        Ok(())
    }

    fn replace_inventory(&mut self, desired: &[DriverDeviceConfig]) -> Result<(), DriverError> {
        self.replace_inventory_change(desired, None)
    }

    fn replace_inventory_change(
        &mut self,
        desired: &[DriverDeviceConfig],
        replacement: Option<[u8; 16]>,
    ) -> Result<(), DriverError> {
        let service = self.service;
        let _transaction = service.transaction.lock();
        let specs = desired
            .iter()
            .map(native_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let previous = service.configuration.lock().clone();
        let old_specs = previous
            .iter()
            .map(native_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let old_formats: Vec<_> = old_specs
            .iter()
            .map(|spec| PreviousDeviceFormat {
                id: spec.id,
                formats: device::native_formats(spec.id),
                rate: device::rate(spec.id),
            })
            .collect();
        let prepared = service
            .registry
            .prepare_inventory_change(&specs, replacement)?;
        let pins = prepared.callback_pins();
        let generation = service.publish(prepared.snapshot()?)?;
        let result = wait_for(|| {
            service.peers.lock().acknowledged(generation)
                && inventory_applied(&pins, &old_specs, &specs)
        })
        .and_then(|()| apply_formats(desired, &previous));
        if let Err(error) = result {
            // Rollback uses a fresh publication generation, so HAL fencing never
            // mistakes the old active mappings for a stale provisional message.
            let generation = service.publish(service.registry.snapshot()?)?;
            let old_pins: Vec<_> = old_specs
                .iter()
                .filter_map(|spec| service.registry.callback(spec.id))
                .collect();
            let restored = wait_for(|| {
                service.peers.lock().acknowledged(generation)
                    && inventory_applied(&old_pins, &specs, &old_specs)
            })
            .and_then(|()| restore_formats(&old_formats));
            if let Err(rollback) = restored {
                return Err(DriverError::Backend(format!(
                    "{error}; rollback failed: {rollback}"
                )));
            }
            return Err(error);
        }
        prepared.commit()?;
        *service.configuration.lock() = desired.to_vec();
        service.registry.collect();
        Ok(())
    }
}

impl DriverBackend for MacDriver {
    fn settings(&self) -> Result<VirtualEndpointSettings, DriverError> {
        if self.state() != DriverState::Ready {
            return Err(DriverError::Unavailable(
                "MixRelay HAL is not connected".into(),
            ));
        }
        let mut settings = VirtualEndpointSettings::configurable(&capabilities());
        settings.devices = self
            .service
            .configuration
            .lock()
            .iter()
            .map(|config| {
                let id = identity(&config.spec.id)?;
                let rate = device::rate(id);
                let supported = mixrelay_audio_io::format_contract::supported_rates(|rate| {
                    device::supports(id, rate)
                });
                Ok(VirtualEndpointDeviceSettings {
                    spec: observed_spec(&config.spec, rate, &supported),
                    native_device_ids: device::device(id)
                        .map(|_| vec![endpoint_uid(id)])
                        .unwrap_or_default(),
                    observed_sample_rate: rate,
                    can_set_sample_rate: rate.is_some() && !supported.is_empty(),
                    supported_sample_rates: supported,
                    native_formats: device::native_formats(id),
                })
            })
            .collect::<Result<Vec<_>, DriverError>>()?;
        Ok(settings)
    }

    fn replace(&mut self, desired: &[DriverDeviceConfig]) -> Result<(), DriverError> {
        if self.state() != DriverState::Ready {
            return Err(DriverError::Unavailable(
                "MixRelay HAL is not connected".into(),
            ));
        }
        let needs_bootstrap = self.service.published.lock().is_none();
        if needs_bootstrap {
            self.bootstrap()?;
        }
        self.replace_inventory(desired)
    }

    fn state(&self) -> DriverState {
        if !std::path::Path::new(DRIVER_BUNDLE).is_dir() {
            DriverState::NotInstalled
        } else if self.service.peers.lock().driver_connected() {
            DriverState::Ready
        } else {
            DriverState::InstalledNotLoaded
        }
    }
}

fn capabilities() -> VirtualEndpointCapabilities {
    VirtualEndpointCapabilities {
        max_render_channels: 64,
        max_capture_channels: 64,
        supported_sample_rates: mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
        supports_duplex: true,
        supports_dynamic_creation: true,
    }
}

fn observed_spec(
    configured: &VirtualEndpointSpec,
    rate: Option<u32>,
    supported: &[u32],
) -> VirtualEndpointSpec {
    let mut spec = configured.clone();
    if let Some(rate) = rate {
        spec.default_sample_rate = rate;
    }
    spec.supported_sample_rates = supported.to_vec();
    spec
}

fn identity(id: &VirtualEndpointId) -> Result<[u8; 16], DriverError> {
    parse_endpoint_id(id.as_str()).ok_or_else(|| {
        DriverError::Invalid(format!(
            "device identity must be a canonical lowercase UUID: {}",
            id.as_str()
        ))
    })
}

fn native_spec(config: &DriverDeviceConfig) -> Result<EndpointSpec, DriverError> {
    config
        .spec
        .validate(&capabilities())
        .map_err(|error| DriverError::Invalid(error.to_string()))?;
    for (format, stream) in [
        (config.render_format, config.spec.render),
        (config.capture_format, config.spec.capture),
    ] {
        if let Some(format) = format
            && (stream.is_none_or(|stream| stream.channels != format.channels)
                || format.sample_rate != config.spec.default_sample_rate)
        {
            return Err(DriverError::Invalid(
                "directional format must match device channels and shared nominal rate".into(),
            ));
        }
    }
    Ok(EndpointSpec {
        id: identity(&config.spec.id)?,
        display_name: config.spec.display_name.clone(),
        sample_rate: config.spec.default_sample_rate,
        capacity_frames: 16_384,
        render_channels: config
            .spec
            .render
            .map_or(0, |stream| u32::from(stream.channels)),
        capture_channels: config
            .spec
            .capture
            .map_or(0, |stream| u32::from(stream.channels)),
    })
}

fn inventory_applied(
    pins: &[CallbackEndpoint<'_>],
    before: &[EndpointSpec],
    after: &[EndpointSpec],
) -> bool {
    pins.iter().all(|pin| {
        let mut clock = EndpointClockSnapshot::default();
        // A fresh mapping becomes ready only when the HAL has actually created
        // or performed its attachment under Device::freeze, not upon XPC receipt.
        (unsafe { mr_endpoint_transport_clock_snapshot(pin.transport(), &raw mut clock) })
            && device::device(pin.spec().id).is_some()
    }) && before
        .iter()
        .filter(|spec| !after.iter().any(|next| next.id == spec.id))
        .all(|spec| device::device(spec.id).is_none())
}

fn wait_for(mut acknowledged: impl FnMut() -> bool) -> Result<(), DriverError> {
    let deadline = Instant::now() + ACK_TIMEOUT;
    loop {
        if acknowledged() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(DriverError::Backend(
                "HAL configuration perform/readback timed out".into(),
            ));
        }
        // Control-only ACK polling, never audio pacing or a callback wait.
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn apply_formats(
    desired: &[DriverDeviceConfig],
    previous: &[DriverDeviceConfig],
) -> Result<(), DriverError> {
    for config in desired {
        let id = identity(&config.spec.id)?;
        let old = previous.iter().find(|old| old.spec.id == config.spec.id);
        if old.is_none_or(|old| old.spec.default_sample_rate != config.spec.default_sample_rate) {
            let rate = config.spec.default_sample_rate;
            if !device::set_rate(id, rate) {
                return Err(DriverError::Backend("native nominal rate rejected".into()));
            }
            wait_for(|| device::rate(id) == Some(rate))?;
        }
        for (render, wanted, old_format) in [
            (
                true,
                config.render_format,
                old.and_then(|old| old.render_format),
            ),
            (
                false,
                config.capture_format,
                old.and_then(|old| old.capture_format),
            ),
        ] {
            if let Some(wanted) = wanted.filter(|wanted| Some(*wanted) != old_format) {
                if !device::set_format(id, render, wanted) {
                    return Err(DriverError::Backend(
                        "native directional ASBD rejected".into(),
                    ));
                }
                wait_for(|| {
                    let formats = device::native_formats(id);
                    (if render {
                        formats.render.current
                    } else {
                        formats.capture.current
                    }) == Some(wanted)
                })?;
            }
        }
    }
    Ok(())
}

fn restore_formats(previous: &[PreviousDeviceFormat]) -> Result<(), DriverError> {
    for PreviousDeviceFormat { id, formats, rate } in previous {
        if let Some(rate) = rate {
            if !device::set_rate(*id, *rate) {
                return Err(DriverError::Backend(
                    "nominal-rate rollback rejected".into(),
                ));
            }
            wait_for(|| device::rate(*id) == Some(*rate))?;
        }
        for (render, format) in [
            (true, formats.render.current),
            (false, formats.capture.current),
        ] {
            if let Some(format) = format {
                if !device::set_format(*id, render, format) {
                    return Err(DriverError::Backend(
                        "directional-format rollback rejected".into(),
                    ));
                }
                wait_for(|| {
                    let current = device::native_formats(*id);
                    (if render {
                        current.render.current
                    } else {
                        current.capture.current
                    }) == Some(format)
                })?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
