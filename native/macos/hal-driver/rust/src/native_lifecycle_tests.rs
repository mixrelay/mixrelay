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
        pcm(&fixture, id, true, 0, 0.75);
        assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.75_f32.to_bits(); 64]);
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
    pcm(&fixture, id, true, 0, 0.25);
    let epoch = transport.clock().epoch;
    super::connect(&fixture, 2);
    assert!(message.apply_from(&fixture, 2));
    assert_eq!(transport.clock().epoch, epoch);
    assert!(crate::device::lock(&fixture.host.requests).is_empty());
    assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.25_f32.to_bits(); 64]);
}

fn pcm(fixture: &super::Fixture, device: u32, render: bool, frame: u64, value: f32) -> [u32; 64] {
    let mut samples = [value; 64];
    assert!(unsafe {
        fixture.registry.io(IoOperation {
            client: 42,
            device,
            stream: device + if render { 2 } else { 1 },
            render,
            frame,
            samples: samples.as_mut_ptr().cast(),
            frames: 32,
        })
    });
    samples.map(f32::to_bits)
}

#[test]
fn callback_evidence_distinguishes_missing_capture_from_published_digital_silence() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Capture evidence", 48_000);
    start(&fixture, id);
    let device = fixture.registry.pin(id).unwrap();
    device.io_trace.set_enabled(true);
    pcm(&fixture, id, true, 0, 0.25);
    assert_eq!(
        pcm(&fixture, id, false, 16, 0.9),
        std::array::from_fn(|index| if index < 32 { 0.25_f32.to_bits() } else { 0 })
    );
    pcm(&fixture, id, true, 64, 0.0);
    assert_eq!(pcm(&fixture, id, false, 64, 0.9), [0; 64]);
    let bytes = device.io_trace.snapshot();
    let words: Vec<_> = bytes
        .chunks_exact(8)
        .map(|value| u64::from_ne_bytes(value.try_into().unwrap()))
        .collect();
    let rows: Vec<_> = words[5..]
        .chunks_exact(9)
        .filter(|row| row[0] != 0)
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(&rows[1][1..7], &[0, 16, 32, 1, 16, 16]);
    assert_eq!(&rows[3][1..7], &[0, 64, 32, 1, 32, 32]);
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
    pcm(&fixture, id, true, 0, 0.5);
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
    let mut next = [0.75_f32; 64];
    assert!(unsafe {
        fixture.registry.io(IoOperation {
            device: id,
            stream: id + 2,
            render: true,
            client: 43,
            frame: 32,
            samples: next.as_mut_ptr().cast(),
            frames: 32,
        })
    });
    assert_eq!(pcm(&fixture, id, false, 32, 0.0), [0.75_f32.to_bits(); 64]);
    assert_eq!(fixture.registry.stop(id, 43), OK);
    assert_eq!(transport.clock().running, 0);
    assert_eq!(
        unsafe { mr_shared_audio_buffer_producer_activity(render) },
        1
    );
    assert_eq!(pcm(&fixture, id, false, 0, 0.9), [0; 64]);
    assert_eq!(fixture.registry.start(id, 42, 3), OK);
    assert_eq!(pcm(&fixture, id, false, 0, 0.9), [0; 64]);
    pcm(&fixture, id, true, 0, 0.25);
    assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.25_f32.to_bits(); 64]);
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
        pcm(&fixture, id, true, 32, 0.75);
        assert_eq!(pcm(&fixture, id, false, 32, 0.0), [0.75_f32.to_bits(); 64]);
        assert_eq!(fixture.registry.set_rate(id, rate), OK);
        assert_eq!(fixture.perform(), OK);
        assert_eq!(
            (transport.clock().sample_rate, transport.clock().running),
            (rate, 1)
        );
        assert!(!unsafe { mr_shared_audio_buffer_epoch_pending(capture) });
        assert_eq!(pcm(&fixture, id, false, 0, 0.9), [0; 64]);
        pcm(&fixture, id, true, 0, 0.25);
        assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.25_f32.to_bits(); 64]);
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
    pcm(&fixture, id, true, 0, 0.75);
    assert_eq!(fixture.registry.stop(id, 42), OK);
    let replacement = Transport::new(10, 2, 2);
    assert_eq!(replacement.add(&fixture, "After", 48_000), id);
    assert_eq!(fixture.perform(), OK);
    assert_eq!(
        (replacement.clock().sample_rate, replacement.clock().running),
        (96_000, 0)
    );
    assert_eq!(old.clock().running, 0);
    assert_eq!(pcm(&fixture, id, false, 0, 0.9), [0; 64]);
    assert_eq!(fixture.registry.start(id, 42, 4), OK);
    assert_eq!(pcm(&fixture, id, false, 0, 0.9), [0; 64]);
    pcm(&fixture, id, true, 0, 0.25);
    assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.25_f32.to_bits(); 64]);
}

