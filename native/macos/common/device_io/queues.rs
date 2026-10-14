use super::{DeviceIo, MAX_FRAMES};
use mixrelay_pcm_ring::{mr_pcm_ring_discard, mr_pcm_ring_read_available, mr_pcm_ring_write};
use std::{
    ptr,
    sync::atomic::Ordering::{Relaxed, Release},
};

/// Actual graph-input transfer and one-time admission of its consumer timeline.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GraphRead {
    /// Valid PCM frames written to the caller's output, never synthetic priming.
    pub frames: u32,
    /// True only for the first nonempty read request of this native owner.
    /// Subsequent shortages and recovery epochs are not initial admission.
    pub initial_epoch: bool,
}

const _: () = assert!(size_of::<GraphRead>() == 8 && align_of::<GraphRead>() == 4);

// SAFETY: graph rendering is serialized; its output span is complete and disjoint
// from either ring. The SRC owner owns only the opposite producer cursor.
pub(super) unsafe fn graph_read(
    io: &DeviceIo,
    samples: *mut f32,
    frames: u32,
    epoch: u64,
) -> GraphRead {
    if !io.selection.input || frames == 0 || frames > MAX_FRAMES {
        return GraphRead::default();
    }
    graph_progress(io, frames);
    let ring = ptr::from_ref(&io.graph_ring).cast_mut();
    // SAFETY: only the serialized graph consumer accesses this private epoch cell.
    let previous = unsafe { &mut *io.graph_epoch.get() };
    let initial_epoch = previous.is_none();
    if *previous != Some(epoch) {
        // SAFETY: discard changes only this graph consumer's cursor.
        unsafe { mr_pcm_ring_discard(ring) };
        *previous = Some(epoch);
        io.measurements.faults.fetch_add(1, Release);
    }
    // SAFETY: retained ring and sole graph consumer, complete caller-owned output.
    let fill = io.graph_ring.stats().queued;
    let read = unsafe { mr_pcm_ring_read_available(ring, samples, frames) };
    if read < frames {
        io.measurements.faults.fetch_add(1, Release);
    }
    io.measurements
        .graph_queue
        .read(u64::from(fill), u64::from(read));
    let frames = read;
    GraphRead {
        frames,
        initial_epoch,
    }
}

// SAFETY: one serialized graph producer; immutable input spans frames*channels floats.
pub(super) unsafe fn graph_write(io: &DeviceIo, samples: *const f32, frames: u32) -> u32 {
    if io.selection.input || frames > MAX_FRAMES {
        return 0;
    }
    graph_progress(io, frames);
    // SAFETY: graph is this ring's sole producer; native callbacks use the other ring.
    unsafe { mr_pcm_ring_write(ptr::from_ref(&io.graph_ring).cast_mut(), samples, frames) }
}

fn graph_progress(io: &DeviceIo, frames: u32) {
    io.progress.graph_quantum.store(frames, Relaxed);
    io.progress
        .graph_frames
        .fetch_add(u64::from(frames), Release);
}

// SAFETY: the SRC owner is the selected ring's sole consumer, and its output
// span owns frames*channels writable floats even when no PCM is available.
pub(super) unsafe fn bridge_read(io: &DeviceIo, samples: *mut f32, frames: u32) -> u32 {
    if frames == 0 || frames > MAX_FRAMES {
        return 0;
    }
    let fill = io.ring(!io.selection.input).stats().queued;
    let tap = if io.selection.input {
        &io.measurements.native_queue
    } else {
        &io.measurements.graph_queue
    };
    // No prefill: SRC owns priming, not a second native-queue latency policy.
    // SAFETY: the bridge owner's input ring depends only on immutable direction.
    let read = unsafe {
        mr_pcm_ring_read_available(
            ptr::from_ref(io.ring(!io.selection.input)).cast_mut(),
            samples,
            frames,
        )
    };
    tap.read(u64::from(fill), u64::from(read));
    read
}

// SAFETY: sole SRC owner producer, full immutable PCM disjoint from queue storage.
pub(super) unsafe fn bridge_write(io: &DeviceIo, samples: *const f32, frames: u32) -> u32 {
    if frames > MAX_FRAMES {
        return 0;
    }
    // SAFETY: bridge owner writes graph input or native output; no other role writes that ring.
    unsafe {
        mr_pcm_ring_write(
            ptr::from_ref(io.ring(io.selection.input)).cast_mut(),
            samples,
            frames,
        )
    }
}

// SAFETY: sole bridge consumer; the opposite producer may run concurrently.
pub(super) unsafe fn bridge_discard(io: &DeviceIo) {
    // SAFETY: only the bridge owner input ring's consumer cursor is changed.
    unsafe { mr_pcm_ring_discard(ptr::from_ref(io.ring(!io.selection.input)).cast_mut()) };
}
