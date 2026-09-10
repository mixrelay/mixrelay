//! Lifetime-bounded C entry points used by the desktop capture owner.

use super::{
    RingOwner, TapConfig, TapError, TapSession, TapStats,
    native::{self, CoreAudio, Native, Timestamp},
};
use crate::process_tap_input::ingest;
use std::{
    alloc::{Layout, alloc},
    ffi::{CStr, c_void},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};
use tonenari_macos_transport::{
    audio_buffer::AudioBufferList,
    callback_clock::tn_callback_clock_record_values,
    capture_ring::{tn_audio_ring_create, tn_audio_ring_read_interleaved},
};

pub(super) struct Active<'a>(&'a AtomicU32);
impl<'a> Active<'a> {
    pub(super) fn enter(counter: &'a AtomicU32) -> Self {
        counter.fetch_add(1, Ordering::Acquire);
        Self(counter)
    }
}
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(super) unsafe extern "C" fn io_proc(
    _: u32,
    _: *const Timestamp,
    input: *const AudioBufferList,
    time: *const Timestamp,
    _: *mut AudioBufferList,
    _: *const Timestamp,
    context: *mut c_void,
) -> i32 {
    // SAFETY: native registration retains the heap-pinned owner until unregister
    // succeeds and this active guard drains. No control-only Cell is accessed here.
    let session = unsafe { &*context.cast::<TapSession>() };
    let _active = Active::enter(&session.active_callbacks);
    session.callbacks.fetch_add(1, Ordering::Relaxed);
    if session.allowed()
        && let Some(format) = session.format.get()
    {
        // SAFETY: ring/format are installed before IOProc registration and remain
        // immutable; CoreAudio owns readable buffer extents during this callback.
        let result = unsafe { ingest(format, input, session.ring(), &session.peaks) };
        // SAFETY: native timestamp is readable for the duration of this callback.
        let time = unsafe { time.as_ref() };
        // SAFETY: this is the sole serialized clock writer; other observers use atomics.
        unsafe {
            tn_callback_clock_record_values(
                ptr::from_ref(&session.clock).cast_mut(),
                time.map_or(0.0, |time| time.sample),
                time.map_or(0, |time| time.host),
                time.is_some_and(|time| time.flags & 3 == 3),
                result.frames as u64,
            );
        };
        session
            .captured
            .fetch_add(result.written as u64, Ordering::Relaxed);
    }
    0
}

fn initialize(session: &TapSession, config: &TapConfig) -> Result<(), TapError> {
    let description = native::Description::create(config)?;
    description.tap(session)?;
    let format = CoreAudio.format(session.resources.get().tap)?;
    if !format.supported() {
        return Err(TapError::code(5));
    }
    let rate = format.sample_rate;
    let channels = format.channels;
    session.format.set(format).map_err(|_| TapError::code(1))?;
    let ring = tn_audio_ring_create(config.ring_capacity_frames, channels);
    if ring.is_null() {
        return Err(TapError::code(6));
    }
    session
        .ring
        .set(RingOwner(ring))
        .map_err(|_| TapError::code(1))?;
    session.sample_rate.store(rate.to_bits(), Ordering::Release);
    session.generation.store(1, Ordering::Release);
    native::add_listener(session)?;
    description.aggregate(session)?;
    CoreAudio.register(session)?;
    Ok(())
}

// SAFETY: configuration and its C string have been retained by the create entry.
unsafe fn create(config: &TapConfig) -> Result<*mut TapSession, TapError> {
    if config.bundle_id.is_null()
        || !(256..=1 << 20).contains(&config.ring_capacity_frames)
        || !config.ring_capacity_frames.is_power_of_two()
    {
        return Err(TapError::code(1));
    }
    // SAFETY: caller retains the complete NUL-terminated identifier during startup.
    let bundle = unsafe { CStr::from_ptr(config.bundle_id) };
    if bundle.is_empty() || bundle.to_str().is_err() {
        return Err(TapError::code(1));
    }
    let retirement_queue = native::prepare_cleanup()?;
    // SAFETY: fresh global allocation for exactly one correctly aligned owner.
    let pointer = unsafe { alloc(Layout::new::<TapSession>()) }
        .cast::<c_void>()
        .cast::<TapSession>();
    if pointer.is_null() {
        return Err(TapError::code(6));
    }
    // SAFETY: unpublished allocation is exclusively initialized before any borrowing.
    let mut owner = TapSession::default();
    owner.retirement_queue = retirement_queue;
    unsafe { pointer.write(owner) };
    // SAFETY: initialized owner remains stable through every native registration.
    let session = unsafe { &*pointer };
    // SAFETY: gate is unpublished and acquired by its sole startup control owner.
    if !session.lifecycle.acquire_owner() {
        // SAFETY: no native references exist on failed admission.
        drop(unsafe { Box::from_raw(pointer) });
        return Err(TapError::code(9));
    }
    if let Err(error) = initialize(session, config) {
        // SAFETY: startup transfers its partial resource ownership to async cleanup.
        unsafe { tn_process_tap_destroy(pointer) };
        return Err(error);
    }
    Ok(pointer)
}

