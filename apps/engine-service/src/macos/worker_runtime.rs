//! Realtime scheduling, event-driven waits and per-path statistics for the graph worker.
//!
//! The worker renders graph blocks in the time-constraint class and steps out of it for
//! control work (activation, checkpoints, format gating, direct-mode hand-off). While a
//! device clock paces it, the worker sleeps until that device's callback publishes credit
//! instead of polling. Every ten seconds a reporting thread logs which paths ran and how
//! quickly the worker woke; the worker itself never writes logs.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use mixrelay_realtime::{
    LatencyHistogram, LatencySummary, Notifier, RealtimeThread, Timebase, Transition, Wake,
};

/// Clock IDs `next_native_pace` reports for the pacing master.
pub(super) const HARDWARE_CLOCK: u64 = 2;
pub(super) const APPLICATION_CLOCK: u64 = 3;

/// Polling interval when no device callback can wake the worker (app capture or timer
/// pacing, or no wakeup semaphore).
const POLL: Duration = Duration::from_micros(250);
/// Longest wait for device credit before the worker re-examines its clocks anyway.
const LONGEST_WAIT: Duration = Duration::from_millis(5);
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// Execution paths, counted per report window.
#[derive(Clone, Copy)]
pub(super) enum Path {
    /// The graph moved into a device `IOProc` (direct rendering).
    Direct,
    Hardware,
    Application,
    Timer,
}

#[derive(Default, Clone, Copy)]
struct Counts {
    direct: u64,
    hardware: u64,
    application: u64,
    timer: u64,
    idle_waits: u64,
    timeouts: u64,
}

/// One report window, handed to the reporting thread.
struct Window {
    counts: Counts,
    latency: Option<LatencySummary>,
    realtime: bool,
}

pub(super) struct WorkerRuntime {
    realtime: Option<RealtimeThread>,
    attempted: bool,
    period: Duration,
    wake: Option<&'static Notifier>,
    timebase: Timebase,
    latency: LatencyHistogram,
    counts: Counts,
    reported: Instant,
    reporter: Option<Reporter>,
}

/// Logs windows off the audio thread. The worker only `try_lock`s the slot (never
/// waits, never wakes anyone); the reporter polls it on its own clock.
struct Reporter {
    slot: Arc<Mutex<Option<Window>>>,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

fn log(window: &Window) {
    let Window {
        counts,
        latency,
        realtime,
    } = window;
    tracing::info!(
        direct = counts.direct,
        hardware_blocks = counts.hardware,
        application_blocks = counts.application,
        timer_blocks = counts.timer,
        idle_waits = counts.idle_waits,
        wait_timeouts = counts.timeouts,
        wake_p50_us = latency.map(|summary| summary.p50_us),
        wake_p99_us = latency.map(|summary| summary.p99_us),
        wake_max_us = latency.map(|summary| summary.maximum_us),
        realtime,
        "graph worker paths"
    );
}

fn start_reporter() -> Option<Reporter> {
    let slot = Arc::new(Mutex::new(None::<Window>));
    let stop = Arc::new(AtomicBool::new(false));
    let (shared, stopping) = (Arc::clone(&slot), Arc::clone(&stop));
    std::thread::Builder::new()
        .name("mixrelay-worker-report".into())
        .spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_secs(1));
                let window = shared.lock().ok().and_then(|mut slot| slot.take());
                if let Some(window) = &window {
                    log(window);
                }
            }
        })
        .inspect_err(|error| tracing::warn!(%error, "graph worker paths are not reported"))
        .ok()
        .map(|thread| Reporter { slot, stop, thread })
}

/// The process's device-credit wakeup: device callbacks signal it once registered.
/// `None` when the kernel refused the semaphore; the worker then keeps polling.
fn device_wakeup() -> Option<&'static Notifier> {
    static WAKE: std::sync::OnceLock<Option<Notifier>> = std::sync::OnceLock::new();
    WAKE.get_or_init(|| {
        let wake = Notifier::new()
            .inspect_err(|error| tracing::warn!(%error, "graph worker polls for device credit"))
            .ok()?;
        Some(wake)
    })
    .as_ref()
    .inspect(|wake| {
        let wake: &'static Notifier = wake;
        mixrelay_macos_transport::device_io::on_device_credit(move || wake.notify());
    })
}

/// Time-constraint budget for a block period: compute within 40 %, finish within 80 %.
fn budget(period: Duration) -> (Duration, Duration, Duration) {
    (period, period.mul_f64(0.4), period.mul_f64(0.8))
}

