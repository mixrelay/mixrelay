use super::{
    MacEngineExternalIo, NativeAudioConfig, WorkerCycle, block_sample_count,
    callback_transport::NativeCapture,
    capture_schedule::CaptureSchedule,
    endpoint_boundaries::{Directions, EndpointBoundaries},
    native_transport::{self, EndpointSnapshot},
    process_native_cycle,
};
use mixrelay_audio_engine::{BlockProcessor, RenderPlan};
use mixrelay_audio_io::VirtualEndpointId;
use mixrelay_audio_io::process::{InputActivity, ProcessReport, TransferOutcome};

/// Synchronous PCM boundary. The worker owns the only mutable transfer handle;
/// implementations must not retain borrowed buffers after a call returns.
pub(super) trait Transport {
    fn prepare_capture(&mut self, endpoint: &EndpointSnapshot) -> Option<EndpointSnapshot>;
    fn invalidate_capture(&mut self, endpoint: &EndpointSnapshot) -> bool;
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
    fn invalidate_capture(&mut self, endpoint: &EndpointSnapshot) -> bool {
        native_transport::rearm(endpoint).is_some()
    }
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

/// The primary endpoint uses the actual pacing coordinates without an SRC.
/// Its transfers are prepared only after the processor samples this block's gates.
pub(super) struct PrimaryIo<'a, T> {
    transport: &'a mut T,
    endpoint: Option<&'a EndpointSnapshot>,
    render: &'a mut [f32],
    capture: &'a mut [f32],
    transfer: TransferOutcome,
    sink_enabled: &'a mut bool,
    schedule: &'a mut CaptureSchedule,
    primary: Option<&'a PrimaryEndpoint>,
    write_hal: bool,
    hal_only: bool,
    reservation: &'a mut Option<(EndpointSnapshot, u64)>,
    rejected: &'a mut bool,
}

