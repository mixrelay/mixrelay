use super::{
    MacEngineExternalIo, NativeAudioConfig, WorkerCycle, block_sample_count,
    callback_transport::NativeCapture,
    capture_schedule::CaptureSchedule,
    endpoint_boundaries::{Directions, EndpointBoundaries},
    native_transport::{self, EndpointSnapshot},
    process_native_cycle,
};
use tonenari_audio_engine::{BlockProcessor, RenderPlan};
use tonenari_audio_io::VirtualEndpointId;
use tonenari_audio_io::process::{InputActivity, ProcessReport, TransferOutcome};

/// Synchronous PCM boundary. The worker owns the only mutable transfer handle;
/// implementations must not retain borrowed buffers after a call returns.
pub(super) trait Transport {
    fn prepare_capture(&mut self, endpoint: &EndpointSnapshot) -> Option<EndpointSnapshot>;
    fn read(
        &mut self,
        endpoint: &EndpointSnapshot,
        frame: u64,
        pcm: &mut [f32],
        frames: u32,
    ) -> u32;
    fn write(&mut self, endpoint: &EndpointSnapshot, frame: u64, pcm: &[f32], frames: u32) -> bool;
}

pub(super) struct NativeTransport;
impl Transport for NativeTransport {
    fn prepare_capture(&mut self, endpoint: &EndpointSnapshot) -> Option<EndpointSnapshot> {
        if native_transport::pending(endpoint) {
            native_transport::rearm(endpoint)
        } else {
            Some(*endpoint)
        }
    }
    fn read(
        &mut self,
        endpoint: &EndpointSnapshot,
        frame: u64,
        pcm: &mut [f32],
        frames: u32,
    ) -> u32 {
        native_transport::read(endpoint, frame, pcm, frames)
    }
    fn write(&mut self, endpoint: &EndpointSnapshot, frame: u64, pcm: &[f32], frames: u32) -> bool {
        native_transport::write(endpoint, frame, pcm, frames)
    }
}

/// Scratch PCM and the destination cursor have the same lifetime as their
/// transfer owner. Allocation occurs before the worker starts processing.
pub(super) struct WorkerIo<T> {
    transport: T,
    render: Vec<f32>,
    capture: Vec<f32>,
    schedule: CaptureSchedule,
    primary: Option<PrimaryEndpoint>,
    independent: EndpointBoundaries<NativeCapture>,
}
pub(super) struct PrimaryEndpoint {
    pub(super) identity: VirtualEndpointId,
    pub(super) id: [u8; 16],
    pub(super) config: NativeAudioConfig,
    pub(super) directions: Directions,
}
impl PrimaryEndpoint {
    fn same_configuration(&self, other: &Self) -> bool {
        self.id == other.id
            && self.directions == other.directions
            && self.config.capacity_frames == other.config.capacity_frames
            && self.config.render_channels == other.config.render_channels
            && self.config.capture_channels == other.config.capture_channels
    }

    fn channels(&self) -> (u32, u32) {
        (
            if self.directions.source {
                self.config.render_channels
            } else {
                0
            },
            if self.directions.sink {
                self.config.capture_channels
            } else {
                0
            },
        )
    }
}
enum Admission {
    Retry,
    Process(Option<(EndpointSnapshot, u64)>),
}

struct WorkerConfiguration {
    primary: Option<PrimaryEndpoint>,
    independent: EndpointBoundaries<NativeCapture>,
}

impl<T: Transport> WorkerIo<T> {
    pub(super) fn new(transport: T, primary: Option<PrimaryEndpoint>) -> Self {
        let (render, capture) = primary.as_ref().map_or((0, 0), |endpoint| {
            let (render, capture) = endpoint.channels();
            (render as usize * 4096, capture as usize * 4096)
        });
        Self {
            transport,
            render: vec![0.0; render],
            capture: vec![0.0; capture],
            schedule: CaptureSchedule::default(),
            primary,
            independent: EndpointBoundaries::empty(),
        }
    }

    fn reserve(&mut self, cycle: &WorkerCycle<'_>, rate: u32) -> Option<(EndpointSnapshot, u64)> {
        let endpoint = cycle.endpoint.as_ref()?;
        let primary = self.primary.as_ref()?;
        if !primary.directions.sink
            || !cycle.write_hal
            || endpoint.endpoint_id != primary.id
            || endpoint.clock.sample_rate != rate
        {
            return None;
        }
        let snapshot = self.transport.prepare_capture(endpoint)?;
        let frame =
            self.schedule
                .reserve(&snapshot, cycle.frames, primary.config.capacity_frames)?;
        Some((snapshot, frame))
    }

