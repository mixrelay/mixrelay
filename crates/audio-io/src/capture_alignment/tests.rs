use super::*;
use crate::session::ClockAuthority;

#[test]
fn producer_jitter_does_not_move_the_shared_cursor_and_restart_reacquires() {
    let mut alignment = CaptureAlignment::default();
    let mut cycle = ProcessCycle {
        clock: ClockDomain {
            authority: ClockAuthority::Device,
            id: 7,
            epoch: 1,
        },
        sample_rate: 48_000,
        frames: 64,
        first_frame: 0,
        monotonic_ns: None,
        discontinuity: false,
    };
    let base = 1_000_000_000_u64;
    for index in 0..1000_u32 {
        let frontier = Some(base + u64::from(index / 8) * 8 * 64 * 1_000_000_000 / 48_000);
        let actual = alignment.begin(cycle, frontier, 1e9 / 48_000.0).unwrap();
        let expected = base + u64::from(index) * 64 * 1_000_000_000 / 48_000;
        assert!(actual.abs_diff(expected) <= 1);
        cycle.first_frame += 64;
    }
    cycle.clock.epoch += 1;
    assert_eq!(
        alignment.begin(cycle, Some(base * 3), 1e9 / 48_000.0),
        Some(base * 3)
    );
}
