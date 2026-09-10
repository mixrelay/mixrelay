use super::*;
use crate::endpoint_registry::{EndpointId, EndpointSpec};
use mixrelay_macos_transport::{
    endpoint_transport::{
        mr_endpoint_transport_begin_clock_transition, mr_endpoint_transport_clock_state,
        mr_endpoint_transport_incarnation,
    },
    shared_audio_buffer::{
        mr_shared_audio_buffer_epoch_pending, mr_shared_audio_buffer_stats,
        mr_shared_audio_buffer_try_begin_epoch, mr_shared_audio_buffer_write,
    },
};
use std::io;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64};

const TEST_ID: EndpointId = [7; 16];

fn initialized<'a>(registry: &'a EndpointRegistry, specs: &[EndpointSpec]) -> Runtime<'a> {
    let runtime = Runtime::new(registry);
    assert!(runtime.snapshot().unwrap().entries().is_empty());
    runtime.prepare_inventory(specs).unwrap();
    publish(&runtime);
    runtime
}
fn publish(runtime: &Runtime<'_>) {
    assert!(runtime.activate());
    assert!(runtime.tick().unwrap());
}
fn publish_recovery(runtime: &Runtime<'_>) {
    assert!(
        !runtime.activate(),
        "a graph activation must not steal a native recovery"
    );
    assert!(runtime.activate_recovery());
    assert!(runtime.tick().unwrap());
}
fn spec(name: &str) -> EndpointSpec {
    EndpointSpec {
        id: TEST_ID,
        display_name: name.into(),
        sample_rate: 96_000,
        capacity_frames: 64,
        render_channels: 2,
        capture_channels: 2,
    }
}
fn ready(runtime: &Runtime<'_>, value: f32) -> EndpointSnapshot {
    ready_id(runtime, TEST_ID, value)
}
fn ready_id(runtime: &Runtime<'_>, id: EndpointId, value: f32) -> EndpointSnapshot {
    let read = runtime.read();
    let endpoint = read.endpoint(id).unwrap();
    assert!(unsafe { mr_endpoint_transport_begin_clock_transition(endpoint.transport()) });
    unsafe { mr_endpoint_transport_clock_state(endpoint.transport(), 17, 96_000, true) };
    assert!(unsafe {
        mr_shared_audio_buffer_write(
            endpoint.render_buffer(),
            mr_shared_audio_buffer_stats(endpoint.render_buffer()).epoch,
            0,
            [value; 8].as_ptr(),
            4,
        )
    });
    audio::snapshot(endpoint).unwrap()
}

#[test]
fn staged_audio_ack_is_atomic_and_inventory_commit_stays_on_control() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("initial")]);
    let before = runtime.snapshot().unwrap().as_abi().generation;
    let old = runtime.read();
    runtime.prepare_inventory(&[spec("next")]).unwrap();
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "initial"
    );
    assert!(runtime.activate());
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "next"
    );
    assert_eq!(runtime.snapshot().unwrap().as_abi().generation, before);
    assert!(runtime.prepare_inventory(&[spec("must wait")]).is_err());
    assert!(runtime.tick().unwrap());
    assert_eq!(runtime.snapshot().unwrap().as_abi().generation, before + 1);
    assert_eq!(
        old.endpoint(TEST_ID).unwrap().spec().display_name,
        "initial"
    );
    drop(old);
    assert!(!runtime.tick().unwrap());
}

#[test]
fn stalled_reader_bounds_retirement_and_cancelled_pins_are_reclaimed() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("initial")]);
    let stalled = runtime.read();
    for _ in 0..1000 {
        runtime.prepare_inventory(&[spec("cancelled")]).unwrap();
        runtime.cancel();
    }
    for _ in 0..63 {
        runtime.prepare_inventory(&[spec("replacement")]).unwrap();
        assert!(runtime.activate());
        assert!(runtime.tick().unwrap());
    }
    assert_eq!(
        runtime
            .prepare_inventory(&[spec("blocked")])
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        stalled.endpoint(TEST_ID).unwrap().spec().display_name,
        "initial"
    );
    drop(stalled);
    runtime.tick().unwrap();
    runtime.prepare_inventory(&[spec("resumed")]).unwrap();
    runtime.cancel();
}

