use super::*;
use std::cell::Cell;

/// A clock that advances only when the waiter pauses.
struct Manual {
    origin: Instant,
    elapsed: Cell<Duration>,
}
impl Manual {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
        }
    }
    fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
}
impl Pacing for Manual {
    fn now(&self) -> Instant {
        self.origin + self.elapsed.get()
    }
    fn pause(&self) {
        self.elapsed.set(self.elapsed.get() + POLL);
    }
}

/// One HAL that performs each published inventory in order, never two at once.
struct Hal<'a> {
    clock: &'a Manual,
    /// When the forward Perform finishes, if ever, and how.
    forward: Option<(Duration, Progress)>,
    /// How long the HAL takes to perform a rollback once it is idle.
    rollback_perform: Duration,
    rollback_published: Cell<Option<Duration>>,
}
impl Hal<'_> {
    fn forward(&self) -> Progress {
        match self.forward {
            Some((done, progress)) if self.clock.elapsed() >= done => progress,
            _ => Progress::Performing,
        }
    }
    fn idle(&self) -> Option<Duration> {
        self.forward.map(|(done, _)| done)
    }
    fn rollback(&self, published: Duration) -> Progress {
        // The HAL starts the rollback only after the forward Perform has drained.
        Progress::applied(self.idle().is_some_and(|idle| {
            self.clock.elapsed() >= idle.max(published) + self.rollback_perform
        }))
    }
    fn transact(&self) -> Result<(), DriverError> {
        transact(
            || step(self.clock, || self.forward()),
            || {
                self.rollback_published.set(Some(self.clock.elapsed()));
                Ok(self.clock.elapsed())
            },
            |published| step(self.clock, || self.rollback(published)),
        )
    }
}

fn hal(clock: &Manual, forward: Option<(Duration, Progress)>) -> Hal<'_> {
    Hal {
        clock,
        forward,
        rollback_perform: Duration::from_secs(1),
        rollback_published: Cell::new(None),
    }
}

#[test]
fn a_step_applied_within_its_budget_succeeds_without_settling() {
    let clock = Manual::new();
    let hal = hal(&clock, Some((Duration::from_secs(2), Progress::Applied)));
    assert!(hal.transact().is_ok());
    assert!(clock.elapsed() < ACK_TIMEOUT);
    assert_eq!(hal.rollback_published.get(), None);
}

#[test]
fn a_late_forward_ack_within_the_settle_bound_completes_without_rollback() {
    // The forward Perform outlasts both its own and a rollback's six-second budget,
    // as it did on Intel runners with retention-sized client queues.
    let settled = ACK_TIMEOUT * 2 + Duration::from_secs(2);
    let clock = Manual::new();
    let hal = hal(&clock, Some((settled, Progress::Applied)));
    assert!(hal.transact().is_ok());
    // Undoing the applied change would cost a second Perform and another dropout.
    assert_eq!(hal.rollback_published.get(), None);
    assert!(clock.elapsed() >= settled && clock.elapsed() < settled + Duration::from_secs(1));
}

#[test]
fn a_slow_rejection_rolls_back_only_after_the_forward_perform_drains() {
    let rejected = ACK_TIMEOUT * 2 + Duration::from_secs(2);
    let clock = Manual::new();
    let hal = hal(&clock, Some((rejected, Progress::Rejected)));
    let error = hal.transact().unwrap_err();
    assert!(matches!(&error, DriverError::Backend(message) if message.contains("rejected")));
    assert!(!error.to_string().contains("rollback failed"), "{error}");
    // Published after the HAL drained, the rollback had its own full budget.
    let published = hal.rollback_published.get().unwrap();
    assert!(published >= rejected && published < rejected + Duration::from_secs(1));
    assert!(clock.elapsed() >= published + hal.rollback_perform);
}

#[test]
fn a_rejected_forward_rolls_back_immediately() {
    let clock = Manual::new();
    let hal = hal(&clock, Some((Duration::from_secs(1), Progress::Rejected)));
    let error = hal.transact().unwrap_err();
    assert!(matches!(&error, DriverError::Backend(message) if message.contains("rejected")));
    assert!(hal.rollback_published.get().unwrap() < Duration::from_secs(2));
}

