//! Exercise the graph acquisition cursor with real partial native publications.
use super::*;
use mixrelay_audio_io::{
    capture_alignment::{CaptureAlignment, CaptureFrontier},
    process::{ClockDomain, ProcessCycle},
    session::ClockAuthority,
};

#[test]
fn partial_native_credit_preserves_every_audible_frame_through_acquisition_alignment() {
    let mut input = AlignedInput::new();
    let measured = allocation_counter::measure(|| input.run());
    assert_eq!(measured.count_total, 0);
    input.verify();
}

struct AlignedInput {
    registry: SourceRegistry,
    transport: SourceTransport,
    converter: Converter,
    token: u64,
    writer: ClientPcmWriter,
    queues: Vec<ClientPcmReader>,
    mixer: ClientPcmMixer,
    alignment: CaptureAlignment,
    nanos: Arc<AtomicU64>,
    consumed: u64,
    published: u64,
    playhead: u64,
    scratch: [f32; 256],
    input: [f32; 256],
    times: [u64; 128],
    output: [f32; 256],
    captured: Vec<f32>,
}
impl AlignedInput {
    fn new() -> Self {
        let nanos = Arc::new(AtomicU64::new(0));
        let registry = SourceRegistry::with_clock(Box::new(Clock {
            origin: Instant::now(),
            nanos: nanos.clone(),
        }));
        let mut request = SourceRequest {
            capacity_frames: 16_384,
            channels: 2,
            transport_version: VERSION,
            ..SourceRequest::default()
        };
        for (target, byte) in request.source_id.iter_mut().zip(b"virtual:fixture") {
            *target = std::ffi::c_char::from_ne_bytes([*byte]);
        }
        let token = source_token("virtual:fixture");
        let mut transport = SourceTransport::create(&registry, &request).unwrap();
        let mut output = [0.0; 256];
        registry.read_audio(token, &mut output, 128, 48_000);
        assert!(registry.set_clock_master(token, true));
        let clock = transport.clock().unwrap();
        let converter = Converter::new(&mut transport, clock, (48_000, 2, 16_384))
            .unwrap()
            .unwrap();
        let (writer, queues) = client_grants();
        Self {
            registry,
            transport,
            converter,
            token,
            writer,
            queues,
            nanos,
            mixer: ClientPcmMixer::default(),
            alignment: CaptureAlignment::default(),
            consumed: 0,
            published: 0,
            playhead: 0,
            scratch: [0.0; 256],
            input: [0.0; 256],
            times: [0; 128],
            output,
            captured: Vec::with_capacity(50_000 * 2),
        }
    }

    fn run(&mut self) {
        // Native callbacks publish 512 frames, with the playhead observed between
        // callbacks. Delayed scheduling also drains multiple slices in one burst.
        for (step, increment) in [21, 5, 23, 79, 128, 43, 85, 10, 118, 16, 112, 39, 89]
            .into_iter()
            .cycle()
            .take(800)
            .enumerate()
        {
            self.playhead += match step {
                160 => 512,
                320 => 1_024,
                520 => 768,
                _ => increment,
            };
            self.publish_callbacks();
            self.nanos
                .store(self.playhead * 1_000_000_000 / 48_000, Ordering::Relaxed);
            while let Some(window) = self.mixer.window(&self.queues, |_| {
                (self.playhead * 1_000_000_000).div_ceil(48_000)
            }) {
                self.transfer(window);
                self.capture();
            }
        }
    }

    fn publish_callbacks(&mut self) {
        while self.published < self.playhead {
            let pcm: [f32; 1024] = std::array::from_fn(|index| {
                let frame = self.published + index as u64 / 2;
                let value = if audible(frame) {
                    f32::from(u16::try_from(frame).unwrap()) / 65_536.0
                } else {
                    0.0
                };
                if index % 2 == 0 { value } else { -value }
            });
            assert!(self.writer.write(self.published, &pcm, 512));
            self.published += 512;
        }
    }

    fn transfer(&mut self, window: mixrelay_macos_transport::client_pcm::MixWindow) {
        let frames = window.frames as usize;
        ClientPcmMixer::mix(
            &self.queues,
            window,
            &mut self.input[..frames * 2],
            &mut self.scratch,
        )
        .unwrap();
        for (offset, time) in self.times[..frames].iter_mut().enumerate() {
            *time = 1_000_000_000 + (window.first + offset as u64) * 1_000_000_000 / 48_000;
        }
        self.converter
            .observe(self.transport.clock().unwrap())
            .unwrap();
        self.converter
            .push(
                &mut self.transport,
                &mut self.input[..frames * 2],
                &self.times[..frames],
            )
            .unwrap();
        assert!(self.converter.flush(&mut self.transport).unwrap());
        self.mixer.commit(window);
        self.transport.publish_measurements(
            window.frames,
            u32::try_from(self.converter.output_quantum()).unwrap(),
            0,
        );
    }

    fn capture(&mut self) {
        let clock = self.registry.clock(self.token).unwrap();
        while self.consumed < clock.native_frames {
            let frames = u32::try_from((clock.native_frames - self.consumed).min(128)).unwrap();
            let cycle = ProcessCycle {
                clock: ClockDomain {
                    authority: ClockAuthority::Device,
                    id: 3,
                    epoch: clock.epoch,
                },
                sample_rate: 48_000,
                frames,
                first_frame: self.consumed,
                monotonic_ns: None,
                discontinuity: self.consumed == 0,
            };
            let frontier = self
                .registry
                .source_time(self.token, 48_000, frames)
                .map(|time| CaptureFrontier::new(self.token, time));
            let period = self
                .registry
                .source_period(self.token)
                .unwrap_or(1e9 / 48_000.0);
            let output = &mut self.output[..frames as usize * 2];
            output.fill(0.0);
            if let Some(time) = self.alignment.begin(cycle, frontier, period) {
                self.registry.read_at(self.token, output, 48_000, time);
            }
            self.captured.extend_from_slice(output);
            self.consumed += u64::from(frames);
        }
    }

    #[expect(
        clippy::float_cmp,
        reason = "same-clock PCM and binary codewords must match exactly, including consecutive frame spacing"
    )]
    fn verify(&self) {
        let actual: Vec<_> = self
            .captured
            .chunks_exact(2)
            .enumerate()
            .filter_map(|(index, frame)| {
                assert_eq!(frame[0], -frame[1]);
                (frame[0] != 0.0).then_some((index, frame[0]))
            })
            .collect();
        let expected: Vec<_> = (0..self.playhead)
            .filter(|&frame| audible(frame))
            .map(|frame| f32::from(u16::try_from(frame).unwrap()) / 65_536.0)
            .collect();
        assert_eq!(
            actual.len(),
            expected.len(),
            "audible PCM was lost or duplicated"
        );
        for (frame, (&(_, actual), expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(
                actual, expected,
                "source order changed at audible frame {frame}"
            );
        }
        for adjacent in actual.windows(2) {
            if adjacent[1].1 - adjacent[0].1 == 1.0 / 65_536.0 {
                assert_eq!(
                    adjacent[1].0,
                    adjacent[0].0 + 1,
                    "silence inserted inside continuous source PCM"
                );
            }
        }
    }
}

fn audible(frame: u64) -> bool {
    (4_096..20_480).contains(&frame) || (24_576..40_960).contains(&frame)
}