#[test]
fn disabled_direction_never_reads_or_writes_its_transport_placeholder() {
    for capture_disabled in [false, true] {
        let registry = EndpointRegistry::new();
        let mut configuration = spec("one direction");
        if capture_disabled {
            configuration.capture_channels = 0;
        } else {
            configuration.render_channels = 0;
        }
        let runtime = initialized(&registry, &[configuration]);
        let read = runtime.read();
        let endpoint = read.endpoint(TEST_ID).unwrap();
        assert!(unsafe { mr_endpoint_transport_begin_clock_transition(endpoint.transport()) });
        unsafe { mr_endpoint_transport_clock_state(endpoint.transport(), 17, 96_000, true) };
        let expected = audio::snapshot(endpoint).unwrap();
        let mut sentinel = [9.0_f32];
        assert_eq!(
            audio::read_for(Some(endpoint), &expected, 0, &mut sentinel, 0),
            0
        );
        assert!(!audio::write_for(endpoint, &expected, 0, &sentinel, 0));
        assert_eq!(sentinel[0].to_bits(), 9.0_f32.to_bits());
    }
}

#[test]
fn thousand_cancelled_preparations_preserve_concurrent_pcm_and_identity() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("active")]);
    let expected = ready(&runtime, 0.25);
    let stop = AtomicBool::new(false);
    let blocks = AtomicU64::new(0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut capture_frame = 0;
            while !stop.load(Ordering::Acquire) {
                let pin = runtime.read();
                let endpoint = pin.endpoint(TEST_ID).unwrap();
                let mut samples = [0.0; 8];
                assert_eq!(
                    audio::read_for(Some(endpoint), &expected, 0, &mut samples, 2),
                    4
                );
                assert_eq!(samples.map(f32::to_bits), [0.25_f32; 8].map(f32::to_bits));
                assert!(audio::write_for(
                    endpoint,
                    &expected,
                    capture_frame,
                    &[0.5; 8],
                    2
                ));
                capture_frame += 4;
                blocks.fetch_add(1, Ordering::Release);
            }
        });
        for width in [2, 8, 16, 32, 64].into_iter().cycle().take(1000) {
            runtime
                .prepare_inventory(&[EndpointSpec {
                    render_channels: width,
                    ..spec("provisional")
                }])
                .unwrap();
            assert!(
                runtime
                    .prepare_inventory(&[spec("must not replace")])
                    .is_err()
            );
            runtime.cancel();
            runtime.cancel();
        }
        stop.store(true, Ordering::Release);
    });
    assert!(blocks.load(Ordering::Acquire) > 0);
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "active"
    );
    assert_eq!(runtime.snapshot().unwrap().entries().len(), 1);
}

#[test]
fn replacement_request_is_generation_checked_and_preserves_every_device() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("default")]);
    let other = EndpointSpec {
        id: [9; 16],
        display_name: "Seven channels".into(),
        render_channels: 7,
        capture_channels: 64,
        ..spec("unused")
    };
    runtime
        .prepare_inventory(&[spec("default"), other.clone()])
        .unwrap();
    publish(&runtime);
    let snapshot = runtime.snapshot().unwrap();
    let generation = snapshot
        .entries()
        .iter()
        .find(|entry| entry.endpoint_id == other.id)
        .unwrap()
        .generation;
    assert!(!runtime.replace(other.id, generation + 1).unwrap());
    assert!(runtime.replace(other.id, generation).unwrap());
    assert!(runtime.replace(other.id, generation).is_err());
    publish_recovery(&runtime);
    assert!(!runtime.replace(other.id, generation).unwrap());
    assert_eq!(registry.callback(other.id).unwrap().spec(), &other);
    assert_eq!(runtime.snapshot().unwrap().entries().len(), 2);
}

#[test]
fn capture_max_recovery_replaces_real_mapping_not_only_status_flag() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("seven")]);
    let old = runtime.read();
    let endpoint = old.endpoint(TEST_ID).unwrap();
    let mut identity = [0; 16];
    assert!(unsafe {
        mr_endpoint_transport_incarnation(endpoint.transport(), identity.as_mut_ptr())
    });
    for _ in 1..u16::MAX {
        assert_ne!(
            unsafe { mr_shared_audio_buffer_try_begin_epoch(endpoint.capture_buffer()) },
            0
        );
    }
    assert_eq!(
        unsafe { mr_shared_audio_buffer_try_begin_epoch(endpoint.capture_buffer()) },
        0
    );
    assert!(unsafe { mr_shared_audio_buffer_epoch_pending(endpoint.capture_buffer()) });
    assert_eq!(
        unsafe { mr_shared_audio_buffer_stats(endpoint.capture_buffer()) }.epoch,
        0
    );
    assert!(runtime.recover_capture(TEST_ID).unwrap());
    publish_recovery(&runtime);
    let new = runtime.read();
    let new = new.endpoint(TEST_ID).unwrap();
    let mut replaced = [0; 16];
    assert!(unsafe { mr_endpoint_transport_incarnation(new.transport(), replaced.as_mut_ptr()) });
    assert_ne!(identity, replaced);
    assert_eq!(new.spec(), endpoint.spec());
    assert_eq!(
        unsafe { mr_shared_audio_buffer_stats(new.capture_buffer()) }.epoch,
        1
    );
    assert_eq!(
        unsafe { mr_shared_audio_buffer_try_begin_epoch(endpoint.capture_buffer()) },
        0
    );
}