impl<T: Transport> PrimaryIo<'_, T> {
    pub(super) fn begin(
        &mut self,
        cycle: &mixrelay_audio_io::process::ProcessCycle,
        active: Directions,
    ) -> bool {
        if *self.sink_enabled && !active.sink {
            if let Some(endpoint) = self.endpoint {
                self.transport.invalidate_capture(endpoint);
            }
            self.schedule.reset();
        }
        *self.sink_enabled = active.sink;
        if !self.admit(cycle, active.sink) {
            return false;
        }
        if active.source {
            self.transfer = read_render(
                self.transport,
                self.endpoint,
                self.render,
                cycle.first_frame,
                cycle.frames,
            );
        }
        true
    }

    fn admit(&mut self, cycle: &mixrelay_audio_io::process::ProcessCycle, sink: bool) -> bool {
        let Some((endpoint, primary)) = self.endpoint.zip(self.primary) else {
            return true;
        };
        if !sink || !self.write_hal || endpoint.endpoint_id != primary.id {
            return true;
        }
        let prepared = self.transport.prepare_capture(endpoint);
        // An exhausted reservation is ordinary consumer backpressure. In
        // particular, closing an OS capture client leaves its last demand stamp
        // unchanged while playback may keep running. Only failed preparation is
        // a transport error; never stop healthy source DSP for absent new demand.
        *self.rejected = prepared.is_none();
        *self.reservation = prepared.and_then(|snapshot| {
            self.schedule
                .reserve(&snapshot, cycle.frames, primary.config.capacity_frames)
                .map(|frame| (snapshot, frame))
        });
        !*self.rejected || !self.hal_only
    }

    pub(super) fn read(&self, destination: &mut [f32]) -> TransferOutcome {
        destination.copy_from_slice(self.render);
        self.transfer
    }

    pub(super) fn write(&mut self, source: &[f32], frames: u32) -> TransferOutcome {
        for (destination, sample) in self.capture.iter_mut().zip(source) {
            *destination += sample;
        }
        TransferOutcome::complete(frames)
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
    primary_sink_enabled: bool,
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
            primary_sink_enabled: true,
        }
    }

    /// Admission precedes PCM consumption. An unavailable HAL-only destination
    /// does not advance DSP; mixed routes still service their healthy branches.
    pub(super) fn process(
        &mut self,
        processor: &mut BlockProcessor,
        cycle: WorkerCycle<'_>,
    ) -> Result<bool, String> {
        let rate = processor.plan().format().sample_rate;
        let mut capture = None;
        let mut rejected = false;
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
        pcm.fill(0.0);
        if cycle.discontinuity {
            self.independent.discontinuity();
        }
        self.independent.begin(frames);
        let hal_only = processor
            .plan()
            .source_bindings()
            .iter()
            .chain(processor.plan().sink_bindings())
            .all(|binding| {
                binding.virtual_endpoint_id()
                    == self.primary.as_ref().map(|endpoint| &endpoint.identity)
            });
        let report = process_native_cycle(
            processor,
            &mut MacEngineExternalIo {
                devices: cycle.devices,
                primary_io: PrimaryIo {
                    transport: &mut self.transport,
                    endpoint,
                    render,
                    capture: pcm,
                    transfer: TransferOutcome::complete(0),
                    sink_enabled: &mut self.primary_sink_enabled,
                    schedule: &mut self.schedule,
                    primary: self.primary.as_ref(),
                    write_hal: cycle.write_hal,
                    hal_only,
                    reservation: &mut capture,
                    rejected: &mut rejected,
                },
                primary: self.primary.as_ref().map(|endpoint| &endpoint.identity),
                independent: &mut self.independent,
            },
            cycle,
        )?;
        if rejected {
            record_rejection(&cycle);
        }
        let Some(report) = report else {
            return Ok(false);
        };
        cycle.status.record_process(report);
        cycle.status.record_process(self.independent.finish_block());
        if !self.independent.synchronized() {
            let authority = if cycle.timeline.clock_id == 0 {
                mixrelay_audio_io::session::ClockAuthority::EngineTimer
            } else {
                mixrelay_audio_io::session::ClockAuthority::Device
            };
            cycle.status.observe_clock(authority, false);
        }
        if let Some((endpoint, frame)) = capture
            && self.primary_sink_enabled
            && !self.transport.write(&endpoint, frame, pcm, frames)
        {
            self.schedule.reset();
            record_rejection(&cycle);
        }
        Ok(true)
    }

    pub(super) fn snapshot(&self) -> Result<EndpointSnapshot, native_transport::SnapshotError> {
        let primary = self
            .primary
            .as_ref()
            .ok_or(native_transport::SnapshotError::Missing)?;
        native_transport::snapshot(primary.id)
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
            .is_some_and(|primary| primary.directions.sink && self.primary_sink_enabled)
            && native_transport::needs_capture_recovery(self.snapshot().as_ref())
        {
            return true;
        }
        let running = |snapshot: &EndpointSnapshot| {
            snapshot.clock.running != 0 && snapshot.clock.sample_rate == rate
        };
        let primary = self.snapshot();
        if matches!(primary, Err(native_transport::SnapshotError::Pending))
            || primary.as_ref().is_ok_and(running)
        {
            return false;
        }
        self.independent.endpoints.iter().any(|endpoint| {
            native_transport::snapshot(endpoint.id)
                .as_ref()
                .is_ok_and(running)
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
            self.primary_sink_enabled = true;
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
            && native_transport::needs_capture_recovery(self.snapshot().as_ref())
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
            native_transport::snapshot(endpoint.id).is_ok_and(|snapshot| {
                snapshot.clock.running != 0
                    && snapshot.clock.sample_rate == plan.format().sample_rate
            })
        };
        let index = independent
            .endpoints
            .iter()
            .position(|endpoint| {
                Some(endpoint.id) == preferred
                    && (running(endpoint)
                        || matches!(
                            native_transport::snapshot(endpoint.id),
                            Err(native_transport::SnapshotError::Pending)
                        ))
            })
            .or_else(|| independent.endpoints.iter().position(running));
        let primary = index
            .map(|index| {
                let endpoint = independent.endpoints.remove(index);
                let config = mixrelay_engine_service::endpoint_service::audio_config(endpoint.id)
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
        .observe_clock(mixrelay_audio_io::session::ClockAuthority::Device, false);
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
