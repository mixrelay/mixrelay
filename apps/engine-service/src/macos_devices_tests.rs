use super::*;
use mixrelay_audio_graph::{AudioDeviceInputConfig, AudioDeviceOutputConfig, NodeSpec};
use mixrelay_audio_io::process::{InputActivity, TransferOutcome};
use mixrelay_macos_test_support::DeviceFixture as Fixture;
#[path = "macos_devices/live_tests/installed_driver.rs"]
mod installed_driver;
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
                control: mixrelay_audio_graph::NodeControl::default(),
                id: "input".into(),
                label: None,
                config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector: DeviceSelector::PersistentUid { uid: uid.into() },
                    output_channels: 2,
                    gain_db: gain,
                }),
            },
            NodeSpec {
                control: mixrelay_audio_graph::NodeControl::default(),
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

fn publish_recovery_now(resources: &MacDevices, active: &mut DeviceSet) {
    *resources.next_group_check.lock().unwrap() = Instant::now();
    resources.reclaim();
    resources.activation().unwrap().activate_recovered(active);
    resources.reclaim();
}

fn assert_published_offline(active: &DeviceSet, graph: &GraphDocument) {
    assert!(active.0.is_empty());
    assert_eq!(active.1, specs(graph));
    let expected = TransferOutcome {
        activity: InputActivity::Inactive,
        ..TransferOutcome::complete(32)
    };
    let mut input = [9.0; 64];
    assert_eq!(active.read("input", &mut input, 0), Some(expected));
    assert_eq!(input.map(f32::to_bits), [0.0_f32; 64].map(f32::to_bits));
    assert_eq!(active.write("output", &[0.5; 64], 0), Some(expected));
}

fn assert_cached_selection(resources: &MacDevices, graph: &GraphDocument) {
    let cached = resources
        .cache
        .lock()
        .unwrap()
        .iter()
        .map(|(id, (spec, _))| (id.clone(), spec.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(cached, specs(graph));
}

fn delete_disconnected_nodes(
    resources: &MacDevices,
    active: &mut DeviceSet,
    fixture: &mut Fixture,
) {
    let retired = active.0.values().map(Arc::downgrade).collect::<Vec<_>>();
    fixture.command("disconnect");
    wait_until(|| {
        active
            .0
            .values()
            .all(|device| device.failed.load(Ordering::Acquire))
    });
    resources
        .reconcile(
            &GraphDocument::new(3, vec![], vec![]),
            mixrelay_engine_service::PlaybackBindings::Preserve,
        )
        .unwrap();
    resources.activation().unwrap().activate(active);
    resources.reclaim();
    assert!(retired.iter().all(|owner| owner.upgrade().is_none()));
    fixture.command("connect");
    publish_recovery_now(resources, active);
    assert!(active.0.is_empty() && active.1.is_empty());
    assert!(resources.cache.lock().unwrap().is_empty());
    assert!(
        !resources.has_pending(),
        "device arrival resurrected deleted intent"
    );
}

#[test]
fn native_control_recovery_publishes_offline_intent_and_restores_the_same_uid_without_graph_edits()
{
    let (mut fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 256);
    let original = document(&uid, 1, 0.0);
    resources
        .reconcile(
            &original,
            mixrelay_engine_service::PlaybackBindings::Preserve,
        )
        .unwrap();
    let mut active = DeviceSet::default();
    resources.activation().unwrap().activate(&mut active);
    resources.reclaim();
    assert_eq!(active.0.len(), 2);
    let retired = active.0.values().map(Arc::downgrade).collect::<Vec<_>>();
    fixture.command("disconnect");
    wait_until(|| {
        active
            .0
            .values()
            .all(|device| device.failed.load(Ordering::Acquire))
    });
    publish_recovery_now(&resources, &mut active);
    assert_published_offline(&active, &original);
    assert!(retired.iter().all(|owner| owner.upgrade().is_none()));

    let edited = document(&uid, 2, -6.0);
    resources
        .reconcile(&edited, mixrelay_engine_service::PlaybackBindings::Preserve)
        .unwrap();
    resources.activation().unwrap().activate(&mut active);
    resources.reclaim();
    assert_published_offline(&active, &edited);
    assert_cached_selection(&resources, &edited);
    let NodeConfig::AudioDeviceInput(input) = &edited.nodes[0].config else {
        panic!("input fixture");
    };
    assert_eq!(input.gain_db.to_bits(), (-6.0_f32).to_bits());

    fixture.command("connect");
    // Only control recovery executes here: no reconcile or graph edit is allowed
    // to create the returning native owners for this assertion.
    wait_until(|| {
        publish_recovery_now(&resources, &mut active);
        active.0.len() == 2 && active.0.values().all(|device| device.ready())
    });
    assert!(active.1.is_empty());
    assert_cached_selection(&resources, &edited);
    let mut pcm = [0.0; 512];
    wait_until(|| active.read("input", &mut pcm, 0).unwrap().frames > 0);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(active.write("output", &[0.0; 512], 0).unwrap().frames > 0);
    delete_disconnected_nodes(&resources, &mut active, &mut fixture);
}

#[test]
fn native_coreaudio_recovery_retains_graph_owners_and_offline_gain_edits() {
    let (mut fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 256);
    resources
        .reconcile(
            &document(&uid, 1, 0.0),
            mixrelay_engine_service::PlaybackBindings::Preserve,
        )
        .unwrap();
    let mut active = DeviceSet::default();
    resources.activation().unwrap().activate(&mut active);
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
            .reconcile(
                &document(&uid, cycle + 2, -6.0),
                mixrelay_engine_service::PlaybackBindings::Preserve,
            )
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
        wait_until(|| active.read("input", &mut pcm, 0).unwrap().frames > 0);
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        assert!(active.write("output", &[0.0; 512], 0).unwrap().frames > 0);
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
        .reconcile(
            &GraphDocument::new(10, vec![], vec![]),
            mixrelay_engine_service::PlaybackBindings::Preserve,
        )
        .unwrap();
    resources.activation().unwrap().activate(&mut active);
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
        unsafe { mr_device_io_native_frames(active.0["input"].native.pointer.as_ptr()) };
    let overflow_start =
        unsafe { mr_device_io_overflows(active.0["input"].native.pointer.as_ptr()) };
    let epoch_start = active.0["input"].epoch.load(Ordering::Acquire);
    let output_overflow_start =
        unsafe { mr_device_io_overflows(active.0["output"].native.pointer.as_ptr()) };
    let mut pcm = [0.0; 512];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let clock = active.hardware_clock_progress().unwrap();
        let through =
            (clock.native_frames - clock_start) * u64::from(rate) / u64::from(clock.native_rate);
        // Native credit is the clock; sleep only yields CPU, never sets rate.
        while graph_frames + 256 <= through {
            captured += active.read("input", &mut pcm, 0).unwrap().frames;
            submitted += active.write("output", &[0.0; 512], 0).unwrap().frames;
            assert!(pcm.iter().all(|sample| sample.is_finite()));
            graph_frames += 256;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let native_frames =
        unsafe { mr_device_io_native_frames(active.0["input"].native.pointer.as_ptr()) }
            - native_start;
    let overflows = unsafe { mr_device_io_overflows(active.0["input"].native.pointer.as_ptr()) }
        - overflow_start;
    let output_overflows =
        unsafe { mr_device_io_overflows(active.0["output"].native.pointer.as_ptr()) }
            - output_overflow_start;
    let epoch_changed = active.0["input"].epoch.load(Ordering::Acquire) != epoch_start;
    eprintln!(
        "hardware SRC graph={rate} native_frames={native_frames} input_overflows={overflows} output_overflows={output_overflows} epoch_changed={epoch_changed} captured_frames={captured} submitted_frames={submitted} input_ready={:?} output_ready={:?}",
        active.clock_synchronized("input", true),
        active.clock_synchronized("output", false)
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
    assert_eq!(active.clock_synchronized("input", true), Some(true));
    assert_eq!(active.clock_synchronized("output", false), Some(true));
}

#[test]
fn native_coreaudio_all_buffer_sizes_are_applied_and_survive_reconnection() {
    let (mut fixture, uid) = Fixture::start();
    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let resources = MacDevices::new(status, 48_000, 256);
    let graph = document(&uid, 1, 0.0);
    resources
        .reconcile(&graph, mixrelay_engine_service::PlaybackBindings::Preserve)
        .unwrap();
    let mut active = DeviceSet::default();
    resources.activation().unwrap().activate(&mut active);
    resources.reclaim();
    let mut previous = 256;
    for &frames in mixrelay_audio_io::formats::BUFFER_SIZES {
        let mut prepared = resources
            .prepare_format_change(
                &graph,
                mixrelay_engine_service::PlaybackBindings::Preserve,
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
                mr_device_io_quantum(device.native.pointer.as_ptr(), false) == frames
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
    resources
        .reconcile(&graph, mixrelay_engine_service::PlaybackBindings::Preserve)
        .unwrap();
    let mut active = DeviceSet::default();
    resources.activation().unwrap().activate(&mut active);
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
                .prepare_format_change(
                    &graph,
                    mixrelay_engine_service::PlaybackBindings::Preserve,
                    old,
                    next,
                )
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
    assert!(mixrelay_audio_io::formats::SAMPLE_RATES.contains(&native_input_rate));
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
        resources
            .reconcile(&graph, mixrelay_engine_service::PlaybackBindings::Preserve)
            .unwrap_or_else(|error| {
                panic!("initial connection iteration={iteration} graph_rate={rate}: {error}")
            });
        let mut active = DeviceSet::default();
        resources.activation().unwrap().activate(&mut active);
        assert!(active.0.values().all(|device| device.ready()));
    }
}
