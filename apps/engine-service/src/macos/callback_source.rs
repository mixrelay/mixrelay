//! HAL render producer -> hardware-clock graph. This is the opposite direction
//! from capture publication, with its own absolute read cursor and SRC history.
use super::{NativeAudioConfig, NativeClockSnapshot};
use mixrelay_audio_io::{
    clock_bridge::{
        AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
    },
    process::{InputActivity, TransferOutcome},
};

const MAX_TRANSFER: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceReadiness {
    Priming { waited: u64 },
    Ready,
    Starved,
}

pub(super) struct RenderSnapshot {
    pub(super) clock: NativeClockSnapshot,
    pub(super) stream_epoch: u64,
    pub(super) through: u64,
    pub(super) activity: InputActivity,
    pub(super) channels: u32,
    pub(super) quantum: u32,
}
pub(super) trait RenderTransport {
    fn render_observation_pending(&mut self) -> bool {
        false
    }
    fn render_snapshot(&mut self) -> Option<RenderSnapshot> {
        None
    }
    fn read_render(&mut self, _frame: u64, _samples: &mut [f32], _frames: u32) -> u32 {
        0
    }
}
#[cfg(test)]
#[path = "callback_source_tests.rs"]
mod tests;
impl RenderTransport for super::callback_transport::NativeCapture {
    fn render_observation_pending(&mut self) -> bool {
        matches!(
            self.render_observation(),
            Err(super::native_transport::SnapshotError::Pending)
        )
    }
    fn render_snapshot(&mut self) -> Option<RenderSnapshot> {
        let snapshot = self.observed_render()?;
        Some(RenderSnapshot {
            clock: super::native_transport::clock(snapshot),
            stream_epoch: snapshot.render_epoch,
            through: snapshot.published_through,
            channels: snapshot.render_channels,
            quantum: snapshot.clock.render_frames,
            activity: match snapshot.render_activity {
                1 | 2 => InputActivity::Inactive,
                3 => InputActivity::Active,
                _ => InputActivity::Unknown,
            },
        })
    }
    fn read_render(&mut self, frame: u64, samples: &mut [f32], frames: u32) -> u32 {
        if let Some(snapshot) = self.observed_render() {
            super::native_transport::read(snapshot, frame, samples, frames)
        } else {
            samples.fill(0.0);
            0
        }
    }
}

