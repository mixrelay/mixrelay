use super::*;

struct Producer {
    through: u64,
    epoch: u64,
    sample: f32,
    active: bool,
    hole: bool,
    reads: Vec<u64>,
    rate: u32,
    incarnation: [u8; 16],
    observation_pending: bool,
    quantum: u32,
}
impl RenderTransport for Producer {
    fn render_observation_pending(&mut self) -> bool {
        self.observation_pending
    }
    fn render_snapshot(&mut self) -> Option<RenderSnapshot> {
        Some(RenderSnapshot {
            clock: NativeClockSnapshot {
                incarnation: self.incarnation,
                epoch: 3,
                render_through: self.through,
                sample_rate: self.rate,
                running: 1,
                ..Default::default()
            },
            stream_epoch: self.epoch,
            channels: 2,
            quantum: self.quantum,
            through: self.through,
            activity: if self.active {
                InputActivity::Active
            } else {
                InputActivity::Inactive
            },
        })
    }
    fn read_render(&mut self, frame: u64, samples: &mut [f32], frames: u32) -> u32 {
        assert!(frame >= 7_000_000);
        assert!(frame + u64::from(frames) <= self.through);
        self.reads.push(frame);
        samples.fill(self.sample);
        if self.hole { frames / 2 } else { frames }
    }
}
fn fixture(rate: u32) -> (HalRender, Producer) {
    (
        HalRender::new(
            96_000,
            rate,
            NativeAudioConfig {
                capacity_frames: 16_384,
                render_channels: 2,
                capture_channels: 2,
            },
        )
        .unwrap(),
        Producer {
            through: 7_000_000,
            epoch: 8,
            sample: 0.25,
            active: true,
            hole: false,
            reads: vec![],
            rate,
            incarnation: [1; 16],
            observation_pending: false,
            quantum: 32,
        },
    )
}

#[test]
fn pending_render_observation_keeps_filter_history_and_does_not_request_rebuild() {
    let (mut source, mut producer) = fixture(48_000);
    let mut pcm = [0.0; 64];
    for _ in 0..100 {
        producer.through += 16;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    let epoch = source.epoch;
    let cursor = source.cursor;
    let queued = source.bridge.buffered_input_frames();
    producer.observation_pending = true;
    source.begin(&mut producer);
    assert_eq!((source.epoch, source.cursor), (epoch, cursor));
    assert_eq!(source.bridge.buffered_input_frames(), queued);
    assert!(!source.reconfigure());
    producer.observation_pending = false;
    producer.through += 16;
    source.begin(&mut producer);
    source.read(&mut pcm);
    assert_eq!(source.epoch, epoch);
    assert!(pcm.iter().all(|sample| (*sample - 0.25).abs() < 0.002));
    assert!(!source.reconfigure());
}

#[test]
fn suspended_source_discards_queued_pcm_and_filter_history_before_resuming() {
    let (mut source, mut producer) = fixture(48_000);
    let mut pcm = [0.0; 64];
    for _ in 0..100 {
        producer.through += 16;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert!(pcm.iter().all(|sample| (*sample - 0.25).abs() < 0.002));
    source.suspend();
    producer.through += 4096;
    producer.sample = 0.0;
    let resume = producer.through;
    producer.reads.clear();
    for _ in 0..100 {
        source.begin(&mut producer);
        assert!(!source.read(&mut pcm).discontinuity);
        assert!(pcm.iter().all(|sample| sample.abs() < 0.000_001));
        producer.through += 16;
    }
    assert!(producer.reads.iter().all(|frame| *frame >= resume));
    assert!(source.synchronized());
}
#[test]
fn source_uses_hal_positions_and_converts_48k_into_96k_hardware_blocks() {
    let (mut source, mut producer) = fixture(48_000);
    let mut pcm = [0.0; 64];
    let mut transferred = 0;
    for cycle in 0..5000 {
        producer.through += 16;
        source.begin(&mut producer);
        let read = source.read(&mut pcm);
        transferred += read.frames;
        if cycle > 100 {
            assert_eq!(read.frames, 32);
            assert!(pcm.iter().all(|v| (*v - 0.25).abs() < 0.002));
        }
    }
    assert!(transferred > 159_000);
    assert!(source.synchronized());
    assert!(!producer.reads.is_empty());
}
#[test]
fn inactive_source_and_packet_holes_do_not_become_valid_pcm() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [0.0; 64];
    producer.active = false;
    source.begin(&mut producer);
    assert_eq!(source.read(&mut pcm).activity, InputActivity::Inactive);
    assert!(producer.reads.is_empty());
    producer.active = true;
    producer.hole = true;
    producer.through += 256;
    source.begin(&mut producer);
    let read = source.read(&mut pcm);
    assert_eq!(read.frames, 0);
    assert!(read.discontinuity);
    assert!(pcm.iter().all(|v| v.abs() < f32::EPSILON));
    producer.hole = false;
    producer.sample = 0.0;
    producer.epoch += 1;
    for _ in 0..30 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert!(pcm.iter().all(|v| v.abs() < f32::EPSILON));
}
#[test]
fn bursty_source_is_consumed_by_hardware_clock_without_per_block_polling_gaps() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [0.0; 64];
    let mut holes = 0;
    for cycle in 0..20_000 {
        if cycle % 16 == 0 {
            producer.through += 512;
        }
        source.begin(&mut producer);
        let read = source.read(&mut pcm);
        if cycle > 128 && read.frames != 32 {
            holes += 1;
        }
    }
    assert_eq!(holes, 0);
    assert!(source.synchronized());
}

