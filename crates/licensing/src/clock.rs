//! The calendar time licence decisions are made at.
use time::OffsetDateTime;

/// Reads the current UTC time. Provider expiry dates are calendar times, so this is a
/// wall clock rather than a monotonic one.
pub trait UtcClock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

/// The system's UTC clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUtcClock;

impl UtcClock for SystemUtcClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