#[test]
fn failed_native_attachment_preserves_running_mapping_clock_and_pcm() {
    let fixture = native_fixture();
    let old = Transport::new(10, 2, 2);
    let id = old.add(&fixture, "Still active", 96_000);
    start(&fixture, id);
    pcm(&fixture, id, true, 0, 0.75);
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
    assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.75_f32.to_bits(); 64]);
    pcm(&fixture, id, true, 32, 0.25);
    assert_eq!(pcm(&fixture, id, false, 32, 0.0), [0.25_f32.to_bits(); 64]);
    assert_eq!(
        old.capture::<64>(&fixture, id, 32).map(f32::to_bits),
        [0.75_f32.to_bits(); 64]
    );
}

#[test]
fn native_runtime_mapping_outlives_caller_descriptor_and_mapping_view() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Owned mapping", 96_000);
    start(&fixture, id);
    pcm(&fixture, id, true, 0, 0.25);
    drop(transport);
    assert_eq!(pcm(&fixture, id, false, 0, 0.0), [0.25_f32.to_bits(); 64]);
    pcm(&fixture, id, true, 32, 0.5);
    assert_eq!(pcm(&fixture, id, false, 32, 0.0), [0.5_f32.to_bits(); 64]);
    let mut timestamp = Timestamp::default();
    assert!(fixture.registry.timestamp(id, 100, &mut timestamp));
    assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 96_000);
    assert_eq!(fixture.registry.stop(id, 42), OK);
    assert_eq!(fixture.registry.client(id, 42, false), OK);
    assert_eq!(fixture.registry.remove([10; 16]), OK);
    assert!(fixture.registry.snapshot(id).is_none());
}

#[test]
fn native_removal_releases_running_mapping_while_host_client_retains_device() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Removed while open", 48_000);
    start(&fixture, id);
    pcm(&fixture, id, true, 0, 0.75);
    assert_eq!(transport.clock().running, 1);
    assert_eq!(fixture.registry.remove([10; 16]), OK);
    assert_eq!(transport.clock().running, 0);
    let device = fixture.registry.pin(id).unwrap();
    assert!(
        device
            .runtime
            .load(std::sync::atomic::Ordering::Acquire)
            .is_null()
    );
    drop(device);
    let retired = fixture.registry.snapshot(id).unwrap();
    assert!(!retired.alive);
    assert!(!retired.running);
    assert_eq!(retired.client_count, 1);
    assert_eq!(pcm(&fixture, id, false, 0, 1.0), [0; 64]);
    let mut timestamp = Timestamp::default();
    assert!(!fixture.registry.timestamp(id, 100, &mut timestamp));
    assert_eq!(fixture.registry.stop(id, 42), OK);
    assert_eq!(fixture.registry.client(id, 42, false), OK);
    assert!(fixture.registry.snapshot(id).is_none());
    let replacement = transport.add(&fixture, "Fresh device", 48_000);
    assert_ne!(replacement, id);
    start(&fixture, replacement);
    pcm(&fixture, replacement, true, 0, 0.25);
    assert_eq!(
        pcm(&fixture, replacement, false, 0, 0.0),
        [0.25_f32.to_bits(); 64]
    );
}

