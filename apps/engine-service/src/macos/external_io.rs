//! Graph input alignment and registry/device transfers at the native boundary.
#[cfg(test)]
mod tests;
use super::endpoint_service;
use mixrelay_audio_engine::{ExternalBinding, ExternalIo};
use mixrelay_audio_io::process::{ProcessCycle, TransferOutcome};

pub(super) struct MacEngineExternalIo<'a> {
    pub(super) sources: Option<&'a mixrelay_macos_transport::source_registry::SourceRegistry>,
    pub(super) alignment: mixrelay_audio_io::capture_alignment::CaptureAlignment,
    capture_time: Option<u64>,
    pub(super) align_inputs: bool,
    devices: &'a crate::macos_devices::DeviceSet,
    /// The admitted cycle's one host label for all of its device transfers.
    host: u64,
}

impl<'a> MacEngineExternalIo<'a> {
    pub(super) fn new(devices: &'a crate::macos_devices::DeviceSet) -> Self {
        Self {
            sources: endpoint_service::source_registry(),
            devices,
            host: 0,
            alignment: mixrelay_audio_io::capture_alignment::CaptureAlignment::default(),
            capture_time: None,
            align_inputs: false,
        }
    }

    fn source_active(&self, binding: &ExternalBinding) -> bool {
        let id = binding.node_id().as_str();
        self.devices.binding_present(id, true)
            || ((binding.kind() == mixrelay_audio_graph::NodeKind::ApplicationCapture
                || binding.virtual_endpoint_id().is_some())
                && binding_sources(binding, self.sources)
                    .and_then(|sources| sources.clock(binding_token(binding)))
                    .is_some_and(|clock| clock.running != 0))
    }
}

impl ExternalIo for MacEngineExternalIo<'_> {
    fn source_epoch(&self, binding: &ExternalBinding) -> Option<u64> {
        if self
            .devices
            .binding_present(binding.node_id().as_str(), true)
        {
            None
        } else {
            application_epoch(binding, self.sources)
        }
    }

    fn begin_cycle(
        &mut self,
        cycle: &ProcessCycle,
        controls: mixrelay_audio_engine::ExternalControlState<'_>,
    ) -> mixrelay_audio_engine::CycleAdmission {
        self.host = self.devices.graph_host();
        for (binding, enabled) in controls.sources() {
            self.devices
                .set_binding_active(binding.node_id().as_str(), true, enabled);
        }
        for (binding, enabled) in controls.sinks() {
            self.devices
                .set_binding_active(binding.node_id().as_str(), false, enabled);
        }
        let mut active = 0;
        let mut latest = None;
        let mut source_period = None;
        let count = controls
            .sources()
            .filter(|(binding, enabled)| {
                *enabled && binding.is_routed_source() && self.source_active(binding)
            })
            .count();
        self.devices
            .reserve_capture_alignment(cycle.sample_rate, cycle.frames, count > 0);
        for (binding, enabled) in controls
            .sources()
            .filter(|(binding, enabled)| *enabled && binding.is_routed_source())
        {
            let _ = enabled;
            let id = binding.node_id().as_str();
            let token = binding_token(binding);
            let sources = binding_sources(binding, self.sources);
            let time = self.devices.capture_time(id, cycle.frames).or_else(|| {
                sources
                    .and_then(|sources| sources.source_time(token, cycle.sample_rate, cycle.frames))
            });
            if self.source_active(binding) {
                active += 1;
            }
            if let Some(time) = time {
                latest = Some(latest.map_or(time, |old: u64| old.min(time)));
            }
            source_period =
                source_period.or_else(|| sources.and_then(|sources| sources.source_period(token)));
        }
        self.align_inputs = active > 0;
        self.capture_time = if self.align_inputs {
            // A source without valid coordinates remains silent independently;
            // it must not hold every other route in startup priming forever.
            let period = self
                .devices
                .frame_period(cycle.sample_rate)
                .or(source_period)
                .unwrap_or(1e9 / f64::from(cycle.sample_rate));
            self.alignment.begin(*cycle, latest, period)
        } else {
            None
        };
        mixrelay_audio_engine::CycleAdmission::Process
    }

    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if !self
            .devices
            .binding_present(binding.node_id().as_str(), true)
            && virtual_source_idle(
                binding,
                binding_sources(binding, self.sources),
                (cycle.sample_rate, cycle.frames),
            )
        {
            destination.fill(0.0);
            return TransferOutcome {
                activity: mixrelay_audio_io::process::InputActivity::Inactive,
                ..TransferOutcome::complete(0)
            };
        }
        if self.align_inputs && binding.is_routed_source() {
            if let Some(time) = self.capture_time {
                if let Some(outcome) = self.devices.read_aligned(
                    binding.node_id().as_str(),
                    destination,
                    (self.host, time, cycle.sample_rate),
                ) {
                    return outcome;
                }
                let token = binding_token(binding);
                let sources = binding_sources(binding, self.sources);
                let available = if let Some(sources) = sources {
                    sources.read_audio_time(
                        token,
                        destination,
                        cycle.frames,
                        mixrelay_macos_transport::source_registry::content_latency::ReadTiming {
                            rate: cycle.sample_rate,
                            first_nanos: Some(time),
                            host_ticks: Some(self.host),
                        },
                    )
                } else {
                    destination.fill(0.0);
                    0
                };
                return TransferOutcome {
                    source_sample_faults: sources
                        .map_or(0, |sources| sources.take_source_faults(token)),
                    ..TransferOutcome::complete(available)
                };
            }
            destination.fill(0.0);
            return TransferOutcome {
                activity: mixrelay_audio_io::process::InputActivity::Priming,
                ..TransferOutcome::complete(0)
            };
        }
        if let Some(outcome) = self
            .devices
            .read(binding.node_id().as_str(), destination, self.host)
        {
            return outcome;
        }
        read_application(binding, destination, cycle, self.sources, Some(self.host))
    }

    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        self.devices
            .write(binding.node_id().as_str(), source, self.host)
            .unwrap_or_else(|| TransferOutcome::complete(0))
    }
}

