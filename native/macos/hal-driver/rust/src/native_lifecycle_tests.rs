//! Native lifecycle parity uses the production Rust runtime and actual shared
//! mappings, not the removed singleton driver's globals or fixture aliases.
use mixrelay_macos_transport::shared_audio_buffer::{
    mr_shared_audio_buffer_epoch_pending, mr_shared_audio_buffer_producer_activity,
};

use super::{IoOperation, OK, Transport, native_fixture, start};
use crate::abi::Timestamp;

#[test]
fn new_engine_inventory_replaces_mapping_with_lower_or_equal_generation() {
    for generation in [2, 90] {
        let fixture = native_fixture();
        let old = Transport::new(10, 2, 2);
        let first = super::Message::snapshot(&[(&old, "Device", 48_000, 90)], 90);
        assert!(first.apply(&fixture));
        let id = fixture.registry.devices()[0];
        start(&fixture, id);
        assert_eq!(fixture.registry.set_rate(id, 96_000), OK);
        let (_, stale_request) = fixture.pending();
        let new = Transport::new(10, 2, 2);
        let next = super::Message::snapshot(&[(&new, "Device", 48_000, generation)], generation);
        super::connect(&fixture, 2);
        assert!(next.apply_from(&fixture, 2));
        let (_, replacement_request) = fixture.pending();
        assert!(replacement_request > stale_request);
        // Delayed CoreAudio replies must not consume the new owned attachment.
        assert_eq!(
            fixture
                .registry
                .perform(id, crate::abi::CONFIG_ACTION, stale_request),
            OK
        );
        assert_eq!(
            fixture
                .registry
                .abort(id, crate::abi::CONFIG_ACTION, stale_request),
            OK
        );
        assert_eq!(fixture.perform(), OK);
        assert_eq!(fixture.registry.devices(), vec![id]);
        assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 96_000);
        assert_eq!(
            (
                old.clock().running,
                new.clock().running,
                new.clock().sample_rate
            ),
            (0, 1, 96_000)
        );
        new.capture(&[0.75; 64], 32);
        assert_eq!(pcm(&fixture, id, false, 0.0), [0.75_f32.to_bits(); 64]);
        assert!(!first.apply_from(&fixture, 1));
        assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 96_000);
    }
}

#[test]
fn new_connection_fences_old_messages_before_first_valid_inventory() {
    let fixture = native_fixture();
    let old = Transport::new(10, 2, 2);
    assert!(super::Message::snapshot(&[(&old, "Device", 48_000, 90)], 90).apply(&fixture));
    let id = fixture.registry.devices()[0];
    super::connect(&fixture, 2);
    // A malformed new peer cannot erase devices or reopen the old authority.
    assert!(!super::Message::snapshot(&[(&old, "Device", 12345, 1)], 1).apply_from(&fixture, 2));
    assert!(!super::Message::snapshot(&[], 91).apply_from(&fixture, 1));
    assert_eq!(fixture.registry.devices(), vec![id]);
    // Preserve old identities until a fully validated new list removes them.
    assert!(super::Message::snapshot(&[], 1).apply_from(&fixture, 2));
    assert!(fixture.registry.devices().is_empty());
}

#[test]
fn removal_requires_inventory_from_current_connection() {
    use mixrelay_macos_transport::control_inventory::mr_hal_control_create_endpoint_remove;

    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let inventory = super::Message::snapshot(&[(&transport, "Device", 48_000, 1)], 1);
    assert!(inventory.apply(&fixture));
    let id = fixture.registry.devices()[0];
    let removal = super::Message(unsafe {
        mr_hal_control_create_endpoint_remove(transport.endpoint.as_ptr(), 2)
    });
    assert!(!removal.0.is_null());
    super::connect(&fixture, 2);
    assert!(!removal.apply_from(&fixture, 1));
    assert!(!removal.apply_from(&fixture, 2));
    assert_eq!(fixture.registry.devices(), vec![id]);
    assert!(inventory.apply_from(&fixture, 2));
    assert!(removal.apply_from(&fixture, 2));
    assert!(fixture.registry.devices().is_empty());
}

#[test]
fn same_mapping_on_new_connection_does_not_reset_running_stream() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let message = super::Message::snapshot(&[(&transport, "Device", 48_000, 1)], 1);
    assert!(message.apply(&fixture));
    let id = fixture.registry.devices()[0];
    start(&fixture, id);
    transport.capture(&[0.25; 64], 32);
    let epoch = transport.clock().epoch;
    super::connect(&fixture, 2);
    assert!(message.apply_from(&fixture, 2));
    assert_eq!(transport.clock().epoch, epoch);
    assert!(crate::device::lock(&fixture.host.requests).is_empty());
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.25_f32.to_bits(); 64]);
}

fn pcm(fixture: &super::Fixture, device: u32, render: bool, value: f32) -> [u32; 64] {
    let mut samples = [value; 64];
    assert!(unsafe {
        fixture.registry.io(IoOperation {
            device,
            stream: device + if render { 2 } else { 1 },
            render,
            frame: 0,
            samples: samples.as_mut_ptr(),
            frames: 32,
        })
    });
    samples.map(f32::to_bits)
}

