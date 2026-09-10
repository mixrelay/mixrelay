use super::{DeviceIo, MAX_FRAMES};
use std::{
    ptr,
    sync::atomic::Ordering::{Relaxed, Release},
};
use tonenari_pcm_ring::{tn_pcm_ring_discard, tn_pcm_ring_read_available, tn_pcm_ring_write};

// SAFETY: graph rendering is serialized; its output span is complete and disjoint
// from either ring. The SRC worker owns only the opposite producer cursor.
pub(super) unsafe fn graph_read(io: &DeviceIo, samples: *mut f32, frames: u32, epoch: u64) -> u32 {
    if !io.selection.input || frames > MAX_FRAMES {
        return 0;
    }
    graph_progress(io, frames);
    let ring = ptr::from_ref(&io.graph_ring).cast_mut();
    // SAFETY: only the serialized graph consumer accesses this private epoch cell.
    let previous = unsafe { &mut *io.graph_epoch.get() };
    if *previous != epoch {
        // SAFETY: discard changes only this graph consumer's cursor.
        unsafe { tn_pcm_ring_discard(ring) };
        *previous = epoch;
    }
    // SAFETY: retained ring and sole graph consumer, complete caller-owned output.
    unsafe { tn_pcm_ring_read_available(ring, samples, frames) }
}

// SAFETY: one serialized graph producer; immutable input spans frames*channels floats.
pub(super) unsafe fn graph_write(io: &DeviceIo, samples: *const f32, frames: u32) -> u32 {
    if io.selection.input || frames > MAX_FRAMES {
        return 0;
    }
    graph_progress(io, frames);
    // SAFETY: graph is this ring's sole producer; native callbacks use the other ring.
    unsafe { tn_pcm_ring_write(ptr::from_ref(&io.graph_ring).cast_mut(), samples, frames) }
}

fn graph_progress(io: &DeviceIo, frames: u32) {
    io.progress.graph_quantum.store(frames, Relaxed);
    io.progress
        .graph_frames
        .fetch_add(u64::from(frames), Release);
}

// SAFETY: the SRC worker is the selected ring's sole consumer, and its output
// span owns frames*channels writable floats even when no PCM is available.
pub(super) unsafe fn worker_read(io: &DeviceIo, samples: *mut f32, frames: u32) -> u32 {
    if frames == 0 || frames > MAX_FRAMES {
        return 0;
    }
    // No prefill: SRC owns priming, not a second native-queue latency policy.
    // SAFETY: the worker's input ring depends only on immutable direction.
    unsafe {
        tn_pcm_ring_read_available(
            ptr::from_ref(io.ring(!io.selection.input)).cast_mut(),
            samples,
            frames,
        )
    }
}

// SAFETY: sole SRC worker producer, full immutable PCM disjoint from queue storage.
pub(super) unsafe fn worker_write(io: &DeviceIo, samples: *const f32, frames: u32) -> u32 {
    if frames > MAX_FRAMES {
        return 0;
    }
    // SAFETY: worker writes graph input or native output; no other role writes that ring.
    unsafe {
        tn_pcm_ring_write(
            ptr::from_ref(io.ring(io.selection.input)).cast_mut(),
            samples,
            frames,
        )
    }
}

// SAFETY: sole worker consumer; the opposite producer may run concurrently.
pub(super) unsafe fn worker_discard(io: &DeviceIo) {
    // SAFETY: only the worker input ring's consumer cursor is changed.
    unsafe { tn_pcm_ring_discard(ptr::from_ref(io.ring(!io.selection.input)).cast_mut()) };
}
