//! Cold first admission, reusable storage and cancellation at the endpoint control boundary.
mod support;
use super::*;
use std::{cell::Cell, rc::Rc};
use support::*;

fn assert_cold_first_admission(rate: u32) {
    let fixture = Fixture::new(rate, 2);
    fixture.prepare();
    fixture.admit();
    // A delay left in admission lets HAL write past its 65,536-frame queue.
    fixture.play(rate * 4);
    fixture.verify();
    assert_eq!(fixture.native.borrow().allocations, 1);
}

#[test]
fn cold_first_admission_keeps_native_sample_zero_at_48k() {
    assert_cold_first_admission(48_000);
}

#[test]
fn cold_first_admission_keeps_native_sample_zero_at_192k() {
    assert_cold_first_admission(192_000);
}

#[test]
fn empty_client_snapshot_reuses_storage_and_re_admits_from_the_new_sample_zero() {
    let fixture = Fixture::new(192_000, 2);
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
    fixture
        .owner
        .replace(Vec::new(), host_time(), host_time())
        .unwrap();
    fixture.update();
    assert_eq!(
        fixture
            .owner
            .sources
            .pcm_progress(token())
            .unwrap()
            .clock
            .running,
        0
    );
    fixture.native.borrow_mut().running = false;
    fixture.prepare();
    let admission = allocation_counter::measure(|| fixture.admit());
    // Queue handoff owns control metadata, never another retention-sized PCM store.
    assert!(
        admission.bytes_total < u64::from(QUEUE_FRAMES) * 4,
        "admission allocated {} bytes",
        admission.bytes_total
    );
    fixture.play(1_024);
    fixture.verify();
    assert_eq!(fixture.native.borrow().allocations, 1);
}

#[test]
fn removed_endpoint_releases_its_source_and_re_admits_without_old_pcm() {
    let fixture = Fixture::new(48_000, 2);
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.native.borrow_mut().running = false;
    fixture.owner.configure(&[], host_time()).unwrap();
    assert!(fixture.owner.endpoints.lock().is_empty());
    assert!(!fixture.owner.sources.set_consumer_route(
        token(),
        mixrelay_macos_transport::application_clock::ConsumerRoute::Unrouted
    ));
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
    assert_eq!(fixture.native.borrow().allocations, 2);
}

#[test]
fn clear_during_source_preparation_cannot_publish_a_stale_owner() {
    let fixture = Fixture::new(48_000, 2);
    let weak = Rc::downgrade(&fixture.owner);
    on_storage(move || {
        weak.upgrade().unwrap().clear();
        Ok(())
    });
    assert!(matches!(
        fixture.owner.configure(&[shape(2)], host_time()),
        Err(Error::Stale)
    ));
    assert!(fixture.owner.endpoints.lock().is_empty());
    assert!(!fixture.owner.sources.set_consumer_route(
        token(),
        mixrelay_macos_transport::application_clock::ConsumerRoute::Unrouted
    ));
    let native = Rc::clone(&fixture.native);
    on_storage(move || {
        native.borrow_mut().slow_storage();
        Ok(())
    });
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
}

#[test]
fn geometry_change_prepares_new_storage_and_ignores_an_older_inventory() {
    let fixture = Fixture::new(192_000, 2);
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    let stale = host_time();
    let old_queues = fixture.native.borrow().queues();
    *fixture.native.borrow_mut() = Native::new(192_000, 4);
    fixture.owner.configure(&[shape(4)], host_time()).unwrap();
    fixture.owner.configure(&[shape(2)], stale).unwrap();
    fixture
        .owner
        .replace(old_queues, stale, host_time())
        .unwrap();
    assert_eq!(fixture.owner.endpoints.lock()[&[1; 16]].geometry, shape(4));
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
    assert_eq!(fixture.native.borrow().allocations, 1);
}