#[test]
fn producer_stall_clears_bridge_ready_until_real_pcm_reprime() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [0.0; 64];
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert!(source.synchronized());
    for _ in 0..64 {
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert!(
        !source.synchronized(),
        "previously primed source is not proof of current transfer"
    );
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert!(source.synchronized());
}

#[test]
fn mapping_identity_resets_source_history_even_when_both_local_epochs_are_reused() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [0.0_f32; 64];
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert_eq!(source.readiness, SourceReadiness::Ready);
    source.begin(&mut producer);
    assert!(
        source.readiness == SourceReadiness::Ready,
        "unchanged mapping does not re-prime on each callback"
    );
    producer.incarnation = [2; 16];
    producer.sample = 0.0;
    source.begin(&mut producer);
    assert_ne!(source.readiness, SourceReadiness::Ready);
    assert_eq!(source.bridge.buffered_input_frames(), 0);
    assert_eq!(source.read(&mut pcm).frames, 0);
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
        assert!(pcm.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
    assert_eq!(source.readiness, SourceReadiness::Ready);
}

#[test]
fn startup_is_explicit_bounded_priming_and_never_hides_a_running_stall() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [1.0; 64];
    source.begin(&mut producer);
    let first = source.read(&mut pcm);
    assert_eq!(first.activity, InputActivity::Priming);
    assert_eq!(first.frames, 0);
    assert!(!first.discontinuity);
    assert!(
        source.synchronized(),
        "a new valid clock is not a broken clock"
    );
    assert!(pcm.iter().all(|sample| sample.abs() < f32::EPSILON));
    for _ in 0..32 {
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert_eq!(source.read(&mut pcm).activity, InputActivity::Active);
    assert!(
        !source.synchronized(),
        "startup deadline must expire without producer progress"
    );
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    assert_eq!(source.readiness, SourceReadiness::Ready);
    for _ in 0..64 {
        source.begin(&mut producer);
        assert_ne!(source.read(&mut pcm).activity, InputActivity::Priming);
    }
    assert_eq!(source.readiness, SourceReadiness::Starved);
}

#[test]
fn producer_activation_with_native_bursts_has_no_false_loss_or_clock_failure() {
    let (mut source, mut producer) = fixture(96_000);
    producer.quantum = 512;
    let mut pcm = [0.0; 64];
    let mut priming = 0;
    let mut running = 0;
    for cycle in 0..256 {
        if cycle % 16 == 0 {
            producer.through += 512;
        }
        source.begin(&mut producer);
        let report = source.read(&mut pcm);
        assert!(!report.discontinuity);
        assert!(source.synchronized());
        if report.activity == InputActivity::Priming {
            priming += 1;
        } else {
            assert_eq!(report.frames, 32);
            running += 1;
        }
    }
    assert!(priming > 0 && priming <= 32);
    assert!(running > 200);
}

#[test]
fn first_published_packet_establishes_origin_instead_of_reading_unwritten_prefix() {
    let (mut source, mut producer) = fixture(96_000);
    producer.through = 0;
    producer.quantum = 512;
    source.begin(&mut producer);
    let mut pcm = [0.0; 64];
    assert_eq!(source.read(&mut pcm).activity, InputActivity::Priming);
    for cycle in 0..128 {
        if cycle % 16 == 0 {
            producer.through = if cycle == 0 {
                7_000_512
            } else {
                producer.through + 512
            };
        }
        source.begin(&mut producer);
        let report = source.read(&mut pcm);
        assert!(
            !report.discontinuity,
            "first native publication is not missing prior PCM"
        );
        if report.activity == InputActivity::Active {
            assert_eq!(report.frames, 32);
        }
    }
    assert_eq!(producer.reads.first(), Some(&7_000_000));
}

#[test]
fn active_epoch_replacement_reports_loss_once_instead_of_becoming_startup() {
    let (mut source, mut producer) = fixture(96_000);
    let mut pcm = [0.0; 64];
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
    }
    producer.epoch += 1;
    source.begin(&mut producer);
    let lost = source.read(&mut pcm);
    assert!(lost.discontinuity);
    assert_eq!(lost.activity, InputActivity::Active);
    assert!(!source.read(&mut pcm).discontinuity);
}