#[test]
fn snapshot_to_pcm_replacement_rejects_old_identity_with_equal_epoch_and_coordinates() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("active")]);
    let expected = ready(&runtime, 0.25);
    let generation = runtime.snapshot().unwrap().entries()[0].generation;
    assert!(runtime.replace(TEST_ID, generation).unwrap());
    publish_recovery(&runtime);
    let current = ready(&runtime, 0.75);
    assert_eq!(expected.clock.epoch, current.clock.epoch);
    assert_eq!(expected.render_epoch, current.render_epoch);
    assert_ne!(expected.incarnation, current.incarnation);
    let pin = runtime.read();
    let endpoint = pin.endpoint(TEST_ID).unwrap();
    let mut samples = [9.0; 8];
    assert_eq!(
        audio::read_for(Some(endpoint), &expected, 0, &mut samples, 2),
        0
    );
    assert_eq!(samples.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    assert!(!audio::write_for(endpoint, &expected, 0, &samples, 2));
    assert!(!audio::begin_for(endpoint, &expected));
    assert_eq!(
        unsafe { mr_shared_audio_buffer_stats(endpoint.capture_buffer()) }.epoch,
        current.capture_epoch
    );
    assert_eq!(
        audio::read_for(Some(endpoint), &current, 0, &mut samples, 2),
        4
    );
    assert_eq!(samples.map(f32::to_bits), [0.75_f32; 8].map(f32::to_bits));
    assert!(audio::write_for(endpoint, &current, 0, &samples, 2));
}

#[test]
fn same_mapping_remote_render_restart_and_rate_change_reject_stale_pcm() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("active")]);
    let expected = ready(&runtime, 0.25);
    let pin = runtime.read();
    let endpoint = pin.endpoint(TEST_ID).unwrap();
    assert_ne!(
        unsafe { mr_shared_audio_buffer_try_begin_epoch(endpoint.render_buffer()) },
        0
    );
    assert!(unsafe {
        mr_shared_audio_buffer_write(
            endpoint.render_buffer(),
            mr_shared_audio_buffer_stats(endpoint.render_buffer()).epoch,
            0,
            [0.75; 8].as_ptr(),
            4,
        )
    });
    let mut samples = [9.0; 8];
    assert_eq!(
        audio::read_for(Some(endpoint), &expected, 0, &mut samples, 2),
        0
    );
    assert_eq!(samples.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    assert!(unsafe { mr_endpoint_transport_begin_clock_transition(endpoint.transport()) });
    unsafe { mr_endpoint_transport_clock_state(endpoint.transport(), 17, 48_000, true) };
    assert!(!audio::write_for(endpoint, &expected, 0, &samples, 2));
    assert!(!audio::begin_for(endpoint, &expected));
}

#[test]
fn safe_pcm_access_rejects_empty_partial_and_mismatched_channel_extents() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("slice validation")]);
    let expected = ready(&runtime, 0.25);
    let pin = runtime.read();
    let endpoint = pin.endpoint(TEST_ID).unwrap();
    let before = unsafe { mr_shared_audio_buffer_stats(endpoint.capture_buffer()) };
    let mut storage = [9.0_f32; 8];
    for (count, channels) in [(0, 2), (3, 2), (8, 1), (8, 65)] {
        assert_eq!(
            audio::read_for(
                Some(endpoint),
                &expected,
                0,
                &mut storage[..count],
                channels
            ),
            0
        );
        assert!(!audio::write_for(
            endpoint,
            &expected,
            0,
            &storage[..count],
            channels
        ));
    }
    let after = unsafe { mr_shared_audio_buffer_stats(endpoint.capture_buffer()) };
    assert_eq!(before.epoch, after.epoch);
    assert_eq!(
        before.published_through_frame,
        after.published_through_frame
    );
}

