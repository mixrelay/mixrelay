//! Native host-clock coordinates admitted to the virtual worker's processing cycle.
#[cfg(test)]
mod tests;

/// The virtual worker period, distinct from HAL callback and ZTS periods.
pub(super) const FRAMES: u32 = 128;

pub(super) fn frame(elapsed_nanos: u64, rate: u32) -> Option<u64> {
    if rate == 0 {
        return None;
    }
    let frame = u64::try_from(u128::from(elapsed_nanos) * u128::from(rate) / 1_000_000_000).ok()?;
    // A poll observes clock progress; only a completed native period releases
    // processing work. The mixer's cursor still drains every retained frame.
    Some(frame / u64::from(FRAMES) * u64::from(FRAMES))
}