pub(super) struct HalRender {
    bridge: AsyncClockBridge,
    input: Vec<f32>,
    channels: usize,
    rate: u32,
    graph_rate: u32,
    capacity: usize,
    epoch: Option<([u8; 16], u64, u64)>,
    cursor: u64,
    published: u64,
    burst: usize,
    target: usize,
    readiness: SourceReadiness,
    activity: InputActivity,
    reconfigure: bool,
    loss: bool,
}
impl HalRender {
    pub(super) fn new(
        graph_rate: u32,
        hal_rate: u32,
        config: NativeAudioConfig,
    ) -> Result<Self, String> {
        let channels = config.render_channels as usize;
        let capacity = (config.capacity_frames as usize).min(262_144);
        let bridge = AsyncClockBridge::new(BridgeConfig {
            channels,
            input_rate: hal_rate,
            output_rate: graph_rate,
            max_input_frames: MAX_TRANSFER,
            max_output_frames: MAX_TRANSFER,
            capacity_frames: capacity,
            priming_frames: 128,
            clock_relation: ClockRelation::Independent,
            feedback_queue: FeedbackQueue::Input,
            max_correction_ppm: 1000.0,
        })
        .map_err(|error| error.to_string())?;
        Ok(Self {
            bridge,
            input: vec![0.0; MAX_TRANSFER * channels],
            channels,
            rate: hal_rate,
            graph_rate,
            capacity,
            epoch: None,
            cursor: 0,
            published: 0,
            burst: 0,
            target: 128,
            readiness: SourceReadiness::Priming { waited: 0 },
            activity: InputActivity::Unknown,
            reconfigure: false,
            loss: false,
        })
    }
    fn reset(&mut self, epoch: ([u8; 16], u64, u64), through: u64) {
        self.bridge.reset(epoch.2, Discontinuity::StreamRestart);
        self.epoch = Some(epoch);
        self.cursor = through;
        self.published = through;
        self.burst = 0;
        self.readiness = if self.loss {
            SourceReadiness::Starved
        } else {
            SourceReadiness::Priming { waited: 0 }
        };
    }
    pub(super) fn source_discontinuity(&mut self) {
        self.epoch = None;
        self.readiness = SourceReadiness::Starved;
        self.loss = true;
    }
    pub(super) fn suspend(&mut self) {
        // A user-requested OFF invalidates PCM without claiming a clock failure.
        self.loss = false;
        self.reset(self.epoch.unwrap_or(([0; 16], 0, 0)), self.published);
        self.epoch = None;
        self.activity = InputActivity::Inactive;
    }
    fn admit_snapshot(&mut self, snapshot: &RenderSnapshot) -> bool {
        self.reconfigure = (snapshot.clock.sample_rate != 0
            && snapshot.clock.sample_rate != self.rate)
            || usize::try_from(snapshot.channels).ok() != Some(self.channels)
            || (snapshot.stream_epoch == 0 && snapshot.activity == InputActivity::Active);
        let was_active = self.activity == InputActivity::Active;
        self.activity = if self.reconfigure {
            InputActivity::Unknown
        } else if snapshot.clock.running == 0 {
            InputActivity::Inactive
        } else {
            snapshot.activity
        };
        let epoch = (
            snapshot.clock.incarnation,
            snapshot.clock.epoch,
            snapshot.stream_epoch,
        );
        if self.activity != InputActivity::Active || snapshot.clock.running == 0 {
            self.reset(epoch, snapshot.through);
            return false;
        }
        if self.epoch != Some(epoch) || snapshot.through < self.published {
            // An unexpected replacement during active playback is a real loss,
            // unlike initial admission or the explicitly suspended OFF/ON path.
            self.loss |= was_active && self.epoch.is_some();
            self.reset(epoch, snapshot.through);
        }
        true
    }
    pub(super) fn begin(&mut self, transport: &mut impl RenderTransport) {
        if transport.render_observation_pending() {
            // Existing converted PCM stays readable; never turn a contended
            // metadata read into removal, a new epoch or filter reconstruction.
            return;
        }
        let Some(snapshot) = transport.render_snapshot() else {
            self.activity = InputActivity::Unknown;
            self.epoch = None;
            self.readiness = SourceReadiness::Starved;
            self.reconfigure = true;
            return;
        };
        if !self.admit_snapshot(&snapshot) {
            return;
        }
        let epoch = (
            snapshot.clock.incarnation,
            snapshot.clock.epoch,
            snapshot.stream_epoch,
        );
        if self.cursor == 0 && self.published == 0 && snapshot.through != 0 {
            // HAL's first packet need not start at sample zero. Nothing before
            // this publication belongs to the newly admitted source timeline.
            // Use actual native packet geometry; do not infer origin from silence.
            if snapshot.quantum == 0 {
                return;
            }
            self.cursor = snapshot.through.saturating_sub(u64::from(snapshot.quantum));
            self.published = self.cursor;
        }
        let advanced = snapshot.through - self.published;
        let unread = snapshot.through.saturating_sub(self.cursor);
        if unread > self.capacity as u64 / 2 || advanced > u64::from(self.rate) {
            self.loss = true;
            self.reset(epoch, snapshot.through);
            return;
        }
        self.burst = self.burst.max(snapshot.quantum as usize).max(
            usize::try_from(advanced)
                .unwrap_or(usize::MAX)
                .min(self.capacity / 8),
        );
        self.target = (self.burst + 128).min(self.capacity / 2);
        if advanced != 0 {
            let queued = usize::try_from(unread)
                .unwrap_or(self.capacity)
                .saturating_add(self.bridge.buffered_input_frames());
            if self
                .bridge
                .observe_clock(ClockFeedback {
                    queue: FeedbackQueue::Input,
                    epoch: epoch.2,
                    queued_frames: queued,
                    target_frames: self.target,
                    elapsed_frames: usize::try_from(advanced).unwrap_or(usize::MAX),
                    clock_running: self.readiness == SourceReadiness::Ready,
                })
                .is_err()
            {
                self.loss = true;
                self.reset(epoch, snapshot.through);
                return;
            }
        }
        self.published = snapshot.through;
        self.feed(transport);
        if self.bridge.buffered_input_frames() >= self.target {
            self.readiness = SourceReadiness::Ready;
        }
    }
    fn feed(&mut self, transport: &mut impl RenderTransport) {
        for _ in 0..8 {
            let count = usize::try_from(self.published - self.cursor)
                .unwrap_or(self.capacity)
                .min(MAX_TRANSFER)
                .min(
                    self.capacity
                        .saturating_sub(self.bridge.queued_input_frames()),
                );
            if count == 0 {
                break;
            }
            let frames = u32::try_from(count).expect("bounded source transfer");
            let samples = &mut self.input[..count * self.channels];
            // Native read returns a count of valid positions, not necessarily a
            // prefix. Reject a partial packet as a gap; never compact holes or
            // enqueue zero padding as if it were actual source PCM.
            if transport.read_render(self.cursor, samples, frames) != frames
                || self.bridge.push_interleaved(samples).is_err()
            {
                self.loss = true;
                self.reset(self.epoch.unwrap_or(([0; 16], 0, 0)), self.published);
                break;
            }
            self.cursor += u64::from(frames);
        }
    }
    pub(super) fn read(&mut self, destination: &mut [f32]) -> TransferOutcome {
        destination.fill(0.0);
        if let SourceReadiness::Priming { waited } = &mut self.readiness {
            // Bound startup by two observed producer bursts plus converter input.
            // Requested graph time advances this budget even if HAL stops publishing.
            *waited += (destination.len() as u64 / self.channels as u64 * u64::from(self.rate))
                .div_ceil(u64::from(self.graph_rate));
            if *waited > (self.target * 2 + 128) as u64 {
                self.readiness = SourceReadiness::Starved;
            }
        }
        let mut result = TransferOutcome {
            frames: 0,
            discontinuity: std::mem::take(&mut self.loss),
            activity: if self.activity == InputActivity::Active
                && matches!(self.readiness, SourceReadiness::Priming { .. })
            {
                InputActivity::Priming
            } else {
                self.activity
            },
        };
        if self.readiness != SourceReadiness::Ready || self.activity != InputActivity::Active {
            return result;
        }
        if let Ok(report) = self.bridge.pull_interleaved(destination) {
            result.frames =
                u32::try_from(report.transferred_frames).expect("bounded converted source");
            if report.transferred_frames < destination.len() / self.channels {
                self.readiness = SourceReadiness::Starved;
            }
        } else {
            result.discontinuity = true;
            self.readiness = SourceReadiness::Starved;
        }
        result
    }
    pub(super) fn synchronized(&self) -> bool {
        self.activity == InputActivity::Inactive
            || (self.activity == InputActivity::Active
                && self.readiness != SourceReadiness::Starved
                && !self.loss
                && !self.reconfigure)
    }
    pub(super) fn reconfigure(&self) -> bool {
        self.reconfigure
    }
}
