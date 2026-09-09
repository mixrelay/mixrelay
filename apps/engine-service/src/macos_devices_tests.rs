use super::*;
use tonenari_audio_graph::{AudioDeviceInputConfig, AudioDeviceOutputConfig, NodeSpec};
use tonenari_macos_test_support::DeviceFixture as Fixture;
fn wait_until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "native device transition timed out"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn document(uid: &str, revision: u64, gain: f32) -> GraphDocument {
    GraphDocument::new(
        revision,
        vec![
            NodeSpec {
                id: "input".into(),
                label: None,
                config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector: DeviceSelector::PersistentUid { uid: uid.into() },
                    output_channels: 2,
                    gain_db: gain,
                }),
            },
            NodeSpec {
                id: "output".into(),
                label: None,
                config: NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                    selector: DeviceSelector::PersistentUid { uid: uid.into() },
                    input_channels: 2,
                }),
            },
        ],
        vec![],
    )
}

#[test]
fn native_coreaudio_recovery_retains_graph_owners_and_offline_gain_edits() {
    let (mut fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 256);
    resources.reconcile(&document(&uid, 1, 0.0)).unwrap();
    let mut active = DeviceSet::default();
    // This test is the single serialized graph consumer, as in production.
    unsafe {
        resources.activate(&mut active);
    }
    resources.reclaim();
    let input = Arc::as_ptr(&active.0["input"]);
    let output = Arc::as_ptr(&active.0["output"]);
    for cycle in 0..4 {
        let epoch = active.0["input"].epoch.load(Ordering::Acquire);
        fixture.command("disconnect");
        wait_until(|| {
            active
                .0
                .values()
                .all(|device| device.failed.load(Ordering::Acquire))
        });
        // Changing gain while the physical device is absent must not try to
        // prepare a new native stream or block an otherwise valid graph edit.
        resources
            .reconcile(&document(&uid, cycle + 2, -6.0))
            .unwrap();
        assert_eq!(Arc::as_ptr(&active.0["input"]), input);
        assert_eq!(Arc::as_ptr(&active.0["output"]), output);
        fixture.command("connect");
        wait_until(|| {
            active
                .0
                .values()
                .all(|device| !device.failed.load(Ordering::Acquire))
                && active.0["input"].epoch.load(Ordering::Acquire) > epoch
        });
        let mut pcm = [0.0; 512];
        wait_until(|| active.read("input", &mut pcm).unwrap().frames > 0);
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        assert!(active.write("output", &[0.0; 512]).unwrap().frames > 0);
        eprintln!(
            "CoreAudio actual removal/reappearance cycle={cycle}; native input/output resumed; same graph owners"
        );
    }
    fixture.command("disconnect");
    wait_until(|| {
        active
            .0
            .values()
            .all(|device| device.failed.load(Ordering::Acquire))
    });
    let weak_input = Arc::downgrade(&active.0["input"]);
    let weak_output = Arc::downgrade(&active.0["output"]);
    resources
        .reconcile(&GraphDocument::new(10, vec![], vec![]))
        .unwrap();
    unsafe {
        resources.activate(&mut active);
    }
    resources.reclaim();
    assert!(weak_input.upgrade().is_none() && weak_output.upgrade().is_none());
    fixture.command("connect");
    assert!(
        active.0.is_empty(),
        "device arrival resurrected deleted nodes"
    );
}

