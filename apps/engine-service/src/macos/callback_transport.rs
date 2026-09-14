//! Hardware owns the graph callback; HAL owns capture coordinates. Each state
//! distinguishes absence, priming, actual delivery and control-plane recovery.
use super::native_transport::{EndpointSnapshot, SnapshotError};
use super::{NativeAudioConfig, NativeClockSnapshot};
use mixrelay_audio_io::{
    clock_bridge::{
        AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
    },
    process::{InputActivity, ProcessReport, TransferOutcome},
};

const MAX_CALLBACK: usize = 4096;

pub(super) trait CaptureTransport {
    fn begin_callback(&mut self) {}
    fn capture_observation_pending(&mut self) -> bool {
        false
    }
    fn clock(&mut self) -> Option<NativeClockSnapshot>;
    fn write(&mut self, frame: u64, samples: &[f32], frames: u32) -> bool;
    fn invalidate_capture(&mut self) -> bool;
    fn capture_rearm_pending(&mut self) -> bool {
        false
    }
}
/// Control only, after the sole writer descriptor has been removed and drained.
pub(super) fn recover_capture_epoch(id: [u8; 16]) -> bool {
    mixrelay_engine_service::endpoint_service::recover_capture_epoch(id).unwrap_or(false)
}
pub(super) struct NativeCapture {
    endpoint_id: [u8; 16],
    capture: std::cell::OnceCell<Result<EndpointSnapshot, SnapshotError>>,
    render: std::cell::OnceCell<Result<EndpointSnapshot, SnapshotError>>,
}
impl NativeCapture {
    /// One endpoint owns both observations; callback preparation clears only
    /// copied metadata, never an allocation or a shared mapping owner.
    pub(super) fn new(endpoint_id: [u8; 16]) -> Self {
        Self {
            endpoint_id,
            capture: std::cell::OnceCell::new(),
            render: std::cell::OnceCell::new(),
        }
    }

    pub(super) fn begin_callback(&mut self) {
        self.capture.take();
        self.render.take();
    }

