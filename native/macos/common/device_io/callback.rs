use super::DeviceIo;
use crate::{
    audio_buffer::{self, AudioBufferList},
    callback_clock::mr_callback_clock_record_values,
    device_buffers::{mr_device_buffers_frames, mr_device_buffers_transfer},
};
use mixrelay_pcm_ring::{mr_pcm_ring_discard, mr_pcm_ring_read_available, mr_pcm_ring_write};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::Ordering::{Acquire, Relaxed, Release},
};

#[repr(C)]
#[derive(Default)]
pub(super) struct Timestamp {
    pub(super) sample: f64,
    pub(super) host: u64,
    pub(super) rate_scalar: f64,
    pub(super) word_clock: u64,
    pub(super) smpte: [u8; 24],
    pub(super) flags: u32,
    pub(super) reserved: u32,
}

/// `CoreAudio`'s registered callback, also exercised by the native PCM benchmark.
/// Each call contributes silence to every supplied output before validating its
/// selected layout. Layout failure latches silence until control reconnects.
/// # Safety
/// `CoreAudio` retains the exact `DeviceIo` context until unregister acknowledges drain.
/// Calls for this registration are serialized. Native lists contain their complete
/// trailing metadata arrays and advertised PCM extents; output storage is writable.
/// The worker and graph each own only their opposite queue cursors. Timestamp
/// pointers are null or native `AudioTimeStamp` values retained for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_device_io_process(
    _device: u32,
    _now: *const c_void,
    input: *const c_void,
    input_time: *const c_void,
    output: *mut c_void,
    output_time: *const c_void,
    context: *mut c_void,
) -> i32 {
    // SAFETY: callback owns advertised output PCM; silence precedes all early returns.
    unsafe { silence(output.cast()) };
    // SAFETY: retained stable context, with field-level cells for concurrent roles.
    let Some(io) = (unsafe { context.cast::<DeviceIo>().as_ref() }) else {
        return 0;
    };
    if io.progress.invalid.load(Acquire) {
        return 0;
    }
    // SAFETY: only the serialized IOProc borrows Callback while registered.
    let callback = unsafe { &mut *io.callback.get() };
    let buffers = if io.selection.input {
        input
    } else {
        output.cast_const()
    }
    .cast::<AudioBufferList>();
    // Empty/missing callback directions are not a format failure, matching CoreAudio duplex calls.
    if unsafe { audio_buffer::buffers(buffers, callback.selection.limit as usize) }.is_none() {
        return 0;
    }
    // SAFETY: full native metadata extent retained for this callback.
    let frames = unsafe { mr_device_buffers_frames(buffers, callback.selection) };
    if frames == 0 {
        io.progress.invalid.store(true, Release);
        return 0;
    }
    // SAFETY: selected native timestamp and serialized clock writer belong to this callback.
    unsafe {
        record_clock(
            io,
            if io.selection.input {
                input_time
            } else {
                output_time
            },
            frames,
        );
    };
    io.progress.native_quantum.store(frames, Relaxed);
    // SAFETY: hook/context installed before registration and retained by engine owner;
    // borrowed native buffers never escape the synchronous render dispatch.
    let rendered = callback
        .hook
        .is_some_and(|hook| unsafe { hook(callback.context, input, output, frames) });
    // SAFETY: callback owns selected PCM, scratch and the native ring role.
    if !rendered && !unsafe { transfer(io, callback, buffers, frames) } {
        io.progress.invalid.store(true, Release);
        return 0;
    }
    io.progress.ticks.fetch_add(1, Relaxed);
    io.progress
        .native_frames
        .fetch_add(u64::from(frames), Release);
    0
}

// SAFETY: invoked only by the serialized native callback, with full validated
// metadata, callback-owned scratch and direction-specific ring cursor ownership.
unsafe fn transfer(
    io: &DeviceIo,
    callback: &mut super::Callback,
    buffers: *const AudioBufferList,
    frames: u32,
) -> bool {
    let ring = ptr::from_ref(&io.native_ring).cast_mut();
    if !io.selection.input {
        let epoch = io.progress.output_epoch.load(Acquire);
        if callback.prime || callback.epoch != epoch {
            // SAFETY: native output IOProc is this ring's sole consumer.
            unsafe { mr_pcm_ring_discard(ring) };
            callback.prime = false;
            callback.epoch = epoch;
        }
        // SAFETY: sole native consumer and callback-owned, full-width scratch.
        unsafe { mr_pcm_ring_read_available(ring, callback.scratch.as_mut_ptr(), frames) };
    }
    // SAFETY: scratch is disjoint from native PCM; callback owns the selected output
    // or immutable input. Geometry was admitted for at most MAX_FRAMES*channels.
    if !unsafe {
        mr_device_buffers_transfer(
            buffers,
            callback.selection,
            callback.scratch.as_mut_ptr(),
            io.selection.input,
        )
    } {
        return false;
    }
    if io.selection.input {
        // SAFETY: IOProc is native input ring's sole producer; scratch remains live.
        unsafe { mr_pcm_ring_write(ring, callback.scratch.as_ptr(), frames) };
    }

    true
}
// SAFETY: CoreAudio supplies a full list and writable advertised output extents.
unsafe fn silence(output: *mut AudioBufferList) {
    if output.is_null() {
        return;
    }
    // SAFETY: read trailing metadata from the original native pointer, not a
    // sized-header reference that would narrow the allocation provenance.
    let count = unsafe { ptr::addr_of!((*output).count).read() } as usize;
    // SAFETY: the OS callback contract owns the entire declared metadata array.
    if let Some(buffers) = unsafe { audio_buffer::buffers(output, count) } {
        for buffer in buffers {
            if !buffer.data.is_null() {
                // SAFETY: byte writes need no f32 alignment; even a malformed
                // format is silenced over its actual native writable byte extent.
                unsafe { ptr::write_bytes(buffer.data.cast::<u8>(), 0, buffer.byte_size as usize) };
            }
        }
    }
}

// SAFETY: timestamp is null or retained native ABI storage; this is the sole clock writer.
unsafe fn record_clock(io: &DeviceIo, timestamp: *const c_void, frames: u32) {
    // SAFETY: exact CoreAudio AudioTimeStamp ABI, asserted below.
    let timestamp = unsafe { timestamp.cast::<Timestamp>().as_ref() };
    let (sample, host, valid) = timestamp.map_or((0.0, 0, false), |time| {
        (time.sample, time.host, time.flags & 3 == 3)
    });
    // SAFETY: callback serialized by registration; clock writer uses its own UnsafeCell.
    unsafe {
        mr_callback_clock_record_values(
            ptr::from_ref(&io.clock).cast_mut(),
            sample,
            host,
            valid,
            u64::from(frames),
        );
    };
}

const _: () = assert!(size_of::<Timestamp>() == 64 && std::mem::offset_of!(Timestamp, flags) == 56);