#[test]
fn allocation_failure_leaves_no_owner_and_the_next_inventory_can_retry() {
    let fixture = Fixture::new(48_000, 2);
    let attempts = Rc::new(Cell::new(0));
    let observed = Rc::clone(&attempts);
    on_storage(move || {
        observed.set(observed.get() + 1);
        if observed.get() == 1 {
            Err(std::io::ErrorKind::OutOfMemory.into())
        } else {
            Ok(())
        }
    });
    let result = fixture.owner.configure(&[shape(2)], host_time());
    assert!(result.unwrap_err().retryable());
    assert!(fixture.owner.endpoints.lock().is_empty());
    assert!(!fixture.owner.sources.set_consumer_route(
        token(),
        mixrelay_macos_transport::application_clock::ConsumerRoute::Unrouted
    ));
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
    assert_eq!(attempts.get(), 2);
}

#[test]
fn worker_start_failure_reclaims_the_prepared_source_and_backlog_before_retry() {
    let fixture = Fixture::new(48_000, 2);
    fail_worker();
    assert!(
        fixture
            .owner
            .configure(&[shape(2)], host_time())
            .unwrap_err()
            .retryable()
    );
    assert!(fixture.owner.endpoints.lock().is_empty());
    assert!(!fixture.owner.sources.set_consumer_route(
        token(),
        mixrelay_macos_transport::application_clock::ConsumerRoute::Unrouted
    ));
    fixture.prepare();
    fixture.admit();
    fixture.play(1_024);
    fixture.verify();
    assert_eq!(fixture.native.borrow().allocations, 2);
}

#[test]
fn clear_wakes_and_joins_a_prepared_worker_with_no_client_queues() {
    let owner = VirtualPcm::<ThreadWorker>::with_sources(SourceRegistry::with_clock_and_storage(
        Box::new(FixedClock(std::time::Instant::now())),
        test_storage::create,
    ));
    owner.configure(&[shape(2)], host_time()).unwrap();
    owner.clear();
    assert!(owner.endpoints.lock().is_empty());
    assert!(!owner.sources.set_consumer_route(
        token(),
        mixrelay_macos_transport::application_clock::ConsumerRoute::Unrouted
    ));
}
struct FixedClock(std::time::Instant);
impl mixrelay_macos_transport::source_registry::SchedulingClock for FixedClock {
    fn now(&self) -> std::time::Instant {
        self.0
    }
}

#[test]
fn current_geometry_grants_received_before_inventory_preparation_still_start_the_pump() {
    let fixture = Fixture::new(192_000, 2);
    let snapshot_at = host_time();
    fixture.owner.configure(&[shape(2)], host_time()).unwrap();
    fixture.admit_at(snapshot_at);
    fixture.play(1_024);
    fixture.verify();
}

#[test]
fn prepared_queue_handoff_does_not_wait_for_another_inventory_preparation() {
    let fixture = Fixture::new(192_000, 2);
    fixture.prepare();
    fixture.admit();
    let _preparing = fixture.owner.preparing.lock();
    fixture
        .owner
        .replace_ready(fixture.native.borrow().queues(), host_time(), host_time())
        .unwrap();
    fixture.update();
    fixture.play(1_024);
    fixture.verify();
}

#[test]
fn unprepared_first_grant_prepares_and_admits_through_the_inventory_fallback() {
    let fixture = Fixture::new(192_000, 2);
    // No publication arrived before IO started. Keep the grant through a failed
    // reservation and retry it without requiring another HAL message.
    on_storage(|| Err(std::io::ErrorKind::OutOfMemory.into()));
    fixture.native.borrow_mut().start();
    fixture.native.borrow_mut().write();
    let observed = host_time();
    let mut pending = fixture
        .owner
        .replace_ready(fixture.native.borrow().queues(), observed, observed)
        .unwrap();
    assert!(fixture.owner.endpoints.lock().is_empty());
    assert!(
        fixture
            .owner
            .configure_pending(&[shape(2)], &pending, observed)
            .unwrap_err()
            .retryable()
    );
    assert_eq!(pending.queues.len(), 1);
    on_storage(|| Ok(()));
    fixture
        .owner
        .configure_pending(&[shape(2)], &pending, observed)
        .unwrap();
    fixture
        .owner
        .replace_pending(&mut pending, observed, observed)
        .unwrap();
    assert!(pending.queues.is_empty());
    fixture.activate();
    fixture.play(1_024);
    fixture.verify();
}
