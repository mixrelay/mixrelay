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
fn a_late_forward_ack_settles_before_a_rollback_with_its_own_budget() {
    // The forward Perform outlasts both its own and a rollback's six-second budget,
    // as it did on Intel runners with retention-sized client queues.
    let settled = ACK_TIMEOUT * 2 + Duration::from_secs(2);
    let clock = Manual::new();
    let hal = hal(&clock, Some((settled, Progress::Applied)));
    let error = hal.transact().unwrap_err();
    assert!(
        matches!(&error, DriverError::Backend(message) if !message.contains("rollback")),
        "{error}"
    );
    // The rollback was published only after the forward step settled and applied in time.
    let published = hal.rollback_published.get().unwrap();
    assert!(published >= settled && published < settled + Duration::from_secs(1));
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
    let clock = Manual::new();
    let result = transact(
        || Err(Failure::Rejected(DriverError::Backend("forward".into()))),
        || Ok(()),
        |()| step(&clock, || Progress::Rejected),
    );
    assert!(
        matches!(&result, Err(DriverError::Backend(message))
            if message.ends_with("forward; rollback failed: Driver backend: HAL rejected the configuration")),
        "{result:?}"
    );
    let clock = Manual::new();
    let result = transact(
        || Err(Failure::Late),
        || Ok(()),
        |()| {
            step(&clock, || {
                Progress::applied(clock.elapsed() >= ACK_TIMEOUT + Duration::from_secs(1))
            })
        },
    );
    // A late rollback still restored the previous state; the forward timeout is reported.
    assert!(
        matches!(&result, Err(DriverError::Backend(message)) if message.contains("timed out")),
        "{result:?}"
    );
    let clock = Manual::new();
    let result = transact(
        || Err(Failure::Late),
        || Ok(()),
        |()| step(&clock, || Progress::Performing),
    );
    assert!(matches!(result, Err(DriverError::ConfigurationPending)));
}
