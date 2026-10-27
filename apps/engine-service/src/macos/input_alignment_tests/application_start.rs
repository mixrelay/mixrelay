//! A graph waiting for its first producer must retain the producer's initial PCM.
use super::*;
use mixrelay_macos_transport::application_source_transport::mr_application_source_transport_write_clocked;

#[test]
fn an_already_active_consumer_preserves_quiet_and_signal_published_before_its_next_poll() {
    preserve_committed_source(48_000, 8192, 0);
}

#[test]
fn a_fractional_native_rate_preserves_the_last_committed_graph_frame() {
    // Include the source FIFO's startup reserve before checking the finite audible tail.
    preserve_committed_source(44_100, 1025, 2048);
}

fn preserve_committed_source(native_rate: u32, total: u32, signal_offset: u64) {
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
    let application = ApplicationInput::at_rate(&registry, native_rate);
    assert!(registry.set_clock_master(source_token("app"), true));
    let mut processor = application_processor();
    let mut timeline = MacTimeline::new();
    let mut pacer = ApplicationPacer::default();
    pacer.await_source(source_token("app"));
    write_startup_source(&application, total, signal_offset);
    let mut capture = Capture {
        io: timeline.take_external_io(&devices),
        pcm: Vec::with_capacity(total as usize * 2),
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
    assert_eq!(capture.pcm.len(), total as usize * 2);
    assert_eq!(
        pacer.next(
            (source_token("app"), application_progress(&registry)),
            (48_000, 128, 16_384)
        ),
        Pace::Wait
    );
    let expected: Vec<_> = (0..total)
        .step_by(128)
        .flat_map(|first| {
            startup_pcm(u64::from(first) + signal_offset)
                .into_iter()
                .take((total - first).min(128) as usize * 2)
        })
        .collect();
    assert_eq!(
        capture.pcm, expected,
        "startup publication was discarded before graph acquisition"
    );
}

fn write_startup_source(application: &ApplicationInput, total: u32, signal_offset: u64) {
    for first in (0..total).step_by(128) {
        let frames = (total - first).min(128);
        let pcm = startup_pcm(u64::from(first) + signal_offset);
        let times: [u64; 128] = std::array::from_fn(|offset| {
            1_000_000_000 + (u64::from(first) + offset as u64) * 1_000_000_000 / 48_000
        });
        // SAFETY: the fixture exclusively owns this producer; both spans contain 128 frames.
        unsafe {
            assert!(mr_application_source_transport_write_clocked(
                application.owner,
                pcm.as_ptr(),
                times.as_ptr(),
                frames
            ));
        }
    }
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