fn drive_native_graph(active: &DeviceSet, rate: u32) {
    let clock_start = active.hardware_clock_progress().unwrap().native_frames;
    let mut graph_frames = 0_u64;
    let mut captured = 0;
    let mut submitted = 0;
    let native_start =
        unsafe { tn_device_io_native_frames(active.0["input"].native.pointer.as_ptr()) };
    let overflow_start =
        unsafe { tn_device_io_overflows(active.0["input"].native.pointer.as_ptr()) };
    let epoch_start = active.0["input"].epoch.load(Ordering::Acquire);
    let output_overflow_start =
        unsafe { tn_device_io_overflows(active.0["output"].native.pointer.as_ptr()) };
    let mut pcm = [0.0; 512];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let clock = active.hardware_clock_progress().unwrap();
        let through =
            (clock.native_frames - clock_start) * u64::from(rate) / u64::from(clock.native_rate);
        // Native credit is the clock; sleep only yields CPU, never sets rate.
        while graph_frames + 256 <= through {
            captured += active.read("input", &mut pcm).unwrap().frames;
            submitted += active.write("output", &[0.0; 512]).unwrap().frames;
            assert!(pcm.iter().all(|sample| sample.is_finite()));
            graph_frames += 256;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let native_frames =
        unsafe { tn_device_io_native_frames(active.0["input"].native.pointer.as_ptr()) }
            - native_start;
    let overflows = unsafe { tn_device_io_overflows(active.0["input"].native.pointer.as_ptr()) }
        - overflow_start;
    let output_overflows =
        unsafe { tn_device_io_overflows(active.0["output"].native.pointer.as_ptr()) }
            - output_overflow_start;
    let epoch_changed = active.0["input"].epoch.load(Ordering::Acquire) != epoch_start;
    eprintln!(
        "hardware SRC graph={rate} native_frames={native_frames} input_overflows={overflows} output_overflows={output_overflows} epoch_changed={epoch_changed} captured_frames={captured} submitted_frames={submitted} input_ready={:?} output_ready={:?}",
        active.clock_synchronized("input"),
        active.clock_synchronized("output")
    );
    assert!(
        captured > 0 && submitted > 0,
        "native SRC must transfer real packets"
    );
    assert_eq!(
        overflows, 0,
        "native-paced graph must not drop input packets"
    );
    assert_eq!(
        output_overflows, 0,
        "native-paced graph must not drop output packets"
    );
    assert_eq!(active.clock_synchronized("input"), Some(true));
    assert_eq!(active.clock_synchronized("output"), Some(true));
}

#[test]
fn native_coreaudio_all_buffer_sizes_are_applied_and_survive_reconnection() {
    let (mut fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 256);
    let graph = document(&uid, 1, 0.0);
    resources.reconcile(&graph).unwrap();
    let mut active = DeviceSet::default();
    unsafe {
        resources.activate(&mut active);
    }
    resources.reclaim();
    let mut previous = 256;
    for &frames in tonenari_audio_io::formats::BUFFER_SIZES {
        let mut prepared = resources
            .prepare_format_change(
                &graph,
                RenderFormat {
                    sample_rate: 48_000,
                    block_frames: previous,
                },
                RenderFormat {
                    sample_rate: 48_000,
                    block_frames: frames,
                },
            )
            .unwrap();
        prepared.configure_native().unwrap();
        prepared.commit();
        let verified = || {
            active.0.values().all(|device| unsafe {
                tn_device_io_quantum(device.native.pointer.as_ptr(), false) == frames
            })
        };
        wait_until(verified);
        fixture.command("disconnect");
        wait_until(|| {
            active
                .0
                .values()
                .all(|device| device.failed.load(Ordering::Acquire))
        });
        fixture.command("connect");
        wait_until(|| active.0.values().all(|device| device.ready()) && verified());
        eprintln!(
            "CoreAudio client buffer actual callbacks={frames} after apply and native reconnect"
        );
        previous = frames;
    }
}

#[test]
fn native_coreaudio_src_uses_actual_rate_and_rebuilds_graph_format_only() {
    let (_fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 44_100, 256);
    let graph = document(&uid, 1, 0.0);
    resources.reconcile(&graph).unwrap();
    let mut active = DeviceSet::default();
    unsafe {
        resources.activate(&mut active);
    }
    resources.reclaim();
    let native_input_rate = active.0["input"].native_rate.load(Ordering::Acquire);
    let native_output_rate = active.0["output"].native_rate.load(Ordering::Acquire);
    let before = Arc::as_ptr(&active.0["output"]);
    let mut old = RenderFormat {
        sample_rate: 44_100,
        block_frames: 256,
    };
    for rate in [44_100, 96_000, 48_000] {
        let next = RenderFormat {
            sample_rate: rate,
            block_frames: 256,
        };
        // No concurrent graph processing here: commit models its acknowledged
        // pause boundary. This test never invokes a native device-rate setter.
        if next != old {
            resources
                .prepare_format_change(&graph, old, next)
                .unwrap()
                .commit();
        }
        wait_until(|| active.0.values().all(|device| device.ready()));
        let clock = active.hardware_clock_progress().unwrap();
        let master_was_set = active.0["output"].clock_aligned.load(Ordering::Acquire);
        active.set_clock_master(Some(clock.owner_id));
        if !master_was_set {
            wait_until(|| {
                active.0["output"].epoch.load(Ordering::Acquire) != clock.epoch
                    && active.0["output"].ready()
            });
        }
        drive_native_graph(&active, rate);
        assert_eq!(Arc::as_ptr(&active.0["output"]), before);
        assert_eq!(
            active.0["input"].native_rate.load(Ordering::Acquire),
            native_input_rate
        );
        assert_eq!(
            active.0["output"].native_rate.load(Ordering::Acquire),
            native_output_rate
        );
        eprintln!(
            "native input/output rates retained at {native_input_rate}/{native_output_rate}; same owners"
        );
        old = next;
    }
    // The fixture follows the installed endpoint, which the user may run at
    // any supported rate. The invariant above is preserving its actual rates,
    // not imposing an unrelated 48 kHz assumption on the fixture.
    assert!(tonenari_audio_io::formats::SAMPLE_RATES.contains(&native_input_rate));
    assert_eq!(native_input_rate, native_output_rate);
    drop(active);
    resources.reclaim();
}

#[test]
fn native_coreaudio_repeated_initial_connections_preserve_failure_diagnostics() {
    for iteration in 0..20 {
        let (_fixture, uid) = Fixture::start();
        let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
        let rate = [44_100, 48_000, 96_000][iteration % 3];
        let resources = MacDevices::new(status, rate, 256);
        let graph = document(&uid, 1, 0.0);
        resources.reconcile(&graph).unwrap_or_else(|error| {
            panic!("initial connection iteration={iteration} graph_rate={rate}: {error}")
        });
        let mut active = DeviceSet::default();
        unsafe {
            resources.activate(&mut active);
        }
        assert!(active.0.values().all(|device| device.ready()));
    }
}
