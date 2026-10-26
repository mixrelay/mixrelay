//! Scheduling policy belongs to the virtual source worker, outside native callbacks.
use super::Error;
use mixrelay_realtime::{RealtimeThread, demote_current_thread};
use std::{cell::Cell, time::Duration};

pub(super) struct Scheduling {
    realtime: Option<RealtimeThread>,
    failed_transitions: Cell<u64>,
}
impl Scheduling {
    pub(super) fn new() -> Self {
        let period = Duration::from_secs_f64(f64::from(super::cadence::FRAMES) / 48_000.0);
        let realtime = RealtimeThread::promote(period, period * 2 / 5, period * 4 / 5)
            .inspect_err(|error| tracing::warn!(boundary = "virtual_pcm_schedule", %error, "virtual input uses standard scheduling"))
            .ok();
        Self {
            realtime,
            failed_transitions: Cell::new(0),
        }
    }
    pub(super) fn retune(&self, rate: u32) {
        if let Some(realtime) = &self.realtime {
            let period =
                Duration::from_secs_f64(f64::from(super::cadence::FRAMES) / f64::from(rate));
            realtime.retune(period, period * 2 / 5, period * 4 / 5);
        }
    }
    pub(super) fn outside<R>(&self, operation: impl FnOnce() -> R) -> R {
        let Some(realtime) = &self.realtime else {
            return operation();
        };
        let (result, transition) = realtime.outside(operation);
        if transition.is_err() {
            self.failed_transitions
                .set(self.failed_transitions.get().saturating_add(1));
        }
        result
    }
    pub(super) fn finish(self) -> Result<(), Error> {
        demote_current_thread().map_err(Error::Scheduling)?;
        if self.failed_transitions.get() != 0 {
            tracing::warn!(
                boundary = "virtual_pcm_schedule",
                failed_transitions = self.failed_transitions.get(),
                "virtual input scheduling transitions failed"
            );
        }
        Ok(())
    }
}
