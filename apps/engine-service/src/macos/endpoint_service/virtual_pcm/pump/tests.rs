//! Drives real pump cycles against HAL-shaped client queues and a graph source,
//! with a synthetic host clock instead of real waits.
use super::*;
use mixrelay_macos_transport::{
    application_clock::VERSION,
    client_pcm::{ClientPcmSnapshot, ClientPcmWriter},
    endpoint_transport::EndpointTransportConfig,
    source_control::SourceRequest,
    source_registry::{SchedulingClock, SourceRegistry},
};
use std::time::{Duration, Instant};

struct FixedClock(Instant);
impl SchedulingClock for FixedClock {
    fn now(&self) -> Instant {
        self.0
    }
}

const SOURCE: &str = "virtual:fixture";

fn source_token() -> u64 {
    SOURCE
        .bytes()
        .fold(14_695_981_039_346_656_037, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        })
}

/// A frame's identity survives mixing, retention and same-clock conversion bit for bit.
fn codeword(frame: u64) -> f32 {
    f32::from_bits(0x3f00_0000 + u32::try_from(frame).unwrap())
}

/// Field order is drop order: the pump releases its source lease before the registry.
struct Fixture {
    pump: Pump,
    writer: ClientPcmWriter,
    registry: SourceRegistry,
    scheduling: Scheduling,
    timebase: Timebase,
    rate: u32,
    anchor: u64,
    /// Native frames the client has published on the current timeline.
    written: u64,
}