    fn observed_capture(&self) -> Option<&super::native_transport::EndpointSnapshot> {
        self.capture_observation().as_ref().ok()
    }
    fn capture_observation(&self) -> &Result<EndpointSnapshot, SnapshotError> {
        self.capture
            .get_or_init(|| super::native_transport::snapshot(self.endpoint_id))
    }
    pub(super) fn render_observation(&self) -> &Result<EndpointSnapshot, SnapshotError> {
        self.render
            .get_or_init(|| super::native_transport::snapshot(self.endpoint_id))
    }
    pub(super) fn observed_render(&self) -> Option<&super::native_transport::EndpointSnapshot> {
        self.render_observation().as_ref().ok()
    }
}
impl CaptureTransport for NativeCapture {
    fn begin_callback(&mut self) {
        NativeCapture::begin_callback(self);
    }
    fn capture_rearm_pending(&mut self) -> bool {
        super::native_transport::needs_capture_recovery(self.capture_observation().as_ref())
    }
    fn capture_observation_pending(&mut self) -> bool {
        matches!(self.capture_observation(), Err(SnapshotError::Pending))
    }
    fn invalidate_capture(&mut self) -> bool {
        let refreshed = self
            .observed_capture()
            .and_then(super::native_transport::rearm);
        let Some(refreshed) = refreshed else {
            return false;
        };
        // observed_capture initialized this callback-owned cell; no concurrent
        // caller can mutate it while this &mut transport is being used.
        *self
            .capture
            .get_mut()
            .expect("capture observation initialized") = Ok(refreshed);
        true
    }
    fn clock(&mut self) -> Option<NativeClockSnapshot> {
        self.observed_capture().map(super::native_transport::clock)
    }
    fn write(&mut self, frame: u64, samples: &[f32], frames: u32) -> bool {
        self.observed_capture().is_some_and(|snapshot| {
            super::native_transport::write(snapshot, frame, samples, frames)
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestinationState {
    Unknown,
    Idle,
    Dormant,
    Priming,
    Running,
    AwaitingRate,
    AwaitingEpoch,
}
impl DestinationState {
    fn active(self) -> bool {
        matches!(self, Self::Priming | Self::Running)
    }
}
pub(super) struct HalCapture {
    bridge: AsyncClockBridge,
    output: Vec<f32>,
    channels: usize,
    rate: u32,
    graph_rate: u32,
    capacity: usize,
    epoch: Option<u64>,
    incarnation: Option<[u8; 16]>,
    consumed: u64,
    cursor: Option<u64>,
    lead: usize,
    filter_delay: usize,
    consumer_burst: usize,
    frames_without_demand: u64,
    state: DestinationState,
    needs_invalidate: bool,
    pending_report: ProcessReport,
}
impl HalCapture {
    pub(super) fn new(
        graph_rate: u32,
        hal_rate: u32,
        config: NativeAudioConfig,
    ) -> Result<Self, String> {
        let channels = config.capture_channels as usize;
        let capacity = config.capacity_frames as usize;
        if capacity < MAX_CALLBACK * 2 {
            return Err("HAL capture capacity is too small for callback SRC".into());
        }
        let mut bridge = AsyncClockBridge::new(BridgeConfig {
            channels,
            input_rate: graph_rate,
            output_rate: hal_rate,
            max_input_frames: MAX_CALLBACK,
            max_output_frames: MAX_CALLBACK,
            capacity_frames: capacity.min(262_144),
            priming_frames: 128,
            clock_relation: ClockRelation::Independent,
            feedback_queue: FeedbackQueue::Output,
            max_correction_ppm: 1000.0,
        })
        .map_err(|error| error.to_string())?;
        let mut output = vec![0.0; MAX_CALLBACK * channels];
        let filter_delay = bridge
            .pull_interleaved(&mut output)
            .map_err(|error| error.to_string())?
            .algorithmic_delay_frames;
        Ok(Self {
            bridge,
            output,
            channels,
            rate: hal_rate,
            graph_rate,
            capacity,
            epoch: None,
            incarnation: None,
            consumed: 0,
            cursor: None,
            lead: 128,
            filter_delay,
            consumer_burst: 0,
            frames_without_demand: 0,
            state: DestinationState::Unknown,
            needs_invalidate: true,
            pending_report: ProcessReport::default(),
        })
    }
    fn reset(&mut self, epoch: u64, consumed: u64) {
        self.needs_invalidate |= self.cursor.is_some() || self.bridge.buffered_input_frames() != 0;
        self.bridge.reset(epoch, Discontinuity::StreamRestart);
        self.epoch = Some(epoch);
        self.consumed = consumed;
        self.cursor = None;
        self.consumer_burst = 0;
        self.frames_without_demand = 0;
    }
    pub(super) fn source_discontinuity(&mut self) {
        self.reset(self.epoch.unwrap_or(0), self.consumed);
        if self.state.active() {
            self.state = DestinationState::Priming;
        }
        self.pending_report.discontinuities += 1;
    }

    /// The sole writer invalidates already published capture on the OFF edge.
    /// No converter is driven while dormant; ON resumes from a fresh cursor.
    pub(super) fn suspend(&mut self, transport: &mut impl CaptureTransport) {
        self.reset(self.epoch.unwrap_or(0), self.consumed);
        self.needs_invalidate = !transport.invalidate_capture();
        self.state = if self.needs_invalidate {
            DestinationState::AwaitingEpoch
        } else {
            DestinationState::Dormant
        };
    }
    /// One observation per hardware callback. Neither sleeps nor split graph
    /// blocks manufacture consumer progress or turn signal amplitude into a clock.
    pub(super) fn begin(&mut self, transport: &mut impl CaptureTransport, native_frames: u32) {
        if transport.capture_observation_pending() {
            // Contention publishes no new clock evidence. Retain filter/queued
            // PCM; this callback may push, but cannot publish using stale demand.
            return;
        }
        if transport.capture_rearm_pending() {
            if self.state != DestinationState::AwaitingEpoch {
                self.reset(self.epoch.unwrap_or(0), self.consumed);
            }
            self.needs_invalidate = true;
            self.state = DestinationState::AwaitingEpoch;
            return;
        }
        let Some(clock) = transport.clock() else {
            self.reset(self.epoch.unwrap_or(0), self.consumed);
            self.epoch = None;
            self.state = DestinationState::Unknown;
            return;
        };
        if self.state == DestinationState::AwaitingEpoch {
            return;
        }
        if clock.sample_rate != 0 && clock.sample_rate != self.rate {
            self.reset(clock.epoch, clock.capture_through);
            self.state = DestinationState::AwaitingRate;
            return;
        }
        if clock.running == 0 || clock.capture_through == 0 {
            self.reset(clock.epoch, clock.capture_through);
            self.state = DestinationState::Idle;
            return;
        }
        if self.incarnation != Some(clock.incarnation)
            || self.epoch != Some(clock.epoch)
            || clock.capture_through < self.consumed
        {
            self.reset(clock.epoch, clock.capture_through);
            self.incarnation = Some(clock.incarnation);
        }
        let elapsed = clock.capture_through - self.consumed;
        let equivalent =
            u64::from(native_frames) * u64::from(self.rate) / u64::from(self.graph_rate);
        if self.no_consumer_progress(clock, elapsed, equivalent) {
            return;
        }
        self.state = if self.cursor.is_some() {
            DestinationState::Running
        } else {
            DestinationState::Priming
        };
        self.observe_queue(
            clock,
            elapsed,
            usize::try_from(equivalent).unwrap_or(self.capacity),
        );
        self.consumed = clock.capture_through;
    }
    fn no_consumer_progress(
        &mut self,
        clock: NativeClockSnapshot,
        elapsed: u64,
        equivalent: u64,
    ) -> bool {
        if elapsed != 0 {
            self.frames_without_demand = 0;
            return false;
        }
        self.frames_without_demand = self.frames_without_demand.saturating_add(equivalent);
        // running is device-wide: a playback client may outlive recording.
        // Demand burst is observed/coalesced credit, not a claimed native size.
        let grace = (self.consumer_burst * 8)
            .max(self.rate as usize / 10)
            .min(self.capacity / 2);
        if self.state == DestinationState::Dormant || self.frames_without_demand > grace as u64 {
            if self.state != DestinationState::Dormant {
                self.reset(clock.epoch, clock.capture_through);
            }
            self.state = DestinationState::Dormant;
            return true;
        }
        false
    }
    fn observe_queue(&mut self, clock: NativeClockSnapshot, elapsed: u64, equivalent: usize) {
        let moved = usize::try_from(elapsed).unwrap_or(usize::MAX);
        self.consumer_burst = self.consumer_burst.max(moved.min(self.capacity / 8));
        self.lead = (self.consumer_burst.max(equivalent) * 2 + self.filter_delay)
            .clamp(128, self.capacity / 2);
        let queued = self
            .cursor
            .unwrap_or(clock.capture_through)
            .saturating_sub(clock.capture_through);
        let pending = self.bridge.buffered_input_frames() as u64 * u64::from(self.rate)
            / u64::from(self.graph_rate);
        let invalid = elapsed > u64::from(self.rate)
            || self
                .cursor
                .is_some_and(|cursor| cursor < clock.capture_through);
        let feedback = ClockFeedback {
            queue: FeedbackQueue::Output,
            epoch: clock.epoch,
            queued_frames: usize::try_from(queued.saturating_add(pending)).unwrap_or(usize::MAX),
            target_frames: self.lead,
            elapsed_frames: moved,
            clock_running: self.cursor.is_some(),
        };
        if invalid || (elapsed != 0 && self.bridge.observe_clock(feedback).is_err()) {
            self.pending_report.discontinuities += u64::from(self.cursor.is_some());
            self.reset(clock.epoch, clock.capture_through);
            self.state = DestinationState::Priming;
        }
    }
    pub(super) fn push(&mut self, samples: &[f32], frames: u32) -> TransferOutcome {
        if !self.state.active() {
            return TransferOutcome {
                frames,
                discontinuity: false,
                activity: if matches!(
                    self.state,
                    DestinationState::Idle | DestinationState::Dormant
                ) {
                    InputActivity::Inactive
                } else {
                    InputActivity::Unknown
                },
            };
        }
        if samples.len() != frames as usize * self.channels
            || self.bridge.push_interleaved(samples).is_err()
        {
            self.reset(self.epoch.unwrap_or(0), self.consumed);
            self.state = DestinationState::Priming;
            return TransferOutcome {
                frames: 0,
                discontinuity: true,
                activity: InputActivity::Active,
            };
        }
        TransferOutcome::complete(frames)
    }
    pub(super) fn flush(&mut self, transport: &mut impl CaptureTransport) -> bool {
        if transport.capture_observation_pending() {
            return true;
        }
        if self.needs_invalidate {
            if !transport.invalidate_capture() {
                self.state = DestinationState::AwaitingEpoch;
                return true;
            }
            self.needs_invalidate = false;
            if self.state == DestinationState::AwaitingEpoch {
                self.state = DestinationState::Unknown;
            }
        }
        if !self.state.active() {
            return true;
        }
        // Fixed maximum work. Only the valid converted prefix is published;
        // neither priming padding nor hardware sample indices enter HAL storage.
        for _ in 0..8 {
            let frame = self.cursor.unwrap_or(self.consumed + self.lead as u64);
            let queued =
                usize::try_from(frame.saturating_sub(self.consumed)).unwrap_or(self.capacity);
            let room = self.capacity.saturating_sub(queued).min(MAX_CALLBACK);
            if room == 0 {
                break;
            }
            let Ok(report) = self
                .bridge
                .pull_interleaved(&mut self.output[..room * self.channels])
            else {
                return false;
            };
            if report.transferred_frames == 0 {
                break;
            }
            let frames =
                u32::try_from(report.transferred_frames).expect("bounded converted output");
            if !transport.write(
                frame,
                &self.output[..report.transferred_frames * self.channels],
                frames,
            ) {
                // HAL can request rearm AFTER our initial check. A rejected
                // publication defers this boundary, never kills hardware DSP.
                self.needs_invalidate = true;
                self.state = DestinationState::AwaitingEpoch;
                self.pending_report.output_overflows += 1;
                self.pending_report.discontinuities += 1;
                self.reset(self.epoch.unwrap_or(0), self.consumed);
                return true;
            }
            self.cursor = Some(frame + u64::from(frames));
            self.state = DestinationState::Running;
        }
        true
    }
    pub(super) fn synchronized(&self) -> bool {
        matches!(
            self.state,
            DestinationState::Idle
                | DestinationState::Dormant
                | DestinationState::Priming
                | DestinationState::Running
        ) && self.pending_report.discontinuities == 0
    }
    pub(super) fn reconfigure(&self) -> bool {
        matches!(
            self.state,
            DestinationState::AwaitingEpoch | DestinationState::AwaitingRate
        )
    }
    pub(super) fn take_report(&mut self) -> ProcessReport {
        std::mem::take(&mut self.pending_report)
    }
}
#[cfg(test)]
#[path = "callback_transport_tests.rs"]
mod tests;
