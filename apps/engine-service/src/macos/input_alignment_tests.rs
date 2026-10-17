//! Real native callbacks, SRC queues, graph admission and a shared acquisition cursor.
use super::*;
use crate::macos_devices::{
    MacDevices,
    callback_fixture::{CallbackTime, NativeOwner, publish},
};
use mixrelay_audio_io::{process::ClockDomain, session::ClockAuthority};
use mixrelay_realtime::Timebase;

struct Capture<'a> {
    io: MacEngineExternalIo<'a>,
    pcm: Vec<f32>,
}
impl ExternalIo for Capture<'_> {
    fn begin_cycle(
        &mut self,
        cycle: &ProcessCycle,
        controls: mixrelay_audio_engine::ExternalControlState<'_>,
    ) -> mixrelay_audio_engine::CycleAdmission {
        self.io.begin_cycle(cycle, controls)
    }
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        output: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        self.io.read_source(binding, output, cycle)
    }
    fn write_sink(
        &mut self,
        _: &ExternalBinding,
        samples: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        self.pcm.extend_from_slice(samples);
        TransferOutcome::complete(cycle.frames)
    }
}

fn processor() -> Box<BlockProcessor> {
    let document = serde_json::from_value(serde_json::json!({
        "schemaVersion":2,"revision":1,"nodes":[
            {"id":"a","type":"audio_device_input","config":{"selector":{"match":"persistent_uid","uid":"a"},"outputChannels":2,"gainDb":0}},
            {"id":"b","type":"audio_device_input","config":{"selector":{"match":"persistent_uid","uid":"b"},"outputChannels":2,"gainDb":0}},
            {"id":"out","type":"audio_device_output","config":{"selector":{"match":"persistent_uid","uid":"out"},"inputChannels":2}}
        ],"edges":[
            {"id":"a-out","source":{"nodeId":"a","portId":"audio"},"destination":{"nodeId":"out","portId":"audio"}},
            {"id":"b-out","source":{"nodeId":"b","portId":"audio"},"destination":{"nodeId":"out","portId":"audio"}}
        ]
    })).unwrap();
    create_initial_processor(
        &document,
        RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        },
        Arc::new(PeakMeterBank::new(8, 128)),
    )
    .unwrap()
}

struct Input {
    rate: f64,
    frames: u32,
    first_ns: f64,
    sample: u32,
    channel: usize,
    pcm: Vec<f32>,
}
impl Input {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded synthetic timestamps and PCM"
    )]
    fn until(&mut self, now: f64, owner: &NativeOwner, timebase: Timebase) {
        while self.first_ns + f64::from(self.sample + self.frames) * 1e9 / self.rate <= now {
            let start = self.first_ns + f64::from(self.sample) * 1e9 / self.rate;
            for (offset, frame) in self.pcm.chunks_exact_mut(2).enumerate() {
                let time = start + f64::from(u32::try_from(offset).unwrap()) * 1e9 / self.rate;
                frame.fill(0.0);
                let event_distance = (time - 20e6 + 50e6).rem_euclid(100e6) - 50e6;
                frame[self.channel] = (-(event_distance / 150_000.0).powi(2)).exp() as f32 * 0.25;
            }
            let host = |nanos: f64| timebase.ticks(Duration::from_nanos(nanos.round() as u64));
            owner.callback(
                &mut self.pcm,
                CallbackTime {
                    now: host(now),
                    host: host(start),
                    sample: f64::from(self.sample),
                    rate_scalar: 1.0,
                },
            );
            self.sample += self.frames;
        }
    }
}

