//! Selects one native pacing owner and observes route clock readiness.
use super::super::external_io::{binding_sources, binding_token};
use crate::macos_pacing::{ApplicationClockSnapshot, ApplicationPacer, HardwarePacer, Pace};
use mixrelay_audio_engine::{BlockProcessor, ExternalBinding, RenderPlan};
use mixrelay_engine_service::endpoint_service;

pub(in crate::macos) struct RouteClock {
    pub(in crate::macos) maximum_frames: u32,
    pub(in crate::macos) synchronized: bool,
    pub(in crate::macos) independent_idle: bool,
}
impl RouteClock {
    pub(in crate::macos) fn observe(
        processor: &BlockProcessor,
        devices: &crate::macos_devices::DeviceSet,
    ) -> Self {
        let mut result = Self {
            maximum_frames: processor.plan().format().block_frames,
            synchronized: true,
            independent_idle: true,
        };
        for (input, binding) in [
            (true, processor.plan().source_bindings()),
            (false, processor.plan().sink_bindings()),
        ]
        .into_iter()
        .flat_map(|(input, bindings)| bindings.iter().map(move |binding| (input, binding)))
        {
            let id = binding.node_id().as_str();
            if devices.is_binding_inactive(id, input) {
                continue;
            }
            if let Some(ready) = devices.clock_synchronized(id, input) {
                result.synchronized &= ready;
                result.independent_idle = false;
            } else if let Some(clock) = binding_clock(binding) {
                result.synchronized &= clock.synchronized != 0
                    && clock.graph_rate == processor.plan().format().sample_rate;
                result.independent_idle &= clock.running == 0;
            } else {
                result.synchronized = false;
                result.independent_idle = false;
            }
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Application,
    Virtual,
}

#[derive(Default)]
pub(in crate::macos) struct NativePacers {
    hardware: HardwarePacer,
    application: ApplicationPacer,
    selected_application: Option<(u64, SourceKind)>,
}
impl NativePacers {
    fn select_application_master(&mut self, token: Option<(u64, SourceKind)>) {
        if token.is_none() {
            self.application = ApplicationPacer::default();
        }
        if token != self.selected_application
            && let Some(previous) = self.selected_application
        {
            set_source_master(previous, false);
        }
        self.selected_application = token;
        if let Some(token) = token {
            set_source_master(token, true);
        }
    }
}
impl Drop for NativePacers {
    fn drop(&mut self) {
        self.select_application_master(None);
    }
}

fn set_source_master((token, kind): (u64, SourceKind), master: bool) {
    let sources = match kind {
        SourceKind::Virtual => endpoint_service::virtual_source_registry(),
        SourceKind::Application => endpoint_service::source_registry(),
    };
    if let Some(sources) = sources {
        sources.set_clock_master(token, master);
    }
}

fn binding_clock(binding: &ExternalBinding) -> Option<ApplicationClockSnapshot> {
    let progress = binding_sources(binding, endpoint_service::source_registry())?
        .pcm_progress(binding_token(binding))?;
    let clock = progress.clock;
    Some(ApplicationClockSnapshot {
        epoch: clock.epoch,
        published_frames: progress.published_frames,
        native_rate: clock.native_rate,
        graph_rate: clock.graph_rate,
        running: clock.running,
        synchronized: clock.synchronized,
        capacity_frames: progress.capacity_frames,
    })
}

fn application_identity(binding: &ExternalBinding) -> Option<(u64, SourceKind)> {
    let kind = if binding.virtual_endpoint_id().is_some() {
        SourceKind::Virtual
    } else if binding.kind() == mixrelay_audio_graph::NodeKind::ApplicationCapture {
        SourceKind::Application
    } else {
        return None;
    };
    Some((binding_token(binding), kind))
}

pub(in crate::macos) fn next_native_pace(
    pacers: &mut NativePacers,
    devices: &crate::macos_devices::DeviceSet,
    plan: &RenderPlan,
    capacity: u32,
) -> (Pace, u64) {
    // Device-owned frame credit remains authoritative across Driver and physical IOProcs.
    if let Some(hardware) = devices.hardware_clock_progress() {
        let format = plan.format();
        devices.set_clock_master(Some(hardware.owner_id));
        devices.set_graph_quantum(
            u32::try_from(
                (u64::from(hardware.quantum) * u64::from(format.sample_rate))
                    .div_ceil(u64::from(hardware.native_rate)),
            )
            .unwrap_or(u32::MAX)
            .max(format.block_frames),
        );
        pacers.select_application_master(None);
        return (
            pacers.hardware.next(
                (hardware.epoch, hardware.native_frames, hardware.native_rate),
                format.sample_rate,
                format.block_frames,
                capacity,
            ),
            2,
        );
    }
    let format = plan.format();
    devices.set_clock_master(None);
    devices.set_graph_quantum(format.block_frames);
    for binding in plan.source_bindings() {
        if let Some(clock) = binding_clock(binding)
            && clock.running != 0
        {
            pacers.select_application_master(Some((
                binding_token(binding),
                if binding.virtual_endpoint_id().is_some() {
                    SourceKind::Virtual
                } else {
                    SourceKind::Application
                },
            )));
            return (
                pacers.application.next(
                    (binding_token(binding), clock),
                    (
                        format.sample_rate,
                        format.block_frames,
                        clock.capacity_frames,
                    ),
                ),
                3,
            );
        }
    }
    // Master selection follows graph identity. A producer format rearm can
    // temporarily hide native running state without removing the selected source.
    let retained = pacers.selected_application.filter(|(token, kind)| {
        plan.source_bindings()
            .iter()
            .any(|binding| application_identity(binding) == Some((*token, *kind)))
    });
    let waiting = retained.or_else(|| plan.source_bindings().iter().find_map(application_identity));
    if let Some((token, _)) = waiting {
        pacers.application.await_source(token);
    }
    pacers.select_application_master(waiting);
    (Pace::Unknown, 0)
}
