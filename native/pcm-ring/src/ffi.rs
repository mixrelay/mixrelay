use super::{PcmRing, RingStats};
use std::{
    ptr,
    sync::atomic::Ordering::{Acquire, Release},
};

/// Initializes exclusive native storage; invalid geometry/mapping failure returns
/// false with an empty, clearable ring. Call only on fresh or cleared storage.
/// # Safety
/// Non-null ring is 120 writable bytes aligned to 8. The native stream owns it
/// exclusively before IO registration; it contains no live mapping to overwrite.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_init(
    ring: *mut PcmRing,
    channels: u32,
    capacity: u32,
) -> bool {
    if ring.is_null() {
        return false;
    }
    let prepared = PcmRing::new(channels, capacity);
    let success = prepared.is_some();
    // SAFETY: fresh/cleared sized storage; failed setup is also safe to clear.
    unsafe {
        ring.write(prepared.unwrap_or_default());
    }
    success
}

/// Unmaps PCM and returns storage to the empty state. Null/zero storage is harmless.
/// # Safety
/// Native stream owns initialized or zeroed storage. Producer, consumer and stat
/// readers must have stopped; no Rust borrow may survive this control-side clear.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_clear(ring: *mut PcmRing) {
    // SAFETY: native zero initialization is a valid empty value for every field.
    if let Some(ring) = unsafe { ring.as_mut() } {
        *ring = PcmRing::default();
    }
}

/// Invalidates the current epoch without waiting for either realtime owner.
/// # Safety
/// Ring remains initialized/live under native stream ownership throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_restart(ring: *mut PcmRing) {
    // SAFETY: retained owner; only the atomic control request is mutated.
    if let Some(ring) = unsafe { ring.as_ref() } {
        ring.publication.requested_epoch.fetch_add(1, Release);
    }
}

/// Consumer-only discard of published PCM; producer cursor is never changed.
/// # Safety
/// The native consumer exclusively owns its private reader cell, with the ring
/// retained. No other consumer/read/discard may overlap; producer may continue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_discard(ring: *mut PcmRing) {
    // SAFETY: initialized ring retained by the native stream.
    if let Some(ring) = unsafe { ring.as_ref() } {
        ring.publication
            .read
            .store(ring.publication.written.load(Acquire), Release);
        // SAFETY: only this serialized consumer touches its reader cell.
        unsafe {
            (*ring.reader.get()).primed = false;
        }
    }
}

/// Writes a whole block, retaining unread PCM on overflow. Zero frames still ACK
/// a pending epoch. Nonzero input must be non-null; null input returns zero.
/// # Safety
/// Retained initialized ring, one serialized producer. Accepted input contains
/// frames*channels readable floats disjoint from the mapped ring allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_write(
    ring: *mut PcmRing,
    input: *const f32,
    frames: u32,
) -> u32 {
    // SAFETY: native stream retains storage; writer mutation is inside its cell.
    let Some(ring) = (unsafe { ring.as_ref() }) else {
        return 0;
    };
    if ring.storage.samples.is_null() || (frames != 0 && input.is_null()) {
        return 0;
    }
    // SAFETY: this native callback/worker is the sole producer for this ring.
    let writer = unsafe { &mut *ring.writer_epoch.get() };
    let Some(cursor) = ring.reserve_write(writer, frames) else {
        return 0;
    };
    let (start, first, second) = ring.spans(cursor, frames);
    // SAFETY: acquire of read grants only these free, bounded mapping spans;
    // no mutable reference to the whole mapping overlaps the consumer.
    unsafe {
        ptr::copy_nonoverlapping(input, ring.storage.samples.add(start), first);
        if second != 0 {
            ptr::copy_nonoverlapping(input.add(first), ring.storage.samples, second);
        }
    }
    ring.publication
        .written
        .store(cursor.wrapping_add(u64::from(frames)), Release);
    frames
}

// SAFETY: retained initialized ring and serialized consumer; output is writable
// for frames*channels floats, disjoint from the mapping, including rejected reads.
pub(super) unsafe fn read(
    ring: *mut PcmRing,
    output: *mut f32,
    frames: u32,
    prefill: bool,
    after_copy: impl FnOnce(&PcmRing),
) -> u32 {
    // SAFETY: native stream retains storage while its consumer is running.
    let Some(ring) = (unsafe { ring.as_ref() }) else {
        return 0;
    };
    if ring.storage.samples.is_null() || output.is_null() || frames == 0 {
        return 0;
    }
    let count = frames as usize * ring.channels as usize;
    // SAFETY: only this serialized consumer touches reader state.
    let reader = unsafe { &mut *ring.reader.get() };
    if let Some(window) = ring.reserve_read(reader, frames, prefill) {
        let (start, first, second) = ring.spans(window.cursor, frames);
        // SAFETY: acquire observed completed producer PCM. Only these published
        // spans are read, before the consumer release permits their reuse.
        unsafe {
            ptr::copy_nonoverlapping(ring.storage.samples.add(start), output, first);
            if second != 0 {
                ptr::copy_nonoverlapping(ring.storage.samples, output.add(first), second);
            }
        }
        after_copy(ring);
        if ring.finish_read(reader, &window, frames) {
            return frames;
        }
    }
    // SAFETY: rejected/invalidated reads still own the complete caller output.
    unsafe {
        ptr::write_bytes(output, 0, count);
    }
    0
}

/// Reads a whole block after two-quantum prefill, otherwise silences the full output.
/// # Safety
/// Native stream retains ring and grants this call sole consumer ownership. Output
/// is writable for frames*channels floats, disjoint from PCM storage, even on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_read(
    ring: *mut PcmRing,
    output: *mut f32,
    frames: u32,
) -> u32 {
    // SAFETY: native consumer owns reader state and the complete output block.
    unsafe { read(ring, output, frames, true, |_| {}) }
}

/// Reads a whole block without prefill; retains epoch checks and failure silence.
/// # Safety
/// Same retained ring, serialized consumer and complete output contract as read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_read_available(
    ring: *mut PcmRing,
    output: *mut f32,
    frames: u32,
) -> u32 {
    // SAFETY: native consumer owns reader state and the complete output block.
    unsafe { read(ring, output, frames, false, |_| {}) }
}

/// Returns independent diagnostic observations; null returns an empty record.
/// # Safety
/// Native stream retains initialized or zeroed ring storage until this read ends.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_pcm_ring_stats(ring: *const PcmRing) -> RingStats {
    // SAFETY: only immutable geometry and atomic observations are accessed.
    unsafe { ring.as_ref() }.map_or_else(RingStats::default, PcmRing::stats)
}
