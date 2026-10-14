use super::DeviceIo;
use crate::{
    audio_buffer::{self, AudioBufferList},
    callback_clock::{mr_callback_clock_discontinuities, mr_callback_clock_record_values},
    device_buffers::{native_frames, native_transfer},
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
/// selected layout. Layout failure or a format notification latches silence until reconnect.
/// # Safety
/// `CoreAudio` retains the exact `DeviceIo` context until unregister acknowledges drain.
/// Calls for this registration are serialized. Native lists contain their complete
/// trailing metadata arrays and advertised PCM extents; output storage is writable.
/// The worker and graph each own only their opposite queue cursors. Timestamp
/// pointers are null or native `AudioTimeStamp` values retained for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_device_io_process(
    _device: u32,
    now: *const c_void,
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
    if callback.watch_invalid.load(Acquire) {
        return 0;
    }
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
    let frames = unsafe { native_frames(buffers, callback.selection, &callback.formats) };
    if frames == 0 {
        io.progress.invalid.store(true, Release);
        return 0;
    }
    // SAFETY: selected native timestamp and serialized clock writer belong to this callback.
    unsafe {
        record_clock(
            io,
            callback,
            if io.selection.input {
                input_time
            } else {
                output_time
            },
            frames,
            now,
        );
    };
    io.progress.native_quantum.store(frames, Relaxed);
    // A format listener can fire during metadata/clock handling. Recheck immediately before
    // exposing native storage to the cached-codec hook or the F32 queue conversion.
    if io.progress.invalid.load(Acquire) || callback.watch_invalid.load(Acquire) {
        return 0;
    }
    // SAFETY: hook/context installed before registration and retained by engine owner;
    // borrowed native buffers never escape the synchronous render dispatch.
    // SAFETY: OS retains both complete timestamps for this synchronous callback.
    let timing = unsafe {
        super::IoTiming {
            input: copy_time(input_time),
            output: copy_time(output_time),
        }
    };
    let rendered = callback.formats.all_float()
        && callback.hook.is_some_and(|hook| unsafe {
            hook(callback.context, input, output, frames, &raw const timing)
        });
    if !rendered && !io.selection.input {
        // Retire old output before the PCM hook can produce the new epoch's samples.
        unsafe { prepare_output(io, callback) };
    }
    // SAFETY: callback owns selected PCM, scratch and the native ring role.
    let admitted = !rendered
        && callback
            .pcm_hook
            .is_none_or(|hook| unsafe { hook(callback.pcm_context) });
    if io.progress.invalid.load(Acquire) || callback.watch_invalid.load(Acquire) {
        // A device notification can arrive after the hook has written this block.
        // SAFETY: the callback still owns the full writable native output extent.
        unsafe { silence(output.cast()) };
        return 0;
    }
    if admitted && !unsafe { transfer(io, callback, buffers, frames) } {
        io.progress.invalid.store(true, Release);
        return 0;
    }
    io.progress.ticks.fetch_add(1, Relaxed);
    io.progress
        .native_frames
        .fetch_add(u64::from(frames), Release);
    super::signal_graph_worker();
    0
}

// SAFETY: pointer is null or a complete AudioTimeStamp retained by CoreAudio.
unsafe fn copy_time(pointer: *const c_void) -> super::IoTimestamp {
    unsafe { pointer.cast::<Timestamp>().as_ref() }.map_or_else(
        super::IoTimestamp::default,
        |time| super::IoTimestamp {
            sample_time: time.sample,
            host_time: time.host,
            rate_scalar: time.rate_scalar,
            flags: time.flags,
        },
    )
}

// SAFETY: only this serialized native consumer changes the output ring cursor.
unsafe fn prepare_output(io: &DeviceIo, callback: &mut super::Callback) {
    let epoch = io.progress.output_epoch.load(Acquire);
    if callback.prime || callback.epoch != epoch {
        unsafe { mr_pcm_ring_discard(ptr::from_ref(&io.native_ring).cast_mut()) };
        callback.prime = false;
        callback.epoch = epoch;
    }
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
        // SAFETY: sole native consumer and callback-owned, full-width scratch.
        unsafe { mr_pcm_ring_read_available(ring, callback.scratch.as_mut_ptr(), frames) };
    }
    // SAFETY: scratch is disjoint from native PCM; callback owns the selected output
    // or immutable input. Geometry was admitted for at most MAX_FRAMES*channels.
    if !unsafe {
        native_transfer(
            buffers,
            callback.selection,
            &callback.formats,
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
unsafe fn record_clock(
    io: &DeviceIo,
    callback: &mut super::Callback,
    timestamp: *const c_void,
    frames: u32,
    now: *const c_void,
) {
    // SAFETY: exact CoreAudio AudioTimeStamp ABI, asserted below.
    let timestamp = unsafe { timestamp.cast::<Timestamp>().as_ref() };
    let (sample, host, valid) = timestamp.map_or((0.0, 0, false), |time| {
        (time.sample, time.host, time.flags & 3 == 3)
    });
    // SAFETY: the native registration retains this clock and serializes its writer.
    let before = unsafe { mr_callback_clock_discontinuities(ptr::from_ref(&io.clock)) };
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
    let after = unsafe { mr_callback_clock_discontinuities(ptr::from_ref(&io.clock)) };
    if after != before {
        // SAFETY: CoreAudio retains the same complete timestamp ABI as input/output time.
        let now = unsafe { now.cast::<Timestamp>().as_ref() };
        let (previous_sample, previous_host, previous_frames) =
            callback.previous_timestamp.unwrap_or((f64::NAN, 0, 0));
        io.clock_evidence.publish(super::ClockBreak {
            count: after,
            expected_sample: previous_sample + f64::from(previous_frames),
            actual_sample: sample,
            previous_host,
            actual_host: host,
            flags: timestamp.map_or(0, |time| u64::from(time.flags)),
            callback_host: now.map_or(0, |time| time.host),
            callback_flags: now.map_or(0, |time| u64::from(time.flags)),
        });
    }
    callback.previous_timestamp = Some((sample, host, frames));
}

const _: () = assert!(size_of::<Timestamp>() == 64 && std::mem::offset_of!(Timestamp, flags) == 56);