impl Fixture {
    fn new(rate: u32) -> Self {
        let timebase = Timebase::current();
        let anchor = timebase.ticks(Duration::from_secs(1));
        let mut writer = ClientPcmWriter::new(
            EndpointTransportConfig {
                endpoint_id: [1; 16],
                capacity_frames: QUEUE_FRAMES,
                render_channels: 2,
                capture_channels: 2,
            },
            42,
        )
        .unwrap();
        assert!(writer.set_clock((2, anchor), rate, true, true));
        let mut snapshot = ClientPcmSnapshot::new().unwrap();
        assert!(snapshot.push(&writer));
        // SAFETY: the snapshot retains the producer-created grants throughout decoding.
        let queues = unsafe { ClientPcmSnapshot::decode(snapshot.as_raw()) }.unwrap();
        let registry = SourceRegistry::with_clock(Box::new(FixedClock(Instant::now())));
        let mut request = SourceRequest {
            channels: 2,
            capacity_frames: mixrelay_audio_io::retention::RING_FRAMES,
            transport_version: VERSION,
            ..SourceRequest::default()
        };
        for (target, byte) in request.source_id.iter_mut().zip(SOURCE.bytes()) {
            *target = std::ffi::c_char::from_ne_bytes([byte]);
        }
        let transport = SourceTransport::create(&registry, &request).unwrap();
        let pump = Pump::prepare(
            queues,
            transport,
            Arc::default(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        Self {
            pump,
            writer,
            registry,
            scheduling: Scheduling::standard(),
            timebase,
            rate,
            anchor,
            written: 0,
        }
    }

    /// The client publishes one native period; the pump cycles once at that playhead.
    fn period(&mut self) {
        let pcm: [f32; 256] = std::array::from_fn(|index| {
            let value = codeword(self.written + index as u64 / 2);
            if index % 2 == 0 { value } else { -value }
        });
        assert!(self.writer.write(self.written, &pcm, 128));
        self.written += 128;
        self.cycle();
    }

    /// One pump cycle with the playhead a quarter period past the published frames.
    fn cycle(&mut self) {
        let nanos = (u128::from(self.written) + 32) * 1_000_000_000 / u128::from(self.rate);
        let now = self.anchor
            + self
                .timebase
                .ticks(Duration::from_nanos(u64::try_from(nanos).unwrap()));
        self.pump
            .cycle(now, self.timebase, &self.scheduling)
            .unwrap();
    }

    /// The graph requests its format at the native rate and paces from this source,
    /// so conversion is a same-clock copy that keeps every sample and coordinate.
    fn admit(&self) {
        let _ = self.registry.source_time(source_token(), self.rate, 128);
        assert!(self.registry.set_clock_master(source_token(), true));
    }

    /// Published graph frames and native credit of the current source epoch.
    fn progress(&self) -> (u64, u64, u64) {
        self.registry
            .pcm_progress(source_token())
            .map_or((0, 0, 0), |progress| {
                (
                    progress.clock.epoch,
                    progress.published_frames,
                    progress.clock.native_frames,
                )
            })
    }

    fn time(&self, frame: u64) -> u64 {
        let offset = u128::from(frame) * 1_000_000_000 / u128::from(self.rate);
        self.timebase.nanoseconds(self.anchor) + u64::try_from(offset).unwrap()
    }

    /// Reads every complete block of `[first, end)` at its original native coordinate.
    fn verify(&self, first: u64, end: u64) {
        let mut output = [0.0_f32; 256];
        for block in (first..end).step_by(128) {
            let read =
                self.registry
                    .read_at(source_token(), &mut output, self.rate, self.time(block));
            assert_eq!(read, 128, "no PCM at native frame {block}");
            for (index, frame) in output.chunks_exact(2).enumerate() {
                let expected = codeword(block + index as u64);
                assert_eq!(
                    (frame[0].to_bits(), frame[1].to_bits()),
                    (expected.to_bits(), (-expected).to_bits()),
                    "native frame {} moved or was lost",
                    block + index as u64
                );
            }
        }
    }
}

/// The client plays while the graph defers its format for `deferred` seconds, longer
/// than a client queue holds. Admission must then publish the whole head, in order and
/// at its native coordinates, at most `FEED_PERIODS` periods per cycle.
fn deferred_admission(rate: u32, deferred: Duration) {
    let mut fixture = Fixture::new(rate);
    let periods =
        u64::try_from(deferred.as_nanos() * u128::from(rate) / 1_000_000_000).unwrap() / 128;
    assert!(periods * 128 > u64::from(QUEUE_FRAMES));
    let measured = allocation_counter::measure(|| {
        for _ in 0..periods {
            fixture.period();
        }
    });
    assert_eq!(
        measured.count_total, 0,
        "retention allocated on the RT pump"
    );
    assert_eq!(fixture.pump.backlog.frames(), periods * 128);
    assert_eq!(fixture.progress(), (0, 0, 0));
    fixture.admit();
    // The admission cycle builds the converter on control; it may allocate there.
    fixture.period();
    let (epoch, mut published, credit) = fixture.progress();
    assert_ne!(epoch, 0);
    assert_eq!((published, credit), (FEED_PERIODS as u64 * 128, published));
    let mut cycles = 1;
    let measured = allocation_counter::measure(|| {
        while fixture.pump.backlog.frames() != 0 {
            fixture.period();
            let (current, total, credit) = fixture.progress();
            assert_eq!(current, epoch, "catch-up restarted the source stream");
            assert!(
                (1..=FEED_PERIODS as u64 * 128).contains(&(total - published)),
                "one cycle published {} frames",
                total - published
            );
            assert_eq!(credit, total, "credit diverged from published PCM");
            published = total;
            cycles += 1;
        }
        // Live periods follow catch-up one for one.
        for _ in 0..8 {
            fixture.period();
            assert_eq!(fixture.progress().1, fixture.written);
        }
    });
    assert_eq!(measured.count_total, 0, "catch-up allocated on the RT pump");
    // Seven periods of catch-up per cycle beyond its own live period.
    assert_eq!(cycles, periods.div_ceil(FEED_PERIODS as u64 - 1));
    assert_eq!(fixture.pump.mixer.skipped_frames(), 0);
    assert_eq!(fixture.pump.backlog.dropped_frames(), 0);
    fixture.verify(0, fixture.written);
}

#[test]
fn a_four_second_admission_delay_at_48_khz_publishes_the_whole_head_in_bounded_steps() {
    deferred_admission(48_000, Duration::from_secs(4));
}

#[test]
fn a_one_and_a_half_second_admission_delay_at_192_khz_publishes_the_whole_head() {
    deferred_admission(192_000, Duration::from_millis(1_500));
}

#[test]
fn a_queue_overrun_before_admission_keeps_the_earlier_retained_run() {
    let mut fixture = Fixture::new(48_000);
    for _ in 0..64 {
        fixture.period();
    }
    // The pump stalls while the client publishes past a whole queue.
    let gap = 1_024;
    let stalled = u64::from(QUEUE_FRAMES) + gap;
    let pcm = [0.25_f32; 256];
    for _ in 0..stalled / 128 {
        assert!(fixture.writer.write(fixture.written, &pcm, 128));
        fixture.written += 128;
    }
    fixture.cycle();
    assert_eq!(fixture.pump.mixer.skipped_frames(), gap);
    assert_eq!(
        fixture.pump.backlog.frames(),
        64 * 128 + u64::from(QUEUE_FRAMES)
    );
    fixture.admit();
    let mut epochs: Vec<(u64, u64)> = Vec::new();
    while fixture.pump.backlog.frames() != 0 {
        fixture.period();
        let (epoch, published, _) = fixture.progress();
        match epochs.last_mut() {
            Some((current, total)) if *current == epoch => *total = published,
            _ => epochs.push((epoch, published)),
        }
    }
    // The run before the gap is published whole; the gap restarts the source stream.
    assert_eq!(epochs.len(), 2, "{epochs:?}");
    assert_eq!(epochs[0].1, 64 * 128);
    assert_eq!(epochs[1].1, fixture.written - (64 * 128 + gap));
}

#[test]
fn native_stop_ends_retention_while_the_graph_format_is_unset() {
    let mut fixture = Fixture::new(48_000);
    for _ in 0..375 {
        fixture.period();
    }
    assert_eq!(fixture.pump.backlog.frames(), 375 * 128);
    assert!(
        fixture
            .writer
            .set_clock((2, fixture.anchor), 48_000, false, false)
    );
    fixture.cycle();
    assert_eq!(fixture.pump.backlog.frames(), 0);
    assert_eq!(
        fixture.pump.report.final_losses((0, 0)).unwrap().stopped,
        375 * 128
    );
    // The next native timeline is retained from its own start, without the stopped one.
    fixture.anchor += fixture.timebase.ticks(Duration::from_secs(10));
    assert!(
        fixture
            .writer
            .set_clock((3, fixture.anchor), 48_000, true, true)
    );
    fixture.written = 0;
    for _ in 0..16 {
        fixture.period();
    }
    fixture.admit();
    for _ in 0..8 {
        fixture.period();
    }
    assert_eq!(fixture.progress().1, fixture.written);
    fixture.verify(0, fixture.written);
}
