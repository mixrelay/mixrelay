//! The production clock over the standard library's monotonic time.
use crate::{Deadline, Instant, MonotonicClock, Sleeper};

/// The standard library's monotonic clock, measured from when this value was created.
///
/// `std::time::Instant` behaves the same on every platform, so this is the one production
/// implementation of the ports. Composition roots construct it and pass it in; code under
/// test receives a `ManualClock` instead.
#[derive(Clone, Copy, Debug)]
pub struct StdClock {
    origin: std::time::Instant,
}

impl StdClock {
    /// A clock whose origin is now.
    #[must_use]
    #[expect(
        clippy::new_without_default,
        reason = "a Default would hand tests the real clock without anyone choosing it"
    )]
    pub fn new() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

impl MonotonicClock for StdClock {
    fn now(&self) -> Instant {
        Instant::from_origin(self.origin.elapsed())
    }
}

impl Sleeper for StdClock {
    fn sleep_until(&self, deadline: Deadline) {
        std::thread::sleep(deadline.remaining(self.now()));
    }
}

#[cfg(test)]
mod tests;