#[test]
fn native_multi_client_activity_and_restart_discard_previous_capture() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Clients", 48_000);
    let render = unsafe { super::mr_endpoint_transport_render_buffer(transport.pointer) };
    start(&fixture, id);
    assert_eq!(
        unsafe { mr_shared_audio_buffer_producer_activity(render) },
        2
    );
    assert_eq!(fixture.registry.client(id, 43, true), OK);
    assert_eq!(fixture.registry.start(id, 43, 2), OK);
    pcm(&fixture, id, true, 0.5);
    assert_eq!(
        unsafe { mr_shared_audio_buffer_producer_activity(render) },
        3
    );
    assert_eq!(fixture.registry.stop(id, 42), OK);
    assert_eq!(
        (transport.clock().running, unsafe {
            mr_shared_audio_buffer_producer_activity(render)
        }),
        (1, 0)
    );
    transport.capture(&[0.75; 64], 32);
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.75_f32.to_bits(); 64]);
    assert_eq!(fixture.registry.stop(id, 43), OK);
    assert_eq!(transport.clock().running, 0);
    assert_eq!(
        unsafe { mr_shared_audio_buffer_producer_activity(render) },
        1
    );
    assert_eq!(pcm(&fixture, id, false, 0.9), [0; 64]);
    assert_eq!(fixture.registry.start(id, 42, 3), OK);
    assert_eq!(pcm(&fixture, id, false, 0.9), [0; 64]);
    transport.capture(&[0.25; 64], 32);
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.25_f32.to_bits(); 64]);
}

#[test]
fn every_native_rate_commit_rearms_capture_before_callbacks_resume() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Rate epochs", 48_000);
    start(&fixture, id);
    let capture = unsafe { super::mr_endpoint_transport_capture_buffer(transport.pointer) };
    let mut previous_seed = 0;
    for &rate in crate::formats::SAMPLE_RATES {
        transport.capture(&[0.75; 64], 32);
        assert_eq!(pcm(&fixture, id, false, 0.0), [0.75_f32.to_bits(); 64]);
        assert_eq!(fixture.registry.set_rate(id, rate), OK);
        assert_eq!(fixture.perform(), OK);
        assert_eq!(
            (transport.clock().sample_rate, transport.clock().running),
            (rate, 1)
        );
        assert!(unsafe { mr_shared_audio_buffer_epoch_pending(capture) });
        assert_eq!(pcm(&fixture, id, false, 0.9), [0; 64]);
        transport.capture(&[0.25; 64], 32);
        assert_eq!(pcm(&fixture, id, false, 0.0), [0.25_f32.to_bits(); 64]);
        let mut timestamp = Timestamp::default();
        assert!(fixture.registry.timestamp(id, 0, &mut timestamp));
        assert!(timestamp.seed > previous_seed);
        previous_seed = timestamp.seed;
    }
}

#[test]
fn idle_replacement_keeps_native_rate_and_does_not_emit_stale_pcm() {
    let fixture = native_fixture();
    let old = Transport::new(10, 2, 2);
    let id = old.add(&fixture, "Before", 96_000);
    start(&fixture, id);
    old.capture(&[0.75; 64], 32);
    assert_eq!(fixture.registry.stop(id, 42), OK);
    let replacement = Transport::new(10, 2, 2);
    assert_eq!(replacement.add(&fixture, "After", 48_000), id);
    assert_eq!(fixture.perform(), OK);
    assert_eq!(
        (replacement.clock().sample_rate, replacement.clock().running),
        (96_000, 0)
    );
    assert_eq!(old.clock().running, 0);
    assert_eq!(pcm(&fixture, id, false, 0.9), [0; 64]);
    assert_eq!(fixture.registry.start(id, 42, 4), OK);
    assert_eq!(pcm(&fixture, id, false, 0.9), [0; 64]);
    replacement.capture(&[0.25; 64], 32);
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.25_f32.to_bits(); 64]);
}

#[test]
fn failed_native_attachment_preserves_running_mapping_clock_and_pcm() {
    let fixture = native_fixture();
    let old = Transport::new(10, 2, 2);
    let id = old.add(&fixture, "Still active", 96_000);
    start(&fixture, id);
    old.capture(&[0.75; 64], 32);
    let before = old.clock();
    let candidate = Transport::new(10, 7, 3);
    assert_eq!(
        candidate.add(&fixture, "Invalidated before commit", 48_000),
        id
    );
    // The descriptor was valid when queued. Invalidate only this test-owned
    // candidate before the real native prepare path; the active mapping is
    // separate and must remain untouched when attach rejects the candidate.
    unsafe { candidate.memory.cast::<u8>().write(0) };
    assert_eq!(fixture.perform(), crate::abi::NATIVE_ERROR);
    let after = old.clock();
    assert_eq!(
        (after.epoch, after.sample_rate, after.running),
        (before.epoch, 96_000, 1)
    );
    assert_eq!(fixture.registry.snapshot(id).unwrap().render_channels, 2);
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.75_f32.to_bits(); 64]);
    pcm(&fixture, id, true, 0.25);
    assert_eq!(
        old.render::<64>(32).map(f32::to_bits),
        [0.25_f32.to_bits(); 64]
    );
}

#[test]
fn native_runtime_mapping_outlives_caller_descriptor_and_producer_view() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Owned mapping", 96_000);
    start(&fixture, id);
    transport.capture(&[0.25; 64], 32);
    drop(transport);
    assert_eq!(pcm(&fixture, id, false, 0.0), [0.25_f32.to_bits(); 64]);
    pcm(&fixture, id, true, 0.5);
    let mut timestamp = Timestamp::default();
    assert!(fixture.registry.timestamp(id, 100, &mut timestamp));
    assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 96_000);
    assert_eq!(fixture.registry.stop(id, 42), OK);
    assert_eq!(fixture.registry.client(id, 42, false), OK);
    assert_eq!(fixture.registry.remove([10; 16]), OK);
    assert!(fixture.registry.snapshot(id).is_none());
}