#[test]
fn safe_service_boundary_rejects_unknown_identity() {
    let mut samples = [9.0_f32; 8];
    let expected = EndpointSnapshot {
        endpoint_id: [99; 16],
        ..Default::default()
    };
    assert_eq!(read_render(&expected, 0, &mut samples, 2), 0);
    assert_eq!(samples.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    assert!(!write_capture(&expected, 0, &samples, 2));
    assert!(rearm_capture(&expected).is_none());
    assert!(capture_pending(&expected));
    assert!(snapshot(expected.endpoint_id).is_none());
}

#[test]
fn empty_startup_and_final_reference_removal_publish_no_synthetic_device() {
    let registry = EndpointRegistry::new();
    let runtime = Runtime::new(&registry);
    assert!(runtime.read().specs().is_empty());
    assert!(runtime.snapshot().unwrap().entries().is_empty());
    runtime
        .prepare_inventory(&[spec("Owned by a graph")])
        .unwrap();
    publish(&runtime);
    let old = runtime.read();
    runtime.prepare_inventory(&[]).unwrap();
    publish(&runtime);
    assert!(runtime.read().specs().is_empty());
    assert!(runtime.snapshot().unwrap().entries().is_empty());
    assert_eq!(
        old.endpoint(TEST_ID).unwrap().spec().display_name,
        "Owned by a graph"
    );
}

#[test]
fn two_devices_are_pcm_isolated_and_removing_one_retains_other_identity() {
    let registry = EndpointRegistry::new();
    let other = EndpointSpec {
        id: [8; 16],
        ..spec("Second")
    };
    let runtime = initialized(&registry, &[spec("First"), other.clone()]);
    let first_snapshot = ready_id(&runtime, TEST_ID, 0.25);
    let second_snapshot = ready_id(&runtime, other.id, 0.75);
    {
        let read = runtime.read();
        let first = read.endpoint(TEST_ID).unwrap();
        let second = read.endpoint(other.id).unwrap();
        let mut pcm = [0.0; 8];
        assert_eq!(
            audio::read_for(Some(second), &first_snapshot, 0, &mut pcm, 2),
            0
        );
        assert!(!audio::write_for(first, &second_snapshot, 0, &pcm, 2));
        assert_eq!(
            audio::read_for(Some(second), &second_snapshot, 0, &mut pcm, 2),
            4
        );
        assert_eq!(pcm.map(f32::to_bits), [0.75_f32; 8].map(f32::to_bits));
    }
    runtime
        .prepare_inventory(std::slice::from_ref(&other))
        .unwrap();
    publish(&runtime);
    let read = runtime.read();
    assert!(read.endpoint(TEST_ID).is_none());
    let retained = read.endpoint(other.id).unwrap();
    let current = audio::snapshot(retained).unwrap();
    assert_eq!(current.incarnation, second_snapshot.incarnation);
    let mut pcm = [0.0; 8];
    assert_eq!(
        audio::read_for(Some(retained), &second_snapshot, 0, &mut pcm, 2),
        4
    );
    assert_eq!(pcm.map(f32::to_bits), [0.75_f32; 8].map(f32::to_bits));
}

#[test]
fn inventory_cancel_and_targeted_reconnect_preserve_unaffected_live_pcm() {
    let registry = EndpointRegistry::new();
    let other = EndpointSpec {
        id: [8; 16],
        ..spec("Second")
    };
    let runtime = initialized(&registry, &[spec("First"), other.clone()]);
    let first = ready_id(&runtime, TEST_ID, 0.25);
    let second = ready_id(&runtime, other.id, 0.75);
    runtime.prepare_inventory(&[]).unwrap();
    runtime.cancel();
    assert_eq!(runtime.read().specs().len(), 2);
    let generation = runtime.read().endpoint(TEST_ID).unwrap().generation();
    assert!(runtime.replace(TEST_ID, generation).unwrap());
    publish_recovery(&runtime);
    let replaced = ready_id(&runtime, TEST_ID, 0.5);
    assert_ne!(replaced.incarnation, first.incarnation);
    let read = runtime.read();
    let retained = read.endpoint(other.id).unwrap();
    assert_eq!(
        audio::snapshot(retained).unwrap().incarnation,
        second.incarnation
    );
    let mut pcm = [0.0; 8];
    assert_eq!(audio::read_for(Some(retained), &second, 0, &mut pcm, 2), 4);
    assert_eq!(pcm.map(f32::to_bits), [0.75_f32; 8].map(f32::to_bits));
}

#[test]
fn recovery_publication_cannot_steal_graph_preparation_or_its_cancel() {
    let registry = EndpointRegistry::new();
    let runtime = initialized(&registry, &[spec("Original")]);
    runtime
        .prepare_inventory(&[spec("Graph prepared")])
        .unwrap();
    assert!(!runtime.activate_recovery());
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "Original"
    );
    runtime.cancel();
    assert!(!runtime.activate());
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "Original"
    );
    let generation = runtime.read().endpoint(TEST_ID).unwrap().generation();
    assert!(runtime.replace(TEST_ID, generation).unwrap());
    runtime.cancel();
    publish_recovery(&runtime);
    assert_eq!(
        runtime
            .read()
            .endpoint(TEST_ID)
            .unwrap()
            .spec()
            .display_name,
        "Original"
    );
}