    /// Admission precedes PCM consumption. An unavailable HAL-only destination
    /// does not advance DSP; mixed routes still service their healthy branches.
    pub(super) fn process(
        &mut self,
        processor: &mut BlockProcessor,
        cycle: WorkerCycle<'_>,
    ) -> Result<bool, String> {
        let rate = processor.plan().format().sample_rate;
        let Admission::Process(capture) = self.admit(processor.plan(), &cycle) else {
            return Ok(false);
        };
        let frames = cycle.frames;
        let (render_channels, capture_channels) = self
            .primary
            .as_ref()
            .map_or((0, 0), PrimaryEndpoint::channels);
        let render = &mut self.render[..block_sample_count(render_channels, frames)?];
        let pcm = &mut self.capture[..block_sample_count(capture_channels, frames)?];
        let endpoint = cycle
            .endpoint
            .as_ref()
            .filter(|value| value.clock.sample_rate == rate);
        let transfer = read_render(
            &mut self.transport,
            endpoint,
            render,
            cycle.timeline.frame,
            frames,
        );
        pcm.fill(0.0);
        if cycle.discontinuity {
            self.independent.discontinuity();
        }
        self.independent.begin(frames);
        self.independent.prepare_block(frames);
        let report = process_native_cycle(
            processor,
            &mut MacEngineExternalIo {
                devices: cycle.devices,
                render_samples: render,
                render_transfer: transfer,
                capture_samples: pcm,
                primary: self.primary.as_ref().map(|endpoint| &endpoint.identity),
                independent: &mut self.independent,
            },
            cycle,
        )?;
        cycle.status.record_process(report);
        cycle.status.record_process(self.independent.finish_block());
        if !self.independent.synchronized() {
            let authority = if cycle.timeline.clock_id == 0 {
                tonenari_audio_io::session::ClockAuthority::EngineTimer
            } else {
                tonenari_audio_io::session::ClockAuthority::Device
            };
            cycle.status.observe_clock(authority, false);
        }
        if let Some((endpoint, frame)) = capture
            && !self.transport.write(&endpoint, frame, pcm, frames)
        {
            self.schedule.reset();
            record_rejection(&cycle);
        }
        Ok(true)
    }

    fn admit(&mut self, plan: &RenderPlan, cycle: &WorkerCycle<'_>) -> Admission {
        let rate = plan.format().sample_rate;
        let capture = self.reserve(cycle, rate);
        let demanded = self
            .primary
            .as_ref()
            .is_some_and(|endpoint| endpoint.directions.sink)
            && cycle.write_hal
            && cycle.endpoint.as_ref().is_some_and(|endpoint| {
                endpoint.clock.sample_rate == rate && endpoint.clock.capture_frames != 0
            });
        if demanded && capture.is_none() {
            record_rejection(cycle);
            if plan
                .source_bindings()
                .iter()
                .chain(plan.sink_bindings())
                .all(|binding| {
                    binding.virtual_endpoint_id()
                        == self.primary.as_ref().map(|endpoint| &endpoint.identity)
                })
            {
                return Admission::Retry;
            }
        }
        Admission::Process(capture)
    }

    pub(super) fn snapshot(&self) -> Option<EndpointSnapshot> {
        native_transport::snapshot(self.primary.as_ref()?.id)
    }

    pub(super) fn capacity(&self) -> u32 {
        self.primary
            .as_ref()
            .map_or(4096, |endpoint| endpoint.config.capacity_frames)
    }

    pub(super) fn pending(&self, rate: u32) -> bool {
        if self.independent.pending() {
            return true;
        }
        if self
            .primary
            .as_ref()
            .is_some_and(|primary| primary.directions.sink)
            && self
                .snapshot()
                .as_ref()
                .is_none_or(native_transport::pending)
        {
            return true;
        }
        let running = |snapshot: &EndpointSnapshot| {
            snapshot.clock.running != 0 && snapshot.clock.sample_rate == rate
        };
        if self.snapshot().as_ref().is_some_and(running) {
            return false;
        }
        self.independent.endpoints.iter().any(|endpoint| {
            native_transport::snapshot(endpoint.id)
                .as_ref()
                .is_some_and(running)
        })
    }

