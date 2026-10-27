//! A graph waiting for its first producer must retain the producer's initial PCM.
use super::*;
use mixrelay_macos_transport::application_source_transport::{
    mr_application_source_transport_publish_native, mr_application_source_transport_write_timed,
};

#[test]
fn an_already_active_consumer_preserves_quiet_and_signal_published_before_its_next_poll() {
    let status = Arc::new(AdapterRuntimeStatus::new(adapter_profile()));
    let resources = MacDevices::new(status.clone(), 48_000, 128);
    let devices = publish(&resources, &[]);
    let nanos = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let registry = mixrelay_macos_transport::source_registry::SourceRegistry::with_clock(Box::new(
        SchedulingClock {
            origin: Instant::now(),
            nanos,
        },
    ));
    let application = ApplicationInput::new(&registry);
    assert!(registry.set_clock_master(source_token("app"), true));
    let mut processor = application_processor();
    let mut timeline = MacTimeline::new();
    let mut pacer = ApplicationPacer::default();
    pacer.await_source(source_token("app"));
    for first in (0..8192).step_by(128) {
        let pcm = startup_pcm(first);
        let times: [u64; 128] = std::array::from_fn(|offset| {
            1_000_000_000 + (first + offset as u64) * 1_000_000_000 / 48_000
        });
        // SAFETY: the fixture exclusively owns this producer; both spans contain 128 frames.
        unsafe {
            assert!(mr_application_source_transport_write_timed(
                application.owner,
                pcm.as_ptr(),
                times.as_ptr(),
                128
            ));
            mr_application_source_transport_publish_native(application.owner, first + 128, true);
        }
    }
    let mut capture = Capture {
        io: timeline.take_external_io(&devices),
        pcm: Vec::with_capacity(8192 * 2),
    };
    let route = RouteClock {
        maximum_frames: 128,
        synchronized: true,
        independent_idle: false,
    };
    let measured = allocation_counter::measure(|| {
        for _ in 0..64 {
            let pace = pacer.next(
                (source_token("app"), application_progress(&registry)),
                (48_000, 128, 16_384),
            );
            if !matches!(pace, Pace::Block { .. }) {
                break;
            }
            let (frames, discontinuity) = timeline.accept((pace, 3), &route, &status).unwrap();
            capture.io = timeline.take_external_io(&devices);
            capture.io.sources = Some(&registry);
            process_native_cycle(
                &mut processor,
                &mut capture,
                WorkerCycle {
                    timeline: &timeline,
                    frames,
                    discontinuity,
                },
            )
            .unwrap();
            timeline.capture_alignment = std::mem::take(&mut capture.io.alignment);
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(capture.pcm.len(), 8192 * 2);
    assert_eq!(
        pacer.next(
            (source_token("app"), application_progress(&registry)),
            (48_000, 128, 16_384)
        ),
        Pace::Wait
    );
    let expected: Vec<_> = (0..8192).step_by(128).flat_map(startup_pcm).collect();
    assert_eq!(
        capture.pcm, expected,
        "startup publication was discarded before graph acquisition"
    );
}

fn startup_pcm(first: u64) -> [f32; 256] {
    std::array::from_fn(|index| {
        let frame = first + index as u64 / 2;
        if (frame / 2048).is_multiple_of(2) {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "fixture frame IDs fit exactly within Float32"
        )]
        let value = (frame + 1) as f32 / 16384.0;
        if index % 2 == 0 { value } else { -value }
    })
}
