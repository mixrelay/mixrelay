//! Coordinates travel with native/SRC PCM using the same ring's absolute cursor.
use super::{DeviceIo, GraphRead};
use std::sync::atomic::Ordering;

/// Shared alignment reserve in graph frames; zero uses ordinary FIFO policy.
/// # Safety
/// The native owner remains retained during this atomic observation.
pub unsafe fn capture_window(io: *const DeviceIo) -> u32 {
    // SAFETY: caller retains the optional native owner; the field is atomic.
    unsafe { io.as_ref() }.map_or(0, |io| io.progress.capture_window.load(Ordering::Acquire))
}

/// Reserves bounded input history for the largest independent producer burst.
/// # Safety
/// The native owner remains retained; this changes no ring cursor or callback state.
pub unsafe fn set_capture_window(io: *const DeviceIo, frames: u32) {
    // SAFETY: caller retains the optional native owner; no callback cell is borrowed.
    if let Some(io) = unsafe { io.as_ref() } {
        io.progress
            .capture_window
            .store(frames.min(super::FIFO_FRAMES / 2), Ordering::Release);
    }
}

/// Native source period measured between hardware sample/host pairs.
/// # Safety
/// Native owner remains retained throughout this atomic observation.
pub unsafe fn frame_nanoseconds(io: *const DeviceIo) -> f64 {
    // SAFETY: retained optional owner, no control/callback state is borrowed.
    unsafe { io.as_ref() }.map_or(0.0, |io| {
        f64::from_bits(io.progress.frame_nanos.load(Ordering::Acquire))
    })
}

/// Latest graph-input coordinate with the shared alignment reserve retained.
/// # Safety
/// Native owner remains retained; no other consumer mutates its graph cursor.
pub unsafe fn input_time(io: *const DeviceIo, frames: u32) -> Option<u64> {
    // SAFETY: caller retains the optional native owner and serializes its consumer.
    let io = unsafe { io.as_ref() }?;
    if !io.selection.input || io.progress.invalid.load(Ordering::Acquire) {
        return None;
    }
    let stats = io.graph_ring.stats();
    let reserve = frames.max(io.progress.capture_window.load(Ordering::Acquire) / 2);
    let start = stats.written.checked_sub(u64::from(reserve))?;
    if start < stats.read {
        return None;
    }
    io.graph_times.at(start).filter(|time| *time != 0)
}

/// Reads a graph input at its common acquisition coordinate.
/// # Safety
/// Same retained owner/serialized graph consumer contract as `graph_read`.
pub unsafe fn graph_read_at(
    io: *mut DeviceIo,
    samples: &mut [f32],
    epoch: u64,
    timing: (u64, u64, u32),
) -> GraphRead {
    samples.fill(0.0);
    // SAFETY: caller retains the optional native owner throughout this read.
    let Some(io) = (unsafe { io.as_ref() }) else {
        return GraphRead::default();
    };
    if !samples.len().is_multiple_of(io.selection.channels as usize) {
        return GraphRead::default();
    }
    let Ok(frames) = u32::try_from(samples.len() / io.selection.channels as usize) else {
        return GraphRead::default();
    };
    // SAFETY: caller owns the full output and graph cursor; timing carries no pointers.
    unsafe {
        super::queues::graph_read(
            io,
            samples.as_mut_ptr(),
            frames,
            epoch,
            (timing.0, Some((timing.1, timing.2))),
        )
    }
}

/// Reads the exact native/SRC input coordinates alongside its PCM.
/// # Safety
/// Retained owner, sole bridge consumer, complete disjoint PCM/time output extents.
pub unsafe fn bridge_read_timed(io: *mut DeviceIo, samples: &mut [f32], times: &mut [u64]) -> u32 {
    // SAFETY: caller retains the optional native owner and owns this bridge consumer.
    let Some(io) = (unsafe { io.as_ref() }) else {
        return 0;
    };
    if samples.len() != times.len() * io.selection.channels as usize {
        return 0;
    }
    let Ok(frames) = u32::try_from(times.len()) else {
        return 0;
    };
    // SAFETY: this callback owns the bridge consumer and both writable extents.
    let graph = !io.selection.input;
    let stamps = if graph {
        &io.graph_times
    } else {
        &io.native_times
    };
    let mut valid = true;
    let read = unsafe {
        super::queues::bridge_read_observed(io, samples.as_mut_ptr(), frames, |cursor| {
            valid = stamps.read(cursor, times);
        })
    };
    if !valid {
        times[..read as usize].fill(0);
        samples[..read as usize * io.selection.channels as usize].fill(0.0);
    }
    read
}

/// Publishes SRC output coordinates before releasing the matching PCM block.
/// # Safety
/// Retained owner, sole bridge producer, complete immutable PCM/time input extents.
pub unsafe fn bridge_write_timed(io: *mut DeviceIo, samples: &[f32], times: &[u64]) -> u32 {
    // SAFETY: caller retains the optional native owner and owns this bridge producer.
    let Some(io) = (unsafe { io.as_ref() }) else {
        return 0;
    };
    if samples.len() != times.len() * io.selection.channels as usize {
        return 0;
    }
    let Ok(frames) = u32::try_from(times.len()) else {
        return 0;
    };
    let graph = io.selection.input;
    let stamps = if graph {
        &io.graph_times
    } else {
        &io.native_times
    };
    // SAFETY: the same producer owns this exact PCM range; coordinates already precede it.
    unsafe {
        super::queues::bridge_write_observed(io, samples.as_ptr(), frames, |cursor| {
            stamps.publish(cursor, times);
        })
    }
}
