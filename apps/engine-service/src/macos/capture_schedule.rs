use super::native_transport::EndpointSnapshot;
use super::{NativePacer, Pace, native_transport};

/// Admit only the latest native callback window. Retained mapping capacity is
/// storage, not permission to enqueue old callbacks ahead of a live reader.
pub(super) fn pace(
    pacer: &mut NativePacer,
    endpoint: Option<&EndpointSnapshot>,
    rate: u32,
    maximum: u32,
    capacity: u32,
) -> Pace {
    let quantum = endpoint.map_or(0, |value| {
        value.clock.capture_frames.max(value.clock.render_frames)
    });
    let clock = endpoint
        .map(|value| native_transport::clock(value).prefer_render(value.render_activity == 3));
    pacer.next_callback(clock, rate, maximum.min(quantum), capacity.min(quantum))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    mapping: [u8; 16],
    clock: u64,
    pcm: u64,
}

/// Schedules at most one observed native quantum ahead of the capture reader.
/// DSP block splitting never adds latency or extends the retained queue budget.
#[derive(Debug, Default)]
pub(super) struct CaptureSchedule {
    identity: Option<Identity>,
    next_frame: u64,
}

impl CaptureSchedule {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn reserve(
        &mut self,
        endpoint: &EndpointSnapshot,
        frames: u32,
        capacity: u32,
    ) -> Option<u64> {
        let clock = endpoint.clock;
        let quantum = clock.capture_frames.max(clock.render_frames);
        if clock.running == 0
            || clock.capture_frames == 0
            || clock.capture_through == 0
            || endpoint.capture_channels == 0
            || endpoint.capture_epoch == 0
            || frames == 0
            || quantum > capacity
            || frames > quantum
        {
            self.reset();
            return None;
        }
        let identity = Identity {
            mapping: endpoint.incarnation,
            clock: clock.epoch,
            pcm: endpoint.capture_epoch,
        };
        let start = if self.identity == Some(identity) {
            self.next_frame.max(clock.capture_through)
        } else {
            clock.capture_through
        };
        let end = start.checked_add(u64::from(frames))?;
        let limit = clock.capture_through.checked_add(u64::from(quantum))?;
        if end > limit || end >= 1 << 48 {
            return None;
        }
        self.identity = Some(identity);
        self.next_frame = end;
        Some(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(through: u64, render: u32, capture: u32) -> EndpointSnapshot {
        let mut endpoint = EndpointSnapshot {
            incarnation: [1; 16],
            capture_epoch: 1,
            capture_channels: 2,
            ..EndpointSnapshot::default()
        };
        endpoint.clock.epoch = 1;
        endpoint.clock.running = 1;
        endpoint.clock.sample_rate = 48_000;
        endpoint.clock.capture_through = through;
        endpoint.clock.capture_frames = capture;
        endpoint.clock.render_frames = render;
        endpoint
    }

    #[test]
    fn coalesced_callbacks_admit_the_latest_quantum_and_report_expired_credit() {
        for accumulated in [64, 96] {
            let mut pacer = NativePacer::default();
            let mut schedule = CaptureSchedule::default();
            let initial = endpoint(32, 32, 32);
            assert!(matches!(
                pace(&mut pacer, Some(&initial), 48_000, 128, 16_384),
                Pace::Block { frames: 32, .. }
            ));
            assert_eq!(schedule.reserve(&initial, 32, 16_384), Some(32));
            let through = 32 + accumulated;
            let current = endpoint(through, 32, 32);
            assert_eq!(
                pace(&mut pacer, Some(&current), 48_000, 128, 16_384),
                Pace::Block {
                    first_frame: through - 32,
                    frames: 32,
                    epoch: 1,
                    discontinuity: true,
                    overrun: true,
                }
            );
            assert_eq!(schedule.reserve(&current, 32, 16_384), Some(through));
            assert_eq!(
                pace(&mut pacer, Some(&current), 48_000, 128, 16_384),
                Pace::Wait
            );
        }
    }

    #[test]
    fn prepared_small_blocks_split_a_native_window_without_discarding_credit() {
        let mut pacer = NativePacer::default();
        let mut schedule = CaptureSchedule::default();
        let current = endpoint(256, 256, 256);
        for first in (0..256).step_by(32) {
            assert!(matches!(
                pace(&mut pacer, Some(&current), 48_000, 32, 16_384),
                Pace::Block { first_frame, frames: 32, overrun: false, .. }
                    if first_frame == first
            ));
            assert_eq!(schedule.reserve(&current, 32, 16_384), Some(256 + first));
        }
        assert_eq!(
            pace(&mut pacer, Some(&current), 48_000, 32, 16_384),
            Pace::Wait
        );
    }

    #[test]
    fn thirty_two_and_two_fifty_six_follow_the_reader_without_fixed_latency() {
        for quantum in [32, 256] {
            let mut schedule = CaptureSchedule::default();
            for callback in 1..=1000_u64 {
                let through = callback * u64::from(quantum);
                let snapshot = endpoint(through, quantum, quantum);
                assert_eq!(schedule.reserve(&snapshot, quantum, 16_384), Some(through));
                assert_eq!(schedule.reserve(&snapshot, quantum, 16_384), None);
            }
        }
    }

    #[test]
    fn split_dsp_blocks_fill_one_native_quantum_without_dropping_its_tail() {
        for (render, capture) in [(256, 32), (32, 256), (256, 256)] {
            let snapshot = endpoint(9000, render, capture);
            let mut schedule = CaptureSchedule::default();
            for block in 0..8 {
                assert_eq!(
                    schedule.reserve(&snapshot, 32, 16_384),
                    Some(9000 + block * 32)
                );
            }
            assert_eq!(schedule.reserve(&snapshot, 32, 16_384), None);
        }
    }

    #[test]
    fn a_stalled_consumer_cannot_accumulate_more_than_one_quantum() {
        let mut schedule = CaptureSchedule::default();
        let snapshot = endpoint(1000, 32, 32);
        assert_eq!(schedule.reserve(&snapshot, 32, 16_384), Some(1000));
        for _ in 0..1000 {
            assert_eq!(schedule.reserve(&snapshot, 32, 16_384), None);
        }
        assert_eq!(
            schedule.reserve(&endpoint(9000, 32, 32), 32, 16_384),
            Some(9000)
        );
    }

    #[test]
    fn every_identity_boundary_discards_its_previous_cursor() {
        let original = endpoint(1000, 32, 32);
        for changed in [0, 1, 2] {
            let mut schedule = CaptureSchedule::default();
            assert_eq!(schedule.reserve(&original, 32, 16_384), Some(1000));
            let mut replacement = endpoint(32, 32, 32);
            match changed {
                0 => replacement.incarnation = [2; 16],
                1 => replacement.clock.epoch = 2,
                2 => replacement.capture_epoch = 2,
                _ => unreachable!(),
            }
            assert_eq!(schedule.reserve(&replacement, 32, 16_384), Some(32));
        }
    }

    #[test]
    fn inactive_missing_or_out_of_bounds_callbacks_do_not_invent_a_deadline() {
        let mut schedule = CaptureSchedule::default();
        for snapshot in [
            EndpointSnapshot::default(),
            endpoint(0, 32, 32),
            endpoint(32, 32, 0),
        ] {
            assert_eq!(schedule.reserve(&snapshot, 32, 16_384), None);
        }
        let snapshot = endpoint(32, 32, 32);
        assert_eq!(schedule.reserve(&snapshot, 0, 16_384), None);
        assert_eq!(schedule.reserve(&snapshot, 32, 16), None);
        assert_eq!(
            schedule.reserve(&endpoint(u64::MAX, 32, 32), 32, 16_384),
            None
        );
    }
}