    /// Apply prepared ownership at a worker block boundary. Returning true
    /// means the primary timing contract changed, requiring fresh clock credit.
    fn configure(
        &mut self,
        configuration: WorkerConfiguration,
        rate_changed: bool,
        mut retire: impl FnMut([u8; 16]),
    ) -> bool {
        let same_primary = match (&self.primary, &configuration.primary) {
            (Some(current), Some(next)) => current.same_configuration(next),
            (None, None) => true,
            _ => false,
        } && !rate_changed;
        if !same_primary {
            if let Some(current) = &self.primary
                && current.directions.sink
            {
                retire(current.id);
            }
            self.primary = configuration.primary;
            let (render, capture) = self
                .primary
                .as_ref()
                .map_or((0, 0), PrimaryEndpoint::channels);
            self.render = vec![0.0; render as usize * 4096];
            self.capture = vec![0.0; capture as usize * 4096];
            self.schedule.reset();
        }
        self.independent
            .reconcile(configuration.independent, retire);
        !same_primary
    }
}

impl WorkerIo<NativeTransport> {
    pub(super) fn retire(&self) {
        self.independent.retire();
        if let Some(primary) = &self.primary
            && primary.directions.sink
        {
            super::callback_transport::recover_capture_epoch(primary.id);
        }
    }

    pub(super) fn prepare(plan: &RenderPlan) -> Result<Self, String> {
        let configuration = WorkerConfiguration::prepare(plan, None)?;
        let mut result = Self::new(NativeTransport, configuration.primary);
        result.independent = configuration.independent;
        Ok(result)
    }

    pub(super) fn reconcile(
        &mut self,
        plan: &RenderPlan,
        rate_changed: bool,
    ) -> Result<bool, String> {
        // The primary has no SRC owner to report terminal rearm. Recover only
        // its own unavailable/pending mapping, never another healthy writer.
        if let Some(primary) = &self.primary
            && primary.directions.sink
            && self
                .snapshot()
                .as_ref()
                .is_none_or(native_transport::pending)
        {
            super::callback_transport::recover_capture_epoch(primary.id);
        }
        let preferred = self.primary.as_ref().map(|endpoint| endpoint.id);
        let configuration = WorkerConfiguration::prepare(plan, preferred)?;
        Ok(self.configure(configuration, rate_changed, |id| {
            super::callback_transport::recover_capture_epoch(id);
        }))
    }
}

impl WorkerConfiguration {
    fn prepare(plan: &RenderPlan, preferred: Option<[u8; 16]>) -> Result<Self, String> {
        let mut independent = EndpointBoundaries::prepare(plan)?;
        let running = |endpoint: &super::endpoint_boundaries::EndpointBoundary<NativeCapture>| {
            native_transport::snapshot(endpoint.id).is_some_and(|snapshot| {
                snapshot.clock.running != 0
                    && snapshot.clock.sample_rate == plan.format().sample_rate
            })
        };
        let index = independent
            .endpoints
            .iter()
            .position(|endpoint| Some(endpoint.id) == preferred && running(endpoint))
            .or_else(|| independent.endpoints.iter().position(running));
        let primary = index
            .map(|index| {
                let endpoint = independent.endpoints.remove(index);
                let config = tonenari_engine_service::endpoint_service::audio_config(endpoint.id)
                    .ok_or_else(|| {
                    "primary virtual device disappeared during preparation".to_owned()
                })?;
                Ok::<_, String>(PrimaryEndpoint {
                    directions: endpoint.directions(),
                    identity: endpoint.identity,
                    id: endpoint.id,
                    config,
                })
            })
            .transpose()?;
        Ok(Self {
            primary,
            independent,
        })
    }
}

fn record_rejection(cycle: &WorkerCycle<'_>) {
    cycle
        .status
        .observe_clock(tonenari_audio_io::session::ClockAuthority::Device, false);
    cycle.status.record_process(ProcessReport {
        output_overflows: 1,
        discontinuities: 1,
        ..ProcessReport::default()
    });
}

fn read_render(
    transport: &mut impl Transport,
    endpoint: Option<&EndpointSnapshot>,
    destination: &mut [f32],
    first_frame: u64,
    frames: u32,
) -> TransferOutcome {
    destination.fill(0.0);
    if destination.is_empty() {
        return TransferOutcome {
            activity: InputActivity::Inactive,
            ..TransferOutcome::complete(0)
        };
    }
    let activity = match endpoint.map(|value| value.render_activity) {
        Some(1 | 2) => InputActivity::Inactive,
        Some(3) => InputActivity::Active,
        _ => InputActivity::Unknown,
    };
    let mut outcome = TransferOutcome {
        activity,
        ..TransferOutcome::complete(0)
    };
    if let Some(endpoint) = endpoint
        && first_frame
            .checked_add(u64::from(frames))
            .is_some_and(|end| endpoint.published_through >= end)
    {
        outcome.frames = transport.read(endpoint, first_frame, destination, frames);
        debug_assert!(outcome.frames <= frames);
    }
    outcome
}

#[cfg(test)]
mod tests;