impl WorkerRuntime {
    pub(super) fn new() -> Self {
        Self {
            realtime: None,
            attempted: false,
            period: Duration::ZERO,
            wake: device_wakeup(),
            timebase: Timebase::current(),
            latency: LatencyHistogram::default(),
            counts: Counts::default(),
            reported: Instant::now(),
            reporter: start_reporter(),
        }
    }

    /// Runs control work at standard priority (property queries, allocation, joins, waits
    /// that are not audio deadlines), then returns to the realtime class.
    pub(super) fn off_realtime<R>(&self, work: impl FnOnce() -> R) -> R {
        let Some(realtime) = &self.realtime else {
            return work();
        };
        let (result, transition) = realtime.outside(work);
        match transition {
            Ok(()) => {}
            Err(Transition::Demote(error)) => {
                tracing::warn!(%error, "graph worker ran control work in the realtime class");
            }
            Err(Transition::Promote(error)) => {
                tracing::warn!(%error, "graph worker could not re-enter the realtime class");
            }
        }
        result
    }

    /// Enters (once) or retunes the realtime class for blocks of `period`.
    pub(super) fn render_at(&mut self, period: Duration) {
        if period.is_zero() || period == self.period {
            return;
        }
        self.period = period;
        let (period, computation, constraint) = budget(period);
        if let Some(realtime) = &self.realtime {
            // A new budget applies on re-entry; re-enter now so this block already has it.
            realtime.retune(period, computation, constraint);
            self.off_realtime(|| ());
            return;
        }
        if std::mem::replace(&mut self.attempted, true) {
            return;
        }
        self.realtime = RealtimeThread::promote(period, computation, constraint)
            .inspect_err(|error| tracing::warn!(%error, "graph worker runs at standard priority"))
            .ok();
    }

    /// Leaves the realtime class for good (worker shutdown) and stops the reporter.
    pub(super) fn finish(&mut self) {
        if self.realtime.take().is_some()
            && let Err(error) = mixrelay_realtime::demote_current_thread()
        {
            tracing::warn!(%error, "graph worker stops in the realtime class");
        }
        if let Some(reporter) = self.reporter.take() {
            reporter.stop.store(true, Ordering::Release);
            reporter.thread.thread().unpark();
            let _ = reporter.thread.join();
        }
    }

    pub(super) fn record(&mut self, path: Path) {
        let count = match path {
            Path::Direct => &mut self.counts.direct,
            Path::Hardware => &mut self.counts.hardware,
            Path::Application => &mut self.counts.application,
            Path::Timer => &mut self.counts.timer,
        };
        *count += 1;
        // Entering direct mode may last indefinitely: close the window now.
        self.report(matches!(path, Path::Direct));
    }

    /// Waits for frame credit. A device clock wakes the worker from its callback; other
    /// masters (app capture in another process, timer) are polled.
    pub(super) fn wait_for_credit(&mut self, clock_id: u64) {
        self.counts.idle_waits += 1;
        let Some(wake) = self.wake.filter(|_| clock_id == HARDWARE_CLOCK) else {
            std::thread::sleep(POLL);
            self.report(false);
            return;
        };
        let longest = self.period.saturating_mul(2).clamp(POLL, LONGEST_WAIT);
        let waiting_since = mixrelay_realtime::host_time();
        match wake.wait(longest) {
            // Only a publication made while this thread waited measures wake latency; an
            // older pending token (left from direct mode or a previous block) does not.
            Wake::Notified { published_at } if published_at >= waiting_since => {
                let now = mixrelay_realtime::host_time();
                self.latency
                    .record(self.timebase.nanoseconds(now.saturating_sub(published_at)));
            }
            Wake::Notified { .. } => {}
            Wake::TimedOut => self.counts.timeouts += 1,
            Wake::Failed(_) => std::thread::sleep(POLL),
        }
        self.report(false);
    }

    /// Hands the window to the reporting thread every ten seconds (or now); never blocks.
    fn report(&mut self, now: bool) {
        if !now && self.reported.elapsed() < REPORT_EVERY {
            return;
        }
        self.reported = Instant::now();
        let window = Window {
            counts: std::mem::take(&mut self.counts),
            latency: self.latency.take(),
            realtime: self.realtime.is_some(),
        };
        // A busy slot (the reporter is reading it) drops this window rather than wait.
        if let Some(Ok(mut slot)) = self
            .reporter
            .as_ref()
            .map(|reporter| reporter.slot.try_lock())
        {
            *slot = Some(window);
        }
    }
}
