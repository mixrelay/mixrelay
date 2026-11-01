//! Monotonic time ports for platform-neutral domain code.
//!
//! Domain and application services read time through [`MonotonicClock`] and wait through
//! [`Sleeper`], so their tests drive time explicitly instead of waiting for real time to
//! pass. This crate makes no OS calls: adapters implement the ports with the platform
//! clock, and the `test-util` feature provides a manually advanced `ManualClock` for tests.
mod clock;
mod deadline;
mod instant;
#[cfg(any(test, feature = "test-util"))]
mod manual;
mod sleep;

pub use clock::MonotonicClock;
pub use deadline::Deadline;
pub use instant::Instant;
#[cfg(any(test, feature = "test-util"))]
pub use manual::ManualClock;
pub use sleep::Sleeper;
