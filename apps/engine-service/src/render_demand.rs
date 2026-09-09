//! Native render demand translated into bounded graph cycles. The cumulative
//! count is in negotiated stream frames, NOT `IAudioClock`'s opaque tick units.
//! A wakeup only prompts a recheck: it never authorizes timer-generated audio.

use tonenari_audio_io::{
    process::{ClockDomain, ProcessCycle},
    session::ClockAuthority,
};

#[derive(Default)]
pub struct RenderDemandCursor {
    previous: Option<(u64, u64)>,
    discontinuity: bool,
}

impl RenderDemandCursor {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// One graph owner consumes a monotonically increasing native demand count.
    /// On attachment discard preparation-time demand. On a delayed wakeup cap
    /// catch-up to ring capacity, explicitly marking the lost interval; never
    /// synthesize an unbounded burst of obsolete audio.
    #[must_use]
    pub fn observe(
        &mut self,
        stream_id: u64,
        total_frames: u64,
        sample_rate: u32,
        capacity: u32,
    ) -> Option<ProcessCycle> {
        if capacity == 0 || sample_rate == 0 {
            return None;
        }
        let previous = self.previous.replace((stream_id, total_frames));
        let Some((id, position)) = previous.filter(|(id, _)| *id == stream_id) else {
            self.discontinuity = true;
            return None;
        };
        if total_frames < position {
            self.discontinuity = true;
            return None;
        }
        let pending = total_frames - position;
        if pending == 0 {
            return None;
        }
        let frames = u32::try_from(pending).unwrap_or(capacity).min(capacity);
        Some(ProcessCycle {
            clock: ClockDomain {
                authority: ClockAuthority::Device,
                id,
                epoch: 0,
            },
            sample_rate,
            frames,
            first_frame: total_frames - u64::from(frames),
            // No wall-clock approximation masquerading as a native timestamp.
            monotonic_ns: None,
            discontinuity: std::mem::take(&mut self.discontinuity) || pending > u64::from(capacity),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonenari_audio_io::{
        process::{CycleBlocks, CycleTracker},
        session::CURRENT_TIMER_FORMAT,
    };

    #[test]
    fn only_native_frames_authorize_processing_and_remainders_are_preserved() {
        let mut cursor = RenderDemandCursor::default();
        let mut tracker = CycleTracker::default();
        assert!(cursor.observe(1, 9_000, 48_000, 4096).is_none());
        assert!(cursor.observe(1, 9_000, 48_000, 4096).is_none());
        let mut total = 9_000;
        let mut processed = 0;
        for quantum in [480, 441, 960, 1, 127, 129]
            .into_iter()
            .cycle()
            .take(10_000)
        {
            total += quantum;
            let cycle = cursor.observe(1, total, 48_000, 4096).unwrap();
            for block in CycleBlocks::new(cycle, CURRENT_TIMER_FORMAT).unwrap() {
                tracker.accept(block, CURRENT_TIMER_FORMAT).unwrap();
                processed += u64::from(block.frames);
            }
        }
        assert_eq!(processed, total - 9_000);
    }

    #[test]
    fn switch_stall_reset_and_counter_restart_are_explicit_discontinuities() {
        let mut cursor = RenderDemandCursor::default();
        assert!(cursor.observe(1, 0, 48_000, 4096).is_none());
        assert!(cursor.observe(1, 480, 48_000, 4096).unwrap().discontinuity);
        assert!(!cursor.observe(1, 960, 48_000, 4096).unwrap().discontinuity);
        let catchup = cursor.observe(1, 100_960, 48_000, 4096).unwrap();
        assert_eq!(catchup.frames, 4096);
        assert_eq!(catchup.first_frame, 96_864);
        assert!(catchup.discontinuity);
        assert!(cursor.observe(2, 3_000, 48_000, 4096).is_none());
        assert!(
            cursor
                .observe(2, 3_480, 48_000, 4096)
                .unwrap()
                .discontinuity
        );
        assert!(cursor.observe(2, 0, 48_000, 4096).is_none());
        assert!(cursor.observe(2, 480, 48_000, 4096).unwrap().discontinuity);
        cursor.reset();
        assert!(cursor.observe(2, 960, 48_000, 4096).is_none());
    }
}