/// Creates and owns a prepared tap. Failure returns null and its original error;
/// partially created native resources remain owned by asynchronous cleanup.
/// # Safety
/// Config and its NUL-terminated UTF-8 bundle string remain readable during this call.
/// Optional error points to exclusively writable `TapError` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_create(
    config: *const TapConfig,
    error: *mut TapError,
) -> *mut TapSession {
    // SAFETY: caller retains the borrowed configuration until creation returns.
    let result = match unsafe { config.as_ref() } {
        Some(config) => unsafe { create(config) },
        None => Err(TapError::code(1)),
    };
    // SAFETY: optional output is exclusive and correctly sized by the C caller.
    if let Some(error) = unsafe { error.as_mut() } {
        *error = result.as_ref().err().copied().unwrap_or_default();
    }
    result.unwrap_or(ptr::null_mut())
}

/// Starts the prepared native registration; null is invalid.
/// # Safety
/// Session is owned and control calls are serialized with read/refresh/stop/destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_start(session: *mut TapSession) -> TapError {
    // SAFETY: control retains allocation; shared borrow does not alias callback writer fields.
    unsafe { session.as_ref() }.map_or(TapError::code(1), |session| {
        session.start(&CoreAudio).err().unwrap_or_default()
    })
}

/// Stops native delivery without retiring the reusable session; null is invalid.
/// # Safety
/// Session is owned and control calls are serialized with read/refresh/start/destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_stop(session: *mut TapSession) -> TapError {
    // SAFETY: control retains allocation through this native stop operation.
    unsafe { session.as_ref() }.map_or(TapError::code(1), |session| {
        session.stop(&CoreAudio).err().unwrap_or_default()
    })
}

/// Reconciles native format after producer drain; incompatible geometry stays gated.
/// # Safety
/// Session is owned and control calls are serialized with PCM read/start/stop/destroy.
/// Atomic stat and peak observers may run concurrently while retaining owner lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_refresh_format(session: *mut TapSession) -> TapError {
    // SAFETY: refresh retains allocation; callbacks access no control resource cells.
    unsafe { session.as_ref() }.map_or(TapError::code(1), |session| {
        session.refresh(&CoreAudio).err().unwrap_or_default()
    })
}

/// Consumes the owner without blocking; failures retain all unreleased resources for retry.
/// # Safety
/// Pump/control/stat/peak users have drained and surrendered ownership before destroy.
/// Native callbacks/listeners may remain: cleanup owns their drain. No later pointer use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_destroy(pointer: *mut TapSession) {
    // SAFETY: destroying owner retains allocation until queue transfer.
    let Some(session) = (unsafe { pointer.as_ref() }) else {
        return;
    };
    // SAFETY: atomic retire assigns at most one cleanup owner for this live allocation.
    if session.lifecycle.retire() {
        native::retire(pointer, false);
    }
}

/// Reads queued PCM for the sole pump consumer; gated/null sessions return zero.
/// # Safety
/// Session is retained and this call is serialized with refresh/control and other reads.
/// Destination owns `maximum_frames * channels` writable floats, disjoint from session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_read(
    session: *mut TapSession,
    destination: *mut f32,
    maximum_frames: usize,
) -> usize {
    // SAFETY: pump retains its session until the consumer read ends.
    let Some(session) = (unsafe { session.as_ref() }) else {
        return 0;
    };
    if !session.allowed() {
        return 0;
    }
    // SAFETY: pump owns exclusive output and the sole consumer cursor.
    unsafe { tn_audio_ring_read_interleaved(session.ring(), destination, maximum_frames) }
}

/// Drains channel peaks independently of PCM; invalid pointers/capacity return zero.
/// # Safety
/// Observer retains session lifetime and exclusively owns `maximum_channels` output floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_take_peaks(
    session: *mut TapSession,
    destination: *mut f32,
    maximum_channels: usize,
) -> usize {
    // SAFETY: observer holds the application session lifetime token.
    let Some(session) = (unsafe { session.as_ref() }) else {
        return 0;
    };
    let channels = session
        .format
        .get()
        .map_or(0, |format| format.channels as usize);
    if destination.is_null() {
        return 0;
    }
    let channels = channels.min(maximum_channels);
    // SAFETY: output capacity bounded by both caller's extent and immutable channel width.
    session
        .peaks
        .take(unsafe { std::slice::from_raw_parts_mut(destination, channels) });
    channels
}

/// Returns atomic capture observations; null yields an empty snapshot.
/// # Safety
/// Observer retains the session through this call and never overlaps its final destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_process_tap_stats(session: *const TapSession) -> TapStats {
    // SAFETY: immutable fields and atomics only; no whole-session mutable borrow exists.
    unsafe { session.as_ref() }.map_or_else(TapStats::default, TapSession::stats)
}