#[test]
fn a_forward_still_performing_reports_pending_after_publishing_the_rollback() {
    let clock = Manual::new();
    let hal = hal(&clock, None);
    assert!(matches!(
        hal.transact(),
        Err(DriverError::ConfigurationPending)
    ));
    // The rollback is published for the HAL to converge to once it drains, not awaited.
    let published = hal.rollback_published.get().unwrap();
    assert!(published >= ACK_TIMEOUT + SETTLE_TIMEOUT);
    assert_eq!(clock.elapsed(), published);
}

#[test]
fn a_rollback_is_failed_only_when_rejected_and_pending_when_still_performing() {
    let forward = || Err(Failure::Rejected(DriverError::Backend("forward".into())));
    let clock = Manual::new();
    let result = transact(forward, || Ok(()), |()| step(&clock, || Progress::Rejected));
    assert!(
        matches!(&result, Err(DriverError::Backend(message))
            if message.ends_with("forward; rollback failed: Driver backend: HAL rejected the configuration")),
        "{result:?}"
    );
    // A late rollback still restored the previous state; the forward error is reported.
    let clock = Manual::new();
    let result = transact(
        forward,
        || Ok(()),
        |()| {
            step(&clock, || {
                Progress::applied(clock.elapsed() >= ACK_TIMEOUT + Duration::from_secs(1))
            })
        },
    );
    assert!(
        matches!(&result, Err(DriverError::Backend(message)) if message == "forward"),
        "{result:?}"
    );
    let clock = Manual::new();
    let result = transact(
        forward,
        || Ok(()),
        |()| step(&clock, || Progress::Performing),
    );
    assert!(matches!(result, Err(DriverError::ConfigurationPending)));
}

fn configured(sequence: u64, rate: u32, applied: bool) -> Configured {
    Configured {
        sequence,
        rate,
        applied,
    }
}

#[test]
fn only_a_newer_rejection_of_the_same_request_rejects_it() {
    let rate = Some(96_000);
    let progress = |settled, rate| request_progress(false, 4, settled, rate);
    assert_eq!(
        request_progress(true, 4, Some(configured(9, 96_000, false)), rate),
        Progress::Applied
    );
    assert_eq!(progress(None, rate), Progress::Performing);
    // Settled before this request, or a request for another rate.
    assert_eq!(
        progress(Some(configured(4, 96_000, false)), rate),
        Progress::Performing
    );
    assert_eq!(
        progress(Some(configured(5, 48_000, false)), rate),
        Progress::Performing
    );
    // Applied at our rate but not yet visible in readback.
    assert_eq!(
        progress(Some(configured(5, 96_000, true)), rate),
        Progress::Performing
    );
    assert_eq!(
        progress(Some(configured(5, 96_000, false)), rate),
        Progress::Rejected
    );
    // Format requests carry the current rate, so any newer rejection is theirs.
    assert_eq!(
        progress(Some(configured(5, 48_000, false)), None),
        Progress::Rejected
    );
}

#[test]
fn a_native_rate_failure_rolls_back_promptly_with_its_own_error() {
    let clock = Manual::new();
    // The HAL reports the failed Perform 200 ms after the service asks for 96 kHz.
    let settled = || {
        (clock.elapsed() >= Duration::from_millis(200)).then_some(Configured {
            sequence: 8,
            rate: 96_000,
            applied: false,
        })
    };
    let restored = Cell::new(false);
    let result = transact(
        || {
            step(&clock, || Progress::Applied)?;
            step(&clock, || {
                request_progress(false, 7, settled(), Some(96_000))
            })
        },
        || Ok(clock.elapsed()),
        |published| {
            step(&clock, || {
                Progress::applied(clock.elapsed() >= published + POLL)
            })?;
            restored.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(&result, Err(DriverError::Backend(message)) if message == "HAL rejected the configuration"),
        "{result:?}"
    );
    assert!(restored.get());
    assert!(clock.elapsed() < Duration::from_secs(1));
}
