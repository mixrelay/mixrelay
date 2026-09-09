use super::*;

struct Transport {
    clock: NativeClockSnapshot,
    writes: Vec<(u64, Vec<f32>)>,
    invalidations: usize,
    can_invalidate: bool,
    can_write: bool,
    rearm_pending: bool,
}
impl CaptureTransport for Transport {
    fn capture_rearm_pending(&mut self) -> bool {
        self.rearm_pending
    }
    fn invalidate_capture(&mut self) -> bool {
        if !self.can_invalidate {
            return false;
        }
        self.invalidations += 1;
        self.rearm_pending = false;
        true
    }
    fn clock(&mut self) -> Option<NativeClockSnapshot> {
        Some(self.clock)
    }
    fn write(&mut self, frame: u64, samples: &[f32], _: u32) -> bool {
        if !self.can_write {
            return false;
        }
        self.writes.push((frame, samples.to_vec()));
        true
    }
}
fn fixture(rate: u32) -> (HalCapture, Transport) {
    let bridge = HalCapture::new(
        96_000,
        rate,
        NativeAudioConfig {
            capacity_frames: 16_384,
            render_channels: 2,
            capture_channels: 2,
        },
    )
    .unwrap();
    let transport = Transport {
        clock: NativeClockSnapshot {
            epoch: 7,
            capture_through: 9_000_000,
            sample_rate: rate,
            running: 1,
            ..Default::default()
        },
        writes: vec![],
        invalidations: 0,
        can_invalidate: true,
        can_write: true,
        rearm_pending: false,
    };
    (bridge, transport)
}
#[test]
fn independent_hal_coordinates_preserve_nonzero_pcm_at_96k_32frames() {
    let (mut bridge, mut transport) = fixture(96_000);
    for _ in 0..5000 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        assert_eq!(bridge.push(&[0.25; 64], 32).frames, 32);
        assert!(bridge.flush(&mut transport));
        assert_eq!(bridge.take_report().discontinuities, 0);
    }
    assert!(bridge.synchronized());
    assert!(transport.writes[0].0 > 9_000_000);
    let mut through = transport.writes[0].0;
    let mut frames = 0;
    for (position, samples) in &transport.writes {
        assert_eq!(
            *position, through,
            "HAL writes are contiguous, not hardware indices"
        );
        assert!(samples.iter().all(|sample| sample.is_finite()));
        through += (samples.len() / 2) as u64;
        frames += samples.len() / 2;
    }
    assert!((159_000..161_000).contains(&frames));
    let last = &transport.writes.last().unwrap().1;
    assert!(last.iter().all(|sample| (*sample - 0.25).abs() < 0.002));
}
#[test]
fn idle_and_epoch_reset_do_not_replay_old_hal_coordinates_or_pcm() {
    let (mut bridge, mut transport) = fixture(96_000);
    transport.clock.running = 0;
    for _ in 0..1000 {
        bridge.begin(&mut transport, 32);
        assert_eq!(
            bridge.push(&[0.75; 64], 32).activity,
            InputActivity::Inactive
        );
        assert!(bridge.flush(&mut transport));
        assert!(
            bridge.synchronized(),
            "idle sink does not invalidate the hardware clock"
        );
    }
    assert!(transport.writes.is_empty());
    assert_eq!(
        transport.invalidations, 1,
        "idle callbacks must not churn capture epochs"
    );
    transport.clock.running = 1;
    transport.clock.epoch = 8;
    transport.clock.capture_through = 1_000;
    for _ in 0..20 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.0; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    assert!(transport.writes[0].0 < 10_000);
    assert!(
        transport
            .writes
            .iter()
            .flat_map(|(_, pcm)| pcm)
            .all(|sample| sample.abs() < f32::EPSILON)
    );
    transport.clock.sample_rate = 48_000;
    bridge.begin(&mut transport, 32);
    assert!(bridge.reconfigure());
    assert!(!bridge.synchronized());
}
#[test]
fn different_hal_rate_converts_only_destination_frame_count() {
    let (mut bridge, mut transport) = fixture(48_000);
    for _ in 0..5000 {
        transport.clock.capture_through += 16;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.25; 64], 32);
        assert!(bridge.flush(&mut transport));
        assert_eq!(bridge.take_report().discontinuities, 0);
    }
    let frames: usize = transport.writes.iter().map(|(_, pcm)| pcm.len() / 2).sum();
    assert!((79_000..81_000).contains(&frames));
    assert_eq!(bridge.rate, 48_000);
    assert_eq!(bridge.graph_rate, 96_000);
}

