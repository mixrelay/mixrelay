use super::*;
use crate::shared_audio_buffer::tn_shared_audio_buffer_write;

fn request(id: &str) -> SourceRequest {
    let mut request = SourceRequest {
        capacity_frames: 1024,
        channels: 2,
        transport_version: VERSION,
        ..SourceRequest::default()
    };
    for (target, byte) in request.source_id.iter_mut().zip(id.bytes()) {
        *target = c_char::from_ne_bytes([byte]);
    }
    request
}
fn exhaust(source: &Source) {
    let buffer = source.mapping.buffer.as_ptr();
    let epoch = unsafe { tn_shared_audio_buffer_stats(buffer) }.epoch;
    for expected in epoch + 1..=u64::from(u16::MAX) {
        assert_eq!(
            unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer) },
            expected
        );
    }
    assert_eq!(unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer) }, 0);
    assert!(source.terminal());
}

#[test]
fn replacing_an_active_max_epoch_revokes_old_pcm_before_publication() {
    let registry = SourceRegistry::new();
    let request = request("active-max");
    registry.attach(&request).unwrap();
    let old = registry.read(token(b"active-max"));
    let buffer = old.source().unwrap().mapping.buffer.as_ptr();
    // SAFETY: the read guard pins this test-only mapping and the test is its
    // sole producer. Deliberately stop at active MAX, before a rearm request.
    unsafe {
        for _ in 2..u16::MAX {
            assert_ne!(tn_shared_audio_buffer_try_begin_epoch(buffer), 0);
        }
        assert_eq!(
            tn_shared_audio_buffer_stats(buffer).epoch,
            u64::from(u16::MAX)
        );
        assert!(!tn_shared_audio_buffer_requires_replacement(buffer));
    }
    registry.attach(&request).unwrap();
    unsafe {
        assert!(tn_shared_audio_buffer_requires_replacement(buffer));
        assert!(!tn_shared_audio_buffer_write(
            buffer,
            u64::from(u16::MAX),
            0,
            [0.5; 2].as_ptr(),
            1
        ));
    }
}

#[test]
fn exhausted_source_replaces_mapping_preserves_settings_and_pins_old_reader() {
    let registry = SourceRegistry::new();
    let request = request("application.one");
    let first = registry.attach(&request).unwrap();
    let read = registry.read(token(b"application.one"));
    let old = read.source().unwrap();
    unsafe {
        tn_application_clock_request_rate(old.mapping.clock.as_ptr(), 96_000);
    }
    old.mapping.clock().clock_master.store(1, Ordering::Release);
    exhaust(old);
    let second = registry.attach(&request).unwrap();
    assert!(second.producer_lease > first.producer_lease);
    assert_eq!(second.source_epoch, 2);
    let current = registry.read(token(b"application.one"));
    let current = current.source().unwrap();
    assert_ne!(old.mapping.buffer, current.mapping.buffer);
    assert_eq!(
        current.mapping.clock().format_word.load(Ordering::Acquire) & u64::from(u32::MAX),
        96_000
    );
    assert_eq!(
        current.mapping.clock().clock_master.load(Ordering::Acquire),
        1
    );
    assert_eq!(
        old.mapping.clock().producer_lease.load(Ordering::Acquire),
        0
    );
    assert_eq!(registry.control().owners.len(), 2);
    assert!(!unsafe {
        tn_shared_audio_buffer_write(
            old.mapping.buffer.as_ptr(),
            first.source_epoch,
            0,
            [0.25; 2].as_ptr(),
            1,
        )
    });
    assert!(unsafe {
        tn_shared_audio_buffer_write(
            current.mapping.buffer.as_ptr(),
            second.source_epoch,
            0,
            [0.5; 2].as_ptr(),
            1,
        )
    });
    drop(read);
    // The second read also retains owners until its scope ends; reclamation
    // only occurs on a later control call, never on either reader's Drop.
}

#[test]
fn attachment_descriptor_outlives_registry_and_does_not_alias_other_sources() {
    let registry = SourceRegistry::new();
    let first = registry.attach(&request("one")).unwrap();
    let second = registry.attach(&request("two")).unwrap();
    let first_pointer = registry
        .read(token(b"one"))
        .source()
        .unwrap()
        .mapping
        .buffer;
    let second_pointer = registry
        .read(token(b"two"))
        .source()
        .unwrap()
        .mapping
        .buffer;
    assert_ne!(first_pointer, second_pointer);
    drop(registry);
    let first_file = std::fs::File::from(first.descriptor);
    let second_file = std::fs::File::from(second.descriptor);
    assert!(first_file.metadata().unwrap().len() >= first.mapped_size as u64);
    assert!(second_file.metadata().unwrap().len() >= second.mapped_size as u64);
}

#[test]
fn busy_or_invalid_attachment_preserves_active_lease_and_mapping() {
    let registry = SourceRegistry::new();
    let request = request("one");
    let attachment = registry.attach(&request).unwrap();
    let read = registry.read(token(b"one"));
    let source = read.source().unwrap();
    let producer = source.claim_producer().unwrap();
    assert_eq!(
        registry.attach(&request).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        source
            .mapping
            .clock()
            .producer_lease
            .load(Ordering::Acquire),
        attachment.producer_lease
    );
    drop(producer);
    let invalid = SourceRequest {
        transport_version: 0,
        ..request
    };
    assert!(registry.attach(&invalid).is_err());
    assert_eq!(registry.control().owners.len(), 1);
}

#[test]
fn consumer_waits_for_current_format_ack_then_transfers_actual_pcm() {
    let registry = SourceRegistry::new();
    let attachment = registry.attach(&request("one")).unwrap();
    let read = registry.read(token(b"one"));
    let source = read.source().unwrap();
    let mut output = [9.0; 256];
    assert_eq!(
        registry.read_audio(source.token, &mut output, 128, 44_100),
        0
    );
    let clock = source.mapping.clock();
    clock
        .acknowledged_format
        .store(clock.format_word.load(Ordering::Acquire), Ordering::Release);
    clock.native_sample_rate.store(48_000, Ordering::Release);
    clock.native_running.store(1, Ordering::Release);
    assert!(unsafe {
        tn_shared_audio_buffer_write(
            source.mapping.buffer.as_ptr(),
            attachment.source_epoch,
            0,
            [0.25; 1536].as_ptr(),
            768,
        )
    });
    assert_eq!(
        registry.read_audio(source.token, &mut output, 128, 44_100),
        128
    );
    assert!(
        output
            .iter()
            .all(|sample| sample.to_bits() == 0.25_f32.to_bits())
    );
    let snapshot = registry.clock(source.token).unwrap();
    assert_eq!(
        snapshot.epoch,
        (attachment.producer_lease << 16) | attachment.source_epoch
    );
    assert_eq!(snapshot.graph_rate, 44_100);
    assert_eq!(snapshot.native_rate, 48_000);
    assert_eq!(
        registry.read_audio(source.token, &mut output, 128, 96_000),
        0
    );
    assert!(registry.clock(source.token).is_none());
}