/// Frame-indexed stereo words for the 32-frame block at `frame`. Words below 2^13 are
/// exact binary fractions in Float32, distinct per frame and lane.
fn loopback_words(frame: u64) -> [f32; 64] {
    std::array::from_fn(|index| {
        let position = frame + index as u64 / 2;
        let word = f32::from(u16::try_from(position % 8192).unwrap()) / 8192.0;
        if index % 2 == 0 { word } else { -word }
    })
}

/// Device IO through the shared loopback with capture one block behind render, so one
/// rendered block is always buffered unread between cycles. Lost, repeated or reset PCM
/// changes the captured words.
struct Loopback<'a> {
    fixture: &'a super::Fixture,
    device: u32,
    /// Frame of the rendered block that has not been captured yet.
    buffered: u64,
}

impl<'a> Loopback<'a> {
    fn start(fixture: &'a super::Fixture, device: u32) -> Self {
        let loopback = Self {
            fixture,
            device,
            buffered: 0,
        };
        loopback.io(true, 0, &mut loopback_words(0));
        loopback
    }

    /// One IO cycle: verify the buffered block, then render the next one.
    fn cycle(&mut self) {
        let mut captured = [9.0_f32; 64];
        self.io(false, self.buffered, &mut captured);
        assert_eq!(
            captured.map(f32::to_bits),
            loopback_words(self.buffered).map(f32::to_bits),
            "loopback discontinuity at frame {}",
            self.buffered
        );
        self.buffered += 32;
        self.io(true, self.buffered, &mut loopback_words(self.buffered));
    }

    fn io(&self, render: bool, frame: u64, samples: &mut [f32; 64]) {
        // SAFETY: the fixture owns the registry; each block is 32 stereo Float32 frames.
        assert!(unsafe {
            self.fixture.registry.io(IoOperation {
                client: 42,
                device: self.device,
                stream: self.device + if render { 2 } else { 1 },
                render,
                frame,
                samples: samples.as_mut_ptr().cast(),
                frames: 32,
            })
        });
    }
}

#[test]
fn loopback_stays_continuous_while_other_clients_attach_run_and_detach() {
    let fixture = native_fixture();
    let transport = Transport::new(10, 2, 2);
    let id = transport.add(&fixture, "Shared", 48_000);
    start(&fixture, id);
    let epoch = transport.clock().epoch;
    let mut loopback = Loopback::start(&fixture, id);
    let mut cycles = |count: u32| {
        for _ in 0..count {
            loopback.cycle();
        }
    };
    // Every transition below happens while one rendered block is still unread; the next
    // cycle captures that block before rendering again.
    cycles(16);
    // A second client attaches and starts IO on the running device.
    assert_eq!(fixture.registry.client(id, 43, true), OK);
    assert_eq!(fixture.registry.start(id, 43, 2), OK);
    cycles(16);
    // It stops, then detaches, each while the continuing stream has a block unread.
    assert_eq!(fixture.registry.stop(id, 43), OK);
    cycles(1);
    assert_eq!(fixture.registry.client(id, 43, false), OK);
    cycles(16);
    // A client whose process disappears without StopIO releases only its own claim.
    assert_eq!(fixture.registry.client(id, 44, true), OK);
    assert_eq!(fixture.registry.start(id, 44, 3), OK);
    cycles(16);
    assert_eq!(fixture.registry.client(id, 44, false), OK);
    cycles(16);
    let clock = transport.clock();
    assert_eq!(
        (clock.running, clock.epoch),
        (1, epoch),
        "the first client's stream was restarted"
    );
}
