use super::ffi::{
    TransportError, TransportErrorCode, mr_application_source_transport_attach,
    mr_application_source_transport_destroy, mr_application_source_transport_write,
};
use super::*;
use crate::{
    application_clock::{mr_application_clock_publish_consumer, mr_application_clock_request_rate},
    create_shared_memory,
    source_control::SourceAttachment,
};
use std::{
    os::fd::{AsRawFd, IntoRawFd},
    ptr,
};

fn fixture() -> (Mapping, SourceTransport) {
    let mapping = Mapping::new(1024, 2).unwrap();
    mapping.clock().producer_lease.store(7, Ordering::Release);
    let epoch = unsafe { mr_shared_audio_buffer_stats(mapping.buffer.as_ptr()) }.epoch;
    let mut attachment = SourceAttachment {
        descriptor: mapping.descriptor.try_clone().unwrap().into_raw_fd(),
        mapped_size: mapping.bytes,
        producer_lease: 7,
        source_epoch: epoch,
    };
    let pointer = unsafe {
        mr_application_source_transport_attach(&raw mut attachment, 1024, 2, ptr::null_mut())
    };
    assert!(!pointer.is_null());
    assert_eq!(attachment.descriptor, -1);
    // Recover the unique native owner without changing the production allocation path.
    (mapping, *unsafe { Box::from_raw(pointer) })
}

#[test]
fn reset_write_snapshot_and_stale_lease_use_the_actual_mapping() {
    let (mapping, mut producer) = fixture();
    let format = unsafe { mr_application_clock_request_rate(mapping.clock.as_ptr(), 44_100) };
    assert!(!producer.write(&[0.25; 256], 128));
    assert!(producer.reset(format, 48_000));
    assert!(producer.write(&[0.25; 256], 128));
    producer.publish_native(256, true);
    unsafe {
        mr_application_clock_publish_consumer(
            mapping.clock.as_ptr(),
            producer.owned_epoch,
            64,
            64,
            true,
        );
    };
    let snapshot = producer.clock().unwrap();
    assert_eq!(
        (snapshot.produced_frames, snapshot.consumed_frames),
        (128, 64)
    );
    assert_eq!(mapping.clock().native_frames.load(Ordering::Acquire), 256);
    mapping.clock().producer_lease.store(8, Ordering::Release);
    assert!(producer.clock().is_none());
    assert!(!producer.reset(format, 48_000));
    assert!(!producer.write(&[0.5; 256], 128));
    drop(producer);
    // A retired producer's destructor must not stop the replacement's native clock.
    assert_eq!(mapping.clock().producer_lease.load(Ordering::Acquire), 8);
    assert_eq!(mapping.clock().native_running.load(Ordering::Acquire), 1);
    assert_eq!(mapping.clock().native_frames.load(Ordering::Acquire), 256);
}

#[test]
fn drop_closes_current_lease_even_while_a_remote_grant_holds_the_guard() {
    let (mapping, producer) = fixture();
    producer.publish_native(128, true);
    let claim = ProducerClaim::acquire(mapping.clock()).unwrap();
    drop(producer);
    assert_eq!(mapping.clock().producer_lease.load(Ordering::Acquire), 0);
    // Without the guard, Drop leaves optional metadata untouched. Readers reject
    // the closed lease regardless, and a successful grant may replace this tuple.
    assert_eq!(mapping.clock().native_running.load(Ordering::Acquire), 1);
    drop(claim);
}

#[test]
fn drop_closes_current_lease_after_epoch_invalidation() {
    let (mapping, producer) = fixture();
    unsafe {
        crate::shared_audio_buffer::mr_shared_audio_buffer_request_epoch(mapping.buffer.as_ptr());
    }
    assert_eq!(
        unsafe { mr_shared_audio_buffer_stats(mapping.buffer.as_ptr()) }.epoch,
        0
    );
    assert!(!producer.owns_stream());
    drop(producer);
    assert_eq!(mapping.clock().producer_lease.load(Ordering::Acquire), 0);
    assert_eq!(
        mapping.clock().acknowledged_format.load(Ordering::Acquire),
        0
    );
}

