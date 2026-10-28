//! Bounded HAL configuration steps and the rollback that follows a failed transaction.
//!
//! While the HAL performs a step it processes no newer inventory, so a rollback
//! published earlier would spend its own budget queued behind that step. A failed
//! step therefore returns only once the HAL has settled it or the settle bound ends.
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

/// Why a step did not apply within `ACK_TIMEOUT`.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Rejected before or during the step; the HAL is not performing it.
    Rejected(DriverError),
    /// Applied, but only after `ACK_TIMEOUT`.
    Late,
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
            Failure::Late => Self::Backend("HAL configuration perform/readback timed out".into()),
            Failure::Pending => Self::ConfigurationPending,
        }
    }
}

/// Waits for one step to apply, then for a late step to settle within the bound.
pub(crate) fn step(
    pacing: &impl Pacing,
    mut observe: impl FnMut() -> Progress,
) -> Result<(), Failure> {
    let mut deadline = pacing.now() + ACK_TIMEOUT;
    let mut late = false;
    loop {
        match observe() {
            Progress::Applied if late => return Err(Failure::Late),
            Progress::Applied => return Ok(()),
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
        // A late rollback still restored the previous state.
        Ok(()) | Err(Failure::Late) => Err(failure.into()),
        Err(Failure::Pending) => Err(DriverError::ConfigurationPending),
        Err(Failure::Rejected(rollback)) => Err(DriverError::Backend(format!(
            "{}; rollback failed: {rollback}",
            DriverError::from(failure)
        ))),
    }
}

#[cfg(test)]
mod tests;
