//! One acquisition cursor shared by independent inputs, advanced only by graph frame credit.
use crate::process::{ClockDomain, ProcessCycle};

/// The source that supplies the oldest currently safe acquisition coordinate.
#[derive(Clone, Copy, Debug)]
pub struct CaptureFrontier {
    source: u64,
    nanos: u64,
}
impl CaptureFrontier {
    #[must_use]
    pub const fn new(source: u64, nanos: u64) -> Self {
        Self { source, nanos }
    }
    #[must_use]
    pub const fn nanos(self) -> u64 {
        self.nanos
    }
}

#[derive(Debug, Default)]
pub struct CaptureAlignment {
    clock: Option<ClockDomain>,
    rate: u32,
    next_frame: u64,
    nanos: Option<f64>,
    source: Option<u64>,
}

impl CaptureAlignment {
    /// Starts at the oldest currently safe source frontier, then preserves one
    /// continuous coordinate across callback/pump scheduling and partial graph blocks.
    /// The period belongs to the actual pacing clock, not a producer's wake time.
    /// A different limiting source can reacquire an older coordinate when it joins.
    /// A scheduling reserve change from the same source never rewinds this cursor.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "validated monotonic nanosecond coordinate and bounded native periods"
    )]
    pub fn begin(
        &mut self,
        cycle: ProcessCycle,
        frontier: Option<CaptureFrontier>,
        frame_nanos: f64,
    ) -> Option<u64> {
        let restart = self.clock != Some(cycle.clock)
            || self.rate != cycle.sample_rate
            || self.next_frame != cycle.first_frame
            || cycle.discontinuity;
        if restart {
            self.nanos = None;
        }
        if let Some(frontier) = frontier {
            if self.source != Some(frontier.source)
                && self
                    .nanos
                    .is_some_and(|next| (frontier.nanos as f64) < next)
            {
                self.nanos = None;
            }
            self.source = Some(frontier.source);
        }
        self.clock = Some(cycle.clock);
        self.rate = cycle.sample_rate;
        self.next_frame = cycle.first_frame.saturating_add(u64::from(cycle.frames));
        let nanos = self
            .nanos
            .or_else(|| frontier.map(|time| time.nanos as f64))?;
        let nominal = 1e9 / f64::from(cycle.sample_rate);
        let period = if frame_nanos.is_finite()
            && (nominal * 0.99..=nominal * 1.01).contains(&frame_nanos)
        {
            frame_nanos
        } else {
            nominal
        };
        self.nanos = Some(nanos + f64::from(cycle.frames) * period);
        Some(nanos.round() as u64)
    }
}

#[cfg(test)]
mod tests;