#[test]
fn missing_input_coordinates_do_not_mute_the_healthy_route() {
    let status = Arc::new(AdapterRuntimeStatus::new(adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 128);
    let a = NativeOwner::new(2, 48_000, true);
    let b = NativeOwner::new(2, 48_000, true);
    let output = NativeOwner::new(2, 48_000, false);
    let devices = publish(&resources, &[("a", &a), ("b", &b), ("out", &output)]);
    devices.set_clock_master(None);
    let _a_bridge = a.serve();
    let _b_bridge = b.serve();
    let _output_bridge = output.serve();
    let mut capture = Capture {
        io: MacEngineExternalIo::new(&devices),
        pcm: Vec::with_capacity(128 * 256),
    };
    let mut processor = processor();
    let timebase = Timebase::current();
    let mut healthy = [0.0; 256];
    let mut unknown = [0.0; 256];
    for frame in healthy.chunks_exact_mut(2) {
        frame[0] = 0.25;
    }
    for frame in unknown.chunks_exact_mut(2) {
        frame[1] = 0.5;
    }
    for block in 0..128 {
        let now = timebase.ticks(Duration::from_nanos(
            1_000_000_000 + u64::from(block) * 128 * 1_000_000_000 / 48_000,
        ));
        a.callback(
            &mut healthy,
            CallbackTime {
                now,
                host: now,
                sample: f64::from(block * 128),
                rate_scalar: 1.0,
            },
        );
        b.callback(
            &mut unknown,
            CallbackTime {
                now,
                host: 0,
                sample: f64::from(block * 128),
                rate_scalar: 1.0,
            },
        );
        tick_output(&output, timebase, block);
        let measured = allocation_counter::measure(|| {
            processor
                .process_external_cycle(cycle(block), &mut capture)
                .unwrap()
                .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
    assert!(
        capture.io.align_inputs,
        "fixture must retain both active inputs"
    );
    assert!(capture.pcm.len() >= 4096);
    for frame in capture.pcm[capture.pcm.len() - 4096..].chunks_exact(2) {
        assert!(
            (frame[0] - 0.25).abs() < 1e-4,
            "healthy route held in priming: {frame:?}"
        );
        assert_eq!(
            frame[1].to_bits(),
            0,
            "unknown PCM was assigned invented coordinates"
        );
    }
}

fn cycle(block: u32) -> ProcessCycle {
    ProcessCycle {
        clock: ClockDomain {
            authority: ClockAuthority::Device,
            id: 77,
            epoch: 1,
        },
        sample_rate: 48_000,
        frames: 128,
        first_frame: u64::from(block) * 128,
        monotonic_ns: None,
        discontinuity: false,
    }
}

#[test]
fn independent_inputs_with_drift_and_different_bursts_align_the_same_event() {
    let status = Arc::new(AdapterRuntimeStatus::new(adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 128);
    let a = NativeOwner::at_rates(2, 48_000, 44_100, true);
    let b = NativeOwner::at_rates(2, 48_000, 48_000, true);
    let output = NativeOwner::new(2, 48_000, false);
    let devices = publish(&resources, &[("a", &a), ("b", &b), ("out", &output)]);
    // Both native rates differ from the graph's pacing clock, so neither bypasses SRC.
    devices.set_clock_master(None);
    let _a_bridge = a.serve();
    let _b_bridge = b.serve();
    let _output_bridge = output.serve();
    let mut a_input = Input {
        rate: 44_100.0 * 1.0003,
        frames: 64,
        first_ns: 17.0 * 1e9 / 48_000.0,
        sample: 0,
        channel: 0,
        pcm: vec![0.0; 128],
    };
    let mut b_input = Input {
        rate: 48_000.0 * 0.9998,
        frames: 512,
        first_ns: 73.0 * 1e9 / 48_000.0,
        sample: 0,
        channel: 1,
        pcm: vec![0.0; 1024],
    };
    let mut capture = Capture {
        io: MacEngineExternalIo::new(&devices),
        pcm: Vec::with_capacity(2_000_000),
    };
    let mut processor = processor();
    let timebase = Timebase::current();
    for block in 0..7500_u32 {
        let now = f64::from(block + 1) * 128.0 * 1e9 / 48_000.0;
        a_input.until(now, &a, timebase);
        b_input.until(now, &b, timebase);
        tick_output(&output, timebase, block);
        let measured = allocation_counter::measure(|| {
            processor
                .process_external_cycle(cycle(block), &mut capture)
                .unwrap()
                .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
    let left = events(&capture.pcm, 0);
    let right = events(&capture.pcm, 1);
    assert_eq!(
        left.len(),
        197,
        "missing aligned events: left={left:?} right={right:?}"
    );
    assert_eq!(left.len(), right.len(), "left={left:?} right={right:?}");
    for (left, right) in left.iter().zip(right) {
        assert!(
            left.abs_diff(right) <= 1,
            "event mismatch: {left} vs {right}"
        );
    }
}

fn events(pcm: &[f32], channel: usize) -> Vec<usize> {
    pcm.chunks_exact(2)
        .map(|frame| frame[channel])
        .enumerate()
        .collect::<Vec<_>>()
        .windows(3)
        .filter_map(|triplet| {
            (triplet[1].0 > 12_000
                && triplet[1].1 > 0.2
                && triplet[1].1 > triplet[0].1
                && triplet[1].1 >= triplet[2].1)
                .then_some(triplet[1].0)
        })
        .collect::<Vec<_>>()
}

fn tick_output(output: &NativeOwner, timebase: Timebase, block: u32) {
    output.callback(
        &mut [0.0; 256],
        CallbackTime {
            now: timebase.ticks(Duration::from_nanos(
                u64::from(block + 1) * 128 * 1_000_000_000 / 48_000,
            )),
            host: timebase.ticks(Duration::from_nanos(
                u64::from(block + 2) * 128 * 1_000_000_000 / 48_000,
            )),
            sample: f64::from(block * 128),
            rate_scalar: 1.0,
        },
    );
}
