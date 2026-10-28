//! Independent macOS virtual-device inventory and native format control.
//!
//! The HAL owns both PCM ring writers and its existing native clock. This crate
//! only publishes bounded device inventory over XPC and observes native ACKs on
//! control; it has no graph, DSP, recording, or plugin lifetime dependency.
#![cfg(target_os = "macos")]

mod device;
mod dispatch;
mod endpoint_registry;
mod settle;
mod xpc;

use endpoint_registry::{CallbackEndpoint, EndpointRegistry, EndpointSpec, OwnedInventory};
use mixrelay_audio_io::{
    NativeDeviceFormats, NativeStreamFormat, VirtualEndpointCapabilities,
    VirtualEndpointDeviceSettings, VirtualEndpointId, VirtualEndpointSettings, VirtualEndpointSpec,
};
use mixrelay_driver_control::{DriverBackend, DriverDeviceConfig, DriverError, DriverState};
use mixrelay_macos_transport::{
    endpoint_identity::{endpoint_uid, parse_endpoint_id},
    endpoint_transport::{EndpointClockSnapshot, mr_endpoint_transport_clock_snapshot},
};
use parking_lot::Mutex;
use settle::{Polling, Progress};
use std::sync::OnceLock;

const DRIVER_BUNDLE: &str = "/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver";
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
    latencies: Mutex<
        std::collections::BTreeMap<
            [u8; 16],
            mixrelay_macos_transport::control_inventory::InputLatency,
        >,
    >,
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
            latencies: Mutex::default(),
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
        if let Err(failure) = settle::step(&Polling, || {
            inventory_progress(service, generation, &pins, &specs, &specs)
        }) {
            // Same actual IDs/name/geometry only: bootstrap never removes a
            // device or writes its format/rate. Preserve that HAL configuration
            // even if attaching this service's storage cannot be acknowledged.
            service.published.lock().take();
            return Err(failure.into());
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
        settle::transact(
            || {
                settle::step(&Polling, || {
                    inventory_progress(service, generation, &pins, &old_specs, &specs)
                })?;
                apply_formats(service, desired)
            },
            // Rollback uses a fresh publication generation, so HAL fencing never
            // mistakes the old active mappings for a stale provisional message.
            || service.publish(service.registry.snapshot()?),
            |generation| {
                let old_pins: Vec<_> = old_specs
                    .iter()
                    .filter_map(|spec| service.registry.callback(spec.id))
                    .collect();
                settle::step(&Polling, || {
                    inventory_progress(service, generation, &old_pins, &specs, &old_specs)
                })?;
                restore_formats(service, &old_formats)
            },
        )?;
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

    fn publish_latency(
        &mut self,
        publication: &mixrelay_driver_control::RuntimeLatency,
    ) -> Result<(), DriverError> {
        let endpoint_id = identity(&publication.endpoint_id)?;
        if !self
            .service
            .configuration
            .lock()
            .iter()
            .any(|config| config.spec.id == publication.endpoint_id)
        {
            return if publication.input_latency_frames.is_none() {
                // A cleared value must not survive to republish on reactivation.
                self.service.latencies.lock().remove(&endpoint_id);
                Ok(())
            } else {
                Err(DriverError::NotFound(publication.endpoint_id.clone()))
            };
        }
        self.service.latencies.lock().insert(
            endpoint_id,
            mixrelay_macos_transport::control_inventory::InputLatency {
                endpoint_id,
                rate: publication.rate,
                engine_pid: publication.engine_lease.process_id,
                lease: publication.engine_lease.generation,
                revision: publication.applied_graph_revision,
                frames: publication.input_latency_frames,
            },
        );
        self.service.peers.lock().send_inventory(self.service);
        Ok(())
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
        // Endpoint rings carry only lifecycle metadata; client PCM queues use the
        // same playback-sized bound. Capture retention lives in the engine's sources.
        capacity_frames: mixrelay_macos_transport::client_pcm::QUEUE_FRAMES,
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

fn inventory_progress(
    service: &Service,
    generation: u64,
    pins: &[CallbackEndpoint<'_>],
    before: &[EndpointSpec],
    after: &[EndpointSpec],
) -> Progress {
    match service.peers.lock().settlement(generation) {
        Some(xpc::Settlement::Applied) => Progress::applied(inventory_applied(pins, before, after)),
        Some(xpc::Settlement::Rejected) => Progress::Rejected,
        None => Progress::Performing,
    }
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

/// Requests the rate and formats `desired` still lacks. The HAL readback, not the
/// service's cached intent, decides what is missing: a Perform reported pending may
/// have settled since, and a DAW may have changed the device.
fn apply_formats(service: &Service, desired: &[DriverDeviceConfig]) -> Result<(), settle::Failure> {
    for config in desired {
        let id = identity(&config.spec.id)?;
        if let Some(rate) = missing(Some(config.spec.default_sample_rate), device::rate(id)) {
            request(
                service,
                id,
                Some(rate),
                || issued(device::set_rate(id, rate), "native nominal rate rejected"),
                || device::rate(id) == Some(rate),
            )?;
        }
        for (render, wanted) in [(true, config.render_format), (false, config.capture_format)] {
            if let Some(wanted) = missing(wanted, current_format(id, render)) {
                request(
                    service,
                    id,
                    None,
                    || {
                        issued(
                            device::set_format(id, render, wanted),
                            "native directional ASBD rejected",
                        )
                    },
                    || current_format(id, render) == Some(wanted),
                )?;
            }
        }
    }
    Ok(())
}

fn restore_formats(
    service: &Service,
    previous: &[PreviousDeviceFormat],
) -> Result<(), settle::Failure> {
    for PreviousDeviceFormat { id, formats, rate } in previous {
        let id = *id;
        if let Some(rate) = missing(*rate, device::rate(id)) {
            request(
                service,
                id,
                Some(rate),
                || issued(device::set_rate(id, rate), "nominal-rate rollback rejected"),
                || device::rate(id) == Some(rate),
            )?;
        }
        for (render, format) in [
            (true, formats.render.current),
            (false, formats.capture.current),
        ] {
            if let Some(format) = missing(format, current_format(id, render)) {
                request(
                    service,
                    id,
                    None,
                    || {
                        issued(
                            device::set_format(id, render, format),
                            "directional-format rollback rejected",
                        )
                    },
                    || current_format(id, render) == Some(format),
                )?;
            }
        }
    }
    Ok(())
}

/// The wanted value when HAL readback does not already report it.
fn missing<T: Copy + PartialEq>(wanted: Option<T>, observed: Option<T>) -> Option<T> {
    wanted.filter(|wanted| observed != Some(*wanted))
}

fn current_format(id: endpoint_registry::EndpointId, render: bool) -> Option<NativeStreamFormat> {
    let formats = device::native_formats(id);
    if render {
        formats.render.current
    } else {
        formats.capture.current
    }
}

/// Issues one rate or format request and waits until the HAL applies or rejects it.
/// Only settlements reported after the request count, so an older rejection is ignored.
fn request(
    service: &Service,
    id: endpoint_registry::EndpointId,
    rate: Option<u32>,
    issue: impl FnOnce() -> Result<(), settle::Failure>,
    applied: impl Fn() -> bool,
) -> Result<(), settle::Failure> {
    let since = service
        .peers
        .lock()
        .configuration(id)
        .map_or(0, |settled| settled.sequence);
    issue()?;
    settle::step(&Polling, || {
        settle::request_progress(
            applied(),
            since,
            service.peers.lock().configuration(id),
            rate,
        )
    })
}

fn issued(accepted: bool, reason: &str) -> Result<(), settle::Failure> {
    if accepted {
        Ok(())
    } else {
        Err(rejected(reason))
    }
}

fn rejected(reason: &str) -> settle::Failure {
    settle::Failure::Rejected(DriverError::Backend(reason.into()))
}

#[cfg(test)]
mod tests;
