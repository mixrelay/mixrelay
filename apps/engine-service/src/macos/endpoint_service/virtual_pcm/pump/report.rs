//! Installed evidence of source admission latency and of client PCM the pump lost.
//! The pump records host ticks on RT; every log is written on control.
use mixrelay_macos_transport::client_pcm::ClientPcmClock;
use mixrelay_realtime::Timebase;
use std::time::Duration;

/// Loss reports repeat at most this often while losses continue.
const LOSS_INTERVAL: Duration = Duration::from_secs(1);

/// Milestones since the previous admission was logged, in host ticks.
#[derive(Default)]
struct Admission {
    first_pcm: Option<u64>,
    format: Option<u64>,
    deferrals: u32,
    converter: Option<(u64, ClientPcmClock, u64, u64)>,
}

/// Cumulative client frames that never reached the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Losses {
    /// Overwritten in a client queue before the pump drained them.
    pub(super) skipped: u64,
    /// Dropped from the full engine backlog.
    pub(super) dropped: u64,
    /// Retained but not converted when their native timeline stopped.
    pub(super) stopped: u64,
}
impl Losses {
    pub(super) fn log(self) {
        tracing::warn!(
            boundary = "hal_shared_pcm",
            stage = "source_retention",
            queue_skipped_frames = self.skipped,
            retention_dropped_frames = self.dropped,
            stopped_unpublished_frames = self.stopped,
            "virtual input lost client PCM"
        );
    }
}

pub(super) struct Report {
    prepared: u64,
    admission: Admission,
    stopped: u64,
    reported: Losses,
    reported_at: Option<u64>,
}

impl Report {
    /// `prepared` is the host time the endpoint's pump was created on control.
    pub(super) fn new(prepared: u64) -> Self {
        Self {
            prepared,
            admission: Admission::default(),
            stopped: 0,
            reported: Losses::default(),
            reported_at: None,
        }
    }
    pub(super) fn retained(&mut self, now: u64) {
        self.admission.first_pcm.get_or_insert(now);
    }
    pub(super) fn format_requested(&mut self, now: u64) {
        self.admission.format.get_or_insert(now);
    }
    pub(super) fn deferred(&mut self) {
        self.admission.deferrals = self.admission.deferrals.saturating_add(1);
    }
    pub(super) fn converter_ready(
        &mut self,
        now: u64,
        native: ClientPcmClock,
        graph_rate: u64,
        retained: u64,
    ) {
        self.admission.converter = Some((now, native, graph_rate, retained));
    }
    pub(super) fn stopped(&mut self, frames: u64) {
        self.stopped = self.stopped.saturating_add(frames);
    }

    /// Whether a ready converter has yet to publish its first PCM.
    pub(super) fn awaiting_publication(&self) -> bool {
        self.admission.converter.is_some()
    }

    /// Logs every admission step relative to native sample zero (the device's IO start),
    /// then times the next admission. Control only.
    pub(super) fn log_admission(&mut self, now: u64, timebase: Timebase, published: usize) {
        let admission = std::mem::take(&mut self.admission);
        let Some((ready, native, graph_rate, retained)) = admission.converter else {
            return;
        };
        let since = |at: u64| {
            let milliseconds =
                |ticks| i64::try_from(timebase.nanoseconds(ticks) / 1_000_000).unwrap_or(i64::MAX);
            if at >= native.anchor {
                milliseconds(at - native.anchor)
            } else {
                -milliseconds(native.anchor - at)
            }
        };
        tracing::info!(
            boundary = "hal_shared_pcm",
            stage = "source_admission",
            native_rate = native.rate,
            graph_rate,
            endpoint_ms = since(self.prepared),
            first_pcm_ms = ?admission.first_pcm.map(since),
            format_ms = ?admission.format.map(since),
            converter_ms = since(ready),
            published_ms = since(now),
            epoch_deferrals = admission.deferrals,
            retained_frames = retained,
            published_frames = published,
            "virtual input source admitted"
        );
    }

    /// Losses changed since the last report, once its interval has passed.
    pub(super) fn due_losses(
        &mut self,
        now: u64,
        timebase: Timebase,
        (skipped, dropped): (u64, u64),
    ) -> Option<Losses> {
        let losses = Losses {
            skipped,
            dropped,
            stopped: self.stopped,
        };
        if losses == self.reported
            || self
                .reported_at
                .is_some_and(|at| now.saturating_sub(at) < timebase.ticks(LOSS_INTERVAL))
        {
            return None;
        }
        self.reported = losses;
        self.reported_at = Some(now);
        Some(losses)
    }

    /// Losses not yet reported when the pump ends.
    pub(super) fn final_losses(&self, (skipped, dropped): (u64, u64)) -> Option<Losses> {
        let losses = Losses {
            skipped,
            dropped,
            stopped: self.stopped,
        };
        (losses != self.reported).then_some(losses)
    }
}