#[test]
fn bursty_independent_consumer_holds_lead_and_never_counts_idle_as_progress() {
    for drift_ppm in [-250_i64, 250] {
        let (mut bridge, mut transport) = fixture(96_000);
        let mut fraction = 0_i64;
        let mut credit = 0_u64;
        let mut prior_lead = 0;
        let mut observed_burst = false;
        for _ in 0..30_000 {
            fraction += 32 * (1_000_000 + drift_ppm);
            credit += u64::try_from(fraction / 1_000_000).unwrap();
            fraction %= 1_000_000;
            let moved = credit >= 512;
            if moved {
                transport.clock.capture_through += 512;
                credit -= 512;
                observed_burst = true;
            }
            bridge.begin(&mut transport, 32);
            if observed_burst && !moved {
                assert_eq!(bridge.lead, prior_lead);
            }
            prior_lead = bridge.lead;
            bridge.push(&[0.125; 64], 32);
            assert!(bridge.flush(&mut transport));
            // Initial consumer burst can expose an unknown phase and force one
            // explicit alignment; steady demand must not reset every callback.
            if transport.clock.capture_through > 9_002_048 {
                assert_eq!(bridge.take_report().discontinuities, 0);
            } else {
                bridge.take_report();
            }
            assert!(bridge.bridge.correction_ppm().abs() <= 1000.0);
        }
        assert!(bridge.synchronized());
        assert!(bridge.consumer_burst >= 512);
        assert!(bridge.cursor.unwrap() - transport.clock.capture_through < 4096);
    }
}

#[test]
fn capture_close_during_playback_discards_backlog_before_same_epoch_resume() {
    let (mut bridge, mut transport) = fixture(96_000);
    for _ in 0..20 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.75; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    for _ in 0..5000 {
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.75; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    assert_eq!(bridge.state, DestinationState::Dormant);
    assert!(bridge.cursor.is_none());
    assert!(
        transport.invalidations >= 2,
        "published backlog was invalidated"
    );
    let baseline = transport.writes.len();
    for _ in 0..40 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.0; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    assert!(transport.writes.len() > baseline);
    assert!(
        transport.writes[baseline..]
            .iter()
            .flat_map(|(_, pcm)| pcm)
            .all(|sample| sample.abs() < f32::EPSILON)
    );
}

#[test]
fn exhausted_capture_epoch_defers_only_boundary_to_control() {
    let (mut bridge, mut transport) = fixture(96_000);
    transport.can_invalidate = false;
    bridge.begin(&mut transport, 32);
    bridge.push(&[0.75; 64], 32);
    assert!(
        bridge.flush(&mut transport),
        "HAL reset refusal must not terminate the hardware callback"
    );
    assert!(bridge.reconfigure());
    assert!(!bridge.synchronized());
    assert!(transport.writes.is_empty());
    assert_eq!(transport.invalidations, 0);
}

#[test]
fn source_gap_invalidates_tail_and_restarts_filter_with_new_pcm() {
    let (mut bridge, mut transport) = fixture(96_000);
    for _ in 0..20 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.75; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    let invalidations = transport.invalidations;
    let first = transport.writes.len();
    bridge.source_discontinuity();
    for _ in 0..20 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.0; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    assert!(transport.invalidations > invalidations);
    assert!(
        transport.writes[first..]
            .iter()
            .flat_map(|(_, pcm)| pcm)
            .all(|v| v.abs() < f32::EPSILON)
    );
}

#[test]
fn capture_rearm_racing_publication_does_not_fail_hardware_render() {
    let (mut bridge, mut transport) = fixture(96_000);
    transport.can_write = false;
    for _ in 0..16 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.25; 64], 32);
        assert!(bridge.flush(&mut transport));
        if bridge.reconfigure() {
            break;
        }
    }
    assert_eq!(bridge.state, DestinationState::AwaitingEpoch);
    assert_eq!(bridge.take_report().output_overflows, 1);
    assert!(transport.writes.is_empty());
}

#[test]
fn idle_capture_observes_rearm_without_waiting_for_pcm_publication() {
    let (mut bridge, mut transport) = fixture(96_000);
    transport.clock.running = 0;
    bridge.begin(&mut transport, 32);
    assert!(bridge.flush(&mut transport));
    assert!(bridge.synchronized());
    transport.rearm_pending = true;
    transport.can_invalidate = false;
    bridge.begin(&mut transport, 32);
    assert!(bridge.reconfigure());
    assert!(!bridge.synchronized());
    assert!(bridge.flush(&mut transport));
    assert!(transport.writes.is_empty());
    transport.can_invalidate = true;
    assert!(bridge.flush(&mut transport));
    bridge.begin(&mut transport, 32);
    assert!(bridge.synchronized());
}

#[test]
fn mapping_identity_resets_capture_history_even_when_local_epoch_is_reused() {
    let (mut bridge, mut transport) = fixture(96_000);
    for _ in 0..64 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.75; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    let invalidations = transport.invalidations;
    bridge.begin(&mut transport, 32);
    assert!(
        bridge.cursor.is_some(),
        "unchanged mapping retains its running converter"
    );
    assert_eq!(transport.invalidations, invalidations);
    let first = transport.writes.len();
    transport.clock.incarnation = [2; 16];
    bridge.begin(&mut transport, 32);
    assert!(bridge.cursor.is_none());
    assert_eq!(bridge.bridge.buffered_input_frames(), 0);
    for _ in 0..32 {
        transport.clock.capture_through += 32;
        bridge.begin(&mut transport, 32);
        bridge.push(&[0.0; 64], 32);
        assert!(bridge.flush(&mut transport));
    }
    assert!(transport.invalidations > invalidations);
    assert!(transport.writes.len() > first);
    assert!(
        transport.writes[first..]
            .iter()
            .flat_map(|(_, pcm)| pcm)
            .all(|sample| sample.abs() < f32::EPSILON)
    );
}