#[test]
fn reset_exhaustion_keeps_local_cursor_and_releases_producer_claim() {
    let (mapping, mut producer) = fixture();
    for _ in producer.owned_epoch..u64::from(u16::MAX) {
        producer.owned_epoch =
            unsafe { mr_shared_audio_buffer_try_begin_epoch(mapping.buffer.as_ptr()) };
    }
    producer.next_frame = 123;
    let format = unsafe { mr_application_clock_request_rate(mapping.clock.as_ptr(), 48_000) };
    mapping
        .clock()
        .acknowledged_format
        .store(format, Ordering::Release);
    mapping.clock().native_running.store(1, Ordering::Release);
    mapping
        .clock()
        .native_sample_rate
        .store(44_100, Ordering::Release);
    assert!(!producer.reset(format, 48_000));
    assert_eq!(
        (producer.owned_epoch, producer.next_frame),
        (u64::from(u16::MAX), 123)
    );
    assert_eq!(
        mapping.clock().acknowledged_format.load(Ordering::Acquire),
        0
    );
    assert_eq!(mapping.clock().native_running.load(Ordering::Acquire), 0);
    assert_eq!(
        mapping.clock().native_sample_rate.load(Ordering::Acquire),
        44_100
    );
    assert!(ProducerClaim::acquire(mapping.clock()).is_some());
}

#[test]
fn busy_producer_and_racing_consumer_defer_without_mutating_local_state() {
    let (mapping, mut producer) = fixture();
    let format = unsafe { mr_application_clock_request_rate(mapping.clock.as_ptr(), 48_000) };
    let claim = ProducerClaim::acquire(mapping.clock()).unwrap();
    assert!(!producer.reset(format, 48_000));
    assert!(!producer.write(&[0.25; 2], 1));
    producer.publish_native(9, true);
    assert_eq!(producer.next_frame, 0);
    assert_eq!(mapping.clock().native_frames.load(Ordering::Acquire), 0);
    drop(claim);
    assert!(producer.reset(format, 48_000));
    mapping
        .clock()
        .consumer_sequence
        .fetch_add(1, Ordering::AcqRel);
    assert!(producer.clock().is_none());
    mapping
        .clock()
        .consumer_sequence
        .fetch_add(1, Ordering::Release);
    assert!(producer.clock().is_some());
}

#[test]
fn attachment_rejects_short_storage_and_clears_descriptor_on_failure() {
    let descriptor = create_shared_memory(64, true).unwrap();
    let mut attachment = SourceAttachment {
        descriptor: descriptor.into_raw_fd(),
        mapped_size: crate::application_clock::mr_application_clock_required_bytes(1024, 2),
        producer_lease: 7,
        source_epoch: 1,
    };
    let mut error = TransportError::default();
    assert!(
        unsafe {
            mr_application_source_transport_attach(&raw mut attachment, 1024, 2, &raw mut error)
        }
        .is_null()
    );
    assert_eq!(error.code, TransportErrorCode::InvalidAttachment);
    assert_eq!(attachment.descriptor, -1);
    assert_eq!(attachment.mapped_size, 0);
}

#[test]
fn attachment_rejects_wrong_geometry_and_stale_grants() {
    let (mapping, producer) = fixture();
    for (channels, lease, epoch) in [
        (1, 7, producer.owned_epoch),
        (2, 8, producer.owned_epoch),
        (2, 7, 0),
    ] {
        let mut attachment = SourceAttachment {
            descriptor: mapping.descriptor.try_clone().unwrap().into_raw_fd(),
            mapped_size: mapping.bytes,
            producer_lease: lease,
            source_epoch: epoch,
        };
        let mut error = TransportError::default();
        assert!(
            unsafe {
                mr_application_source_transport_attach(
                    &raw mut attachment,
                    1024,
                    channels,
                    &raw mut error,
                )
            }
            .is_null()
        );
        assert_eq!(error.code, TransportErrorCode::InvalidAttachment);
        assert_eq!(attachment.descriptor, -1);
    }
}

