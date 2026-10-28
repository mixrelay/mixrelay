//! Bounded HAL configuration steps and the rollback that follows a failed transaction.
//!
//! While the HAL performs a step it processes no newer inventory, so a rollback
//! published earlier would spend its own budget queued behind that step. A step
//! therefore returns only once the HAL has applied or rejected it, or the settle
//! bound ends. A step applied late still succeeds: rolling it back would cost a
//! second Perform and undo a configuration the HAL holds.
use mixrelay_driver_control::DriverError;
use std::time::{Duration, Instant};

/// HAL budget for applying one configuration step in normal operation.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(6);
/// Further time a step that missed `ACK_TIMEOUT` may take to settle before the
/// transaction reports it pending. It covers the slowest observed Perform (9.4 s on
/// Intel runners) twice over; raising it delays only the pending report.
pub(crate) const SETTLE_TIMEOUT: Duration = Duration::from_secs(24);
// Control-only polling interval, never audio pacing or a callback wait.
const POLL: Duration = Duration::from_millis(10);

/// The HAL's newest settled rate, format or mapping request for one endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Configured {
    /// HAL-wide settlement order.
    pub(crate) sequence: u64,
    /// The nominal rate the request asked for.
    pub(crate) rate: u32,
    pub(crate) applied: bool,
}

/// What the Driver service observes about one HAL step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Applied,
    /// The HAL aborted or failed the step and is idle again.
    Rejected,
    Performing,
}

impl Progress {
    pub(crate) fn applied(applied: bool) -> Self {
        if applied {
            Self::Applied
        } else {
            Self::Performing
        }
    }
}

/// Clock and pause between observations.
pub(crate) trait Pacing {
    fn now(&self) -> Instant;
    fn pause(&self);
}

/// Wall-clock polling for the installed service.
pub(crate) struct Polling;

impl Pacing for Polling {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn pause(&self) {
        std::thread::sleep(POLL);
    }
}

/// Why a step did not apply.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Rejected before or during the step; the HAL is not performing it.
    Rejected(DriverError),
    /// Still performing when the settle bound ended.
    Pending,
}

/// An error raised before the HAL was asked to perform anything.
impl From<DriverError> for Failure {
    fn from(error: DriverError) -> Self {
        Self::Rejected(error)
    }
}

impl From<Failure> for DriverError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Rejected(error) => error,
            Failure::Pending => Self::ConfigurationPending,
        }
    }
}

/// Progress of a rate or format request issued after the HAL had settled `since`.
/// A rate request is rejected only by a settlement for its own rate, so a concurrent
/// request for another rate is not taken for it.
pub(crate) fn request_progress(
    applied: bool,
    since: u64,
    settled: Option<Configured>,
    rate: Option<u32>,
) -> Progress {
    if applied {
        Progress::Applied
    } else if settled.is_some_and(|settled| {
        settled.sequence > since && !settled.applied && rate.is_none_or(|rate| rate == settled.rate)
    }) {
        Progress::Rejected
    } else {
        Progress::Performing
    }
}

/// Waits for one step to apply or be rejected, through `ACK_TIMEOUT` and then the
/// settle bound. A step applied within the bound succeeds; a late one is logged.
pub(crate) fn step(
    pacing: &impl Pacing,
    mut observe: impl FnMut() -> Progress,
) -> Result<(), Failure> {
    let start = pacing.now();
    let mut deadline = start + ACK_TIMEOUT;
    let mut late = false;
    loop {
        match observe() {
            Progress::Applied => {
                if late {
                    let elapsed = pacing.now().saturating_duration_since(start);
                    tracing::warn!(
                        elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                        budget_ms = u64::try_from(ACK_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
                        "HAL applied a configuration step after its acknowledgement budget"
                    );
                }
                return Ok(());
            }
            Progress::Rejected => {
                return Err(Failure::Rejected(DriverError::Backend(
                    "HAL rejected the configuration".into(),
                )));
            }
            Progress::Performing => {}
        }
        if pacing.now() >= deadline {
            if late {
                return Err(Failure::Pending);
            }
            late = true;
            deadline += SETTLE_TIMEOUT;
        }
        pacing.pause();
    }
}

/// Runs `forward`; after a settled failure, publishes the previous state and waits for
/// it with a fresh budget. A step still pending publishes the rollback without waiting,
/// so the HAL converges to the state the service keeps once it drains.
pub(crate) fn transact<T>(
    forward: impl FnOnce() -> Result<(), Failure>,
    publish_rollback: impl FnOnce() -> Result<T, DriverError>,
    restore: impl FnOnce(T) -> Result<(), Failure>,
) -> Result<(), DriverError> {
    let Err(failure) = forward() else {
        return Ok(());
    };
    let rollback = publish_rollback()?;
    if matches!(failure, Failure::Pending) {
        return Err(DriverError::ConfigurationPending);
    }
    match restore(rollback) {
        Ok(()) => Err(failure.into()),
        Err(Failure::Pending) => Err(DriverError::ConfigurationPending),
        Err(Failure::Rejected(rollback)) => Err(DriverError::Backend(format!(
            "{}; rollback failed: {rollback}",
            DriverError::from(failure)
        ))),
    }
}

#[cfg(test)]
mod tests;
