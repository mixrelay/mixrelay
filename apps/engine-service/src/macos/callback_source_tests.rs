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
}
impl RenderTransport for Producer {
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
        },
    )
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
    assert!(source.ready);
    source.begin(&mut producer);
    assert!(
        source.ready,
        "unchanged mapping does not re-prime on each callback"
    );
    producer.incarnation = [2; 16];
    producer.sample = 0.0;
    source.begin(&mut producer);
    assert!(!source.ready);
    assert_eq!(source.bridge.buffered_input_frames(), 0);
    assert_eq!(source.read(&mut pcm).frames, 0);
    for _ in 0..64 {
        producer.through += 32;
        source.begin(&mut producer);
        source.read(&mut pcm);
        assert!(pcm.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
    assert!(source.ready);
}
