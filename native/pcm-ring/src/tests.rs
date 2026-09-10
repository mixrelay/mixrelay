use super::*;
use std::{
    ptr,
    sync::atomic::Ordering::{Relaxed, SeqCst},
};

#[test]
fn empty_and_failed_initialization_are_clearable_without_allocations() {
    let mut storage = std::mem::MaybeUninit::<PcmRing>::zeroed();
    // SAFETY: C-style zero initialization is the documented empty native owner.
    unsafe {
        tn_pcm_ring_clear(storage.as_mut_ptr());
    }
    for (channels, capacity) in [(0, 4), (65, 4), (1, 3), (1, 65537)] {
        // SAFETY: sized/aligned exclusive cleared storage, no live callbacks.
        unsafe {
            assert!(!tn_pcm_ring_init(storage.as_mut_ptr(), channels, capacity));
            tn_pcm_ring_clear(storage.as_mut_ptr());
        }
    }
    assert!(storage::Storage::new(usize::MAX).is_err());
    // SAFETY: null calls do not access storage.
    unsafe {
        tn_pcm_ring_clear(ptr::null_mut());
        assert!(!tn_pcm_ring_init(ptr::null_mut(), 1, 4));
    }
}

#[test]
fn pcm_bits_wrap_for_non_power_of_two_capacity_and_every_channel_width() {
    for channels in 1..=64 {
        let mut ring = PcmRing::new(channels, 7).unwrap();
        let input: Vec<_> = (0..channels * 3)
            .map(|n| f32::from_bits(0x7fc0_0000 + n))
            .collect();
        let mut output = vec![0.0_f32; input.len()];
        for _ in 0..20 {
            // SAFETY: exclusive test owner, complete disjoint input/output spans.
            unsafe {
                assert_eq!(tn_pcm_ring_write(&raw mut ring, input.as_ptr(), 3), 3);
                assert_eq!(
                    tn_pcm_ring_read_available(&raw mut ring, output.as_mut_ptr(), 3),
                    3
                );
            }
            assert!(
                input
                    .iter()
                    .zip(&output)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
    }
}

#[test]
fn zero_frame_write_acknowledges_a_full_stale_epoch_before_capacity_check() {
    let mut ring = PcmRing::new(1, 4).unwrap();
    let input = [1.0_f32; 2];
    let mut output = [9.0_f32; 2];
    // SAFETY: one sequential producer/consumer with complete buffers.
    unsafe {
        for _ in 0..2 {
            assert_eq!(tn_pcm_ring_write(&raw mut ring, input.as_ptr(), 2), 2);
        }
        tn_pcm_ring_restart(&raw mut ring);
        assert_eq!(tn_pcm_ring_write(&raw mut ring, ptr::null(), 0), 0);
        assert_eq!(
            tn_pcm_ring_read_available(&raw mut ring, output.as_mut_ptr(), 2),
            0
        );
    }
    assert_eq!(ring.stats().read, ring.stats().written);
    assert_eq!(output.map(f32::to_bits), [0; 2]);
    assert_eq!(ring.stats().overflows, 0);
}

#[test]
fn partial_epoch_publication_never_advances_the_consumer_or_spins() {
    let mut ring = PcmRing::new(1, 8).unwrap();
    ring.publication.written.store(4, Relaxed);
    ring.publication.epoch_sequence.store(1, SeqCst);
    ring.publication.epoch_start.store(4, SeqCst);
    let mut output = [9.0_f32; 2];
    // SAFETY: single consumer, initialized owner; writer publication intentionally stalled.
    assert_eq!(
        unsafe { tn_pcm_ring_read_available(&raw mut ring, output.as_mut_ptr(), 2) },
        0
    );
    assert_eq!(output.map(f32::to_bits), [0; 2]);
    assert_eq!(ring.stats().read, 0);
}

#[test]
fn restart_during_a_read_invalidates_the_read_commit_and_does_not_rewind_discard() {
    let ring = PcmRing::new(1, 8).unwrap();
    ring.publication.written.store(4, Relaxed);
    let mut reader = Reader::default();
    let window = ring.reserve_read(&mut reader, 2, false).unwrap();
    ring.publication.requested_epoch.store(1, Relaxed);
    assert!(!ring.finish_read(&mut reader, &window, 2));
    assert!(!reader.primed);
    ring.publication.read.store(4, Relaxed);
    ring.publication.epoch_start.store(2, SeqCst);
    ring.publication.published_epoch.store(1, SeqCst);
    assert!(ring.reserve_read(&mut reader, 2, false).is_none());
    assert_eq!(ring.stats().read, 4);
}

#[test]
fn publication_changes_during_observation_are_rejected_without_cursor_movement() {
    let ring = PcmRing::new(1, 8).unwrap();
    ring.publication.written.store(4, Relaxed);
    let mut writer = 0;
    assert!(
        ring.observed_epoch(|| {
            ring.publication.requested_epoch.store(1, Relaxed);
            assert!(ring.reserve_write(&mut writer, 0).is_none());
        })
        .is_none()
    );
    assert_eq!(ring.stats().read, 0);
    assert_eq!(ring.observed_epoch(|| {}), Some((1, 4)));
}

#[test]
fn actual_read_path_silences_pcm_if_restart_arrives_after_the_copy() {
    let mut ring = PcmRing::new(1, 8).unwrap();
    let input = [1.0_f32; 2];
    let mut output = [9.0_f32; 2];
    // SAFETY: exclusive owner with sized input/output; test requests restart using
    // the same atomic control boundary while the consumer owns its reader cell.
    unsafe {
        assert_eq!(tn_pcm_ring_write(&raw mut ring, input.as_ptr(), 2), 2);
        assert_eq!(
            ffi::read(&raw mut ring, output.as_mut_ptr(), 2, false, |ring| {
                ring.publication
                    .requested_epoch
                    .fetch_add(1, Ordering::Release);
            }),
            0
        );
    }
    assert_eq!(output.map(f32::to_bits), [0; 2]);
    assert_eq!(ring.stats().read, 2);
}

#[test]
fn mapping_failure_returns_an_error_without_publishing_storage() {
    // SAFETY: injected MAP_FAILED owns no memory and must never reach unmap.
    let failure = unsafe {
        storage::Storage::map_with(8, |bytes| {
            assert_eq!(bytes, 32);
            ptr::without_provenance_mut(usize::MAX)
        })
    };
    assert!(failure.is_err());
}

#[test]
fn retirement_returns_mapped_pages_to_the_os_in_an_isolated_process() {
    unsafe extern "C" {
        fn mprotect(address: *mut std::ffi::c_void, length: usize, protection: i32) -> i32;
    }
    const TEST: &str = "tests::retirement_returns_mapped_pages_to_the_os_in_an_isolated_process";
    const MARKER: &str = "TONENARI_PCM_RETIREMENT_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--test-threads=1"])
            .env(MARKER, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    for _ in 0..64 {
        let ring = PcmRing::new(64, 65536).unwrap();
        let address = ring.storage.samples.cast();
        // SAFETY: the owned mapping is page-aligned; retain its existing read/write
        // protection. Unlike mincore on Darwin, mprotect rejects unmapped pages.
        assert_eq!(unsafe { mprotect(address, 1, 1 | 2) }, 0);
        drop(ring);
        // SAFETY: checking this now-unmapped page does not dereference it; the
        // isolated child performs no allocation between drop and this check.
        assert_eq!(unsafe { mprotect(address, 1, 1 | 2) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(12));
    }
}

#[test]
fn callbacks_allocate_nothing_and_repeated_retirement_clears_the_owner() {
    for _ in 0..64 {
        let mut ring = PcmRing::new(64, 65536).unwrap();
        let input = [0.5_f32; 64 * 32];
        let mut output = [0.0_f32; 64 * 32];
        let count = allocation_counter::measure(|| {
            for _ in 0..100 {
                // SAFETY: exclusive native test owner and fully sized caller buffers.
                unsafe {
                    assert_eq!(tn_pcm_ring_write(&raw mut ring, input.as_ptr(), 32), 32);
                    assert_eq!(
                        tn_pcm_ring_read_available(&raw mut ring, output.as_mut_ptr(), 32),
                        32
                    );
                }
            }
        });
        assert_eq!(count.count_total, 0);
        assert_eq!(count.count_current, 0);
        let address = ring.storage.samples;
        // SAFETY: producer/consumer are stopped; native clear must release mapping and be idempotent.
        unsafe {
            tn_pcm_ring_clear(&raw mut ring);
            tn_pcm_ring_clear(&raw mut ring);
        }
        assert!(!address.is_null());
        assert!(ring.storage.samples.is_null());
    }
}