pub(super) fn application_epoch(
    binding: &ExternalBinding,
    sources: Option<&mixrelay_macos_transport::source_registry::SourceRegistry>,
) -> Option<u64> {
    (binding.kind() == mixrelay_audio_graph::NodeKind::ApplicationCapture
        || binding.virtual_endpoint_id().is_some())
    .then(|| {
        binding_sources(binding, sources)
            .and_then(|sources| sources.clock(binding_token(binding)))
            .filter(|clock| clock.running != 0)
            .map_or(0, |clock| clock.epoch)
    })
}

pub(super) fn read_application(
    binding: &ExternalBinding,
    destination: &mut [f32],
    cycle: &ProcessCycle,
    sources: Option<&mixrelay_macos_transport::source_registry::SourceRegistry>,
    host: Option<u64>,
) -> TransferOutcome {
    read_bound_source(
        binding,
        destination,
        cycle,
        binding_sources(binding, sources),
        host,
    )
}

fn read_bound_source(
    binding: &ExternalBinding,
    destination: &mut [f32],
    cycle: &ProcessCycle,
    sources: Option<&mixrelay_macos_transport::source_registry::SourceRegistry>,
    host: Option<u64>,
) -> TransferOutcome {
    if virtual_source_idle(binding, sources, (cycle.sample_rate, cycle.frames)) {
        destination.fill(0.0);
        return TransferOutcome {
            activity: mixrelay_audio_io::process::InputActivity::Inactive,
            ..TransferOutcome::complete(0)
        };
    }
    let token = binding_token(binding);
    let available = if let Some(sources) = sources {
        sources.read_audio_time(
            token,
            destination,
            cycle.frames,
            mixrelay_macos_transport::source_registry::content_latency::ReadTiming {
                rate: cycle.sample_rate,
                first_nanos: None,
                host_ticks: host,
            },
        )
    } else {
        destination.fill(0.0);
        0
    };
    TransferOutcome {
        source_sample_faults: sources.map_or(0, |sources| sources.take_source_faults(token)),
        ..TransferOutcome::complete(available)
    }
}

fn virtual_source_idle(
    binding: &ExternalBinding,
    sources: Option<&mixrelay_macos_transport::source_registry::SourceRegistry>,
    (rate, frames): (u32, u32),
) -> bool {
    if binding.virtual_endpoint_id().is_none() {
        return false;
    }
    let Some(sources) = sources else {
        return true;
    };
    let token = binding_token(binding);
    // Unrouted sources also negotiate their graph format before the first PCM,
    // so their meters cannot deadlock behind an unacknowledged running clock.
    let _ = sources.source_time(token, rate, frames);
    sources.clock(token).is_none_or(|clock| clock.running == 0)
}

pub(super) fn source_token(source_id: &str) -> u64 {
    hash_source(source_id.bytes())
}

fn hash_source(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    if hash == 0 { 1 } else { hash }
}

pub(super) fn binding_token(binding: &ExternalBinding) -> u64 {
    binding.virtual_endpoint_id().map_or_else(
        || source_token(binding.node_id().as_str()),
        |id| hash_source("virtual:".bytes().chain(id.as_str().bytes())),
    )
}

pub(super) fn binding_sources<'a>(
    binding: &ExternalBinding,
    applications: Option<&'a mixrelay_macos_transport::source_registry::SourceRegistry>,
) -> Option<&'a mixrelay_macos_transport::source_registry::SourceRegistry> {
    if binding.virtual_endpoint_id().is_some() {
        endpoint_service::virtual_source_registry()
    } else {
        applications
    }
}