#[test]
fn allocation_failure_retires_owner_and_reports_enomem() {
    let (mapping, producer) = fixture();
    producer.publish_native(1, true);
    let mut error = TransportError::default();
    let pointer = unsafe { ffi::publish_owner(Ok(producer), &raw mut error, |_| ptr::null_mut()) };
    assert!(pointer.is_null());
    assert_eq!(error.code, TransportErrorCode::MappingFailed);
    assert_eq!(error.system_error, 12);
    assert_eq!(mapping.clock().native_running.load(Ordering::Acquire), 0);
    assert_eq!(mapping.clock().producer_lease.load(Ordering::Acquire), 0);
}

#[test]
fn callbacks_allocate_nothing_and_reject_invalid_input_extents() {
    let (mapping, mut producer) = fixture();
    let format = unsafe { mr_application_clock_request_rate(mapping.clock.as_ptr(), 48_000) };
    assert!(producer.reset(format, 48_000));
    let input = [0.25; 64];
    let observed = allocation_counter::measure(|| {
        for _ in 0..128 {
            assert!(producer.write(&input, 32));
            producer.publish_native(producer.next_frame, true);
            assert!(producer.clock().is_some());
        }
    });
    assert_eq!(observed.count_total, 0);
    assert!(!producer.write(&input[..1], 1));
    assert!(!unsafe { mr_application_source_transport_write(&raw mut producer, ptr::null(), 1) });
    assert!(!unsafe {
        mr_application_source_transport_write(&raw mut producer, input.as_ptr(), 1025)
    });
    unsafe { mr_application_source_transport_destroy(ptr::null_mut()) };
}

#[test]
fn retirement_unmaps_and_closes_storage_in_an_isolated_process() {
    unsafe extern "C" {
        fn mprotect(address: *mut std::ffi::c_void, length: usize, protection: i32) -> i32;
        fn fcntl(descriptor: i32, command: i32, ...) -> i32;
    }
    const CHILD: &str = "MIXRELAY_SOURCE_RETIREMENT_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "application_source_transport::tests::retirement_unmaps_and_closes_storage_in_an_isolated_process", "--test-threads=1"])
            .env(CHILD, "1").status().unwrap();
        assert!(status.success());
        return;
    }
    // No parallel test may reuse a retired descriptor or virtual address between
    // drop and these OS observations; this child runs only this one fixture.
    let (_mapping, producer) = fixture();
    let address = producer.mapping.buffer.as_ptr().cast();
    let descriptor = producer.mapping.descriptor.as_raw_fd();
    assert_eq!(unsafe { mprotect(address, 1, 3) }, 0);
    assert_ne!(unsafe { fcntl(descriptor, 1) }, -1);
    drop(producer);
    // Darwin mincore does not reject unmapped pages; unchanged RW protection
    // verifies that this first mapped page exists before drop and not afterward.
    assert_eq!(unsafe { mprotect(address, 1, 3) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(12));
    assert_eq!(unsafe { fcntl(descriptor, 1) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(9));

    let owned = create_shared_memory(64, true).unwrap();
    let descriptor = owned.into_raw_fd();
    let mut attachment = SourceAttachment {
        descriptor,
        mapped_size: 64,
        producer_lease: 7,
        source_epoch: 1,
    };
    assert!(
        unsafe {
            mr_application_source_transport_attach(&raw mut attachment, 1024, 2, ptr::null_mut())
        }
        .is_null()
    );
    assert_eq!(unsafe { fcntl(descriptor, 1) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(9));
}
