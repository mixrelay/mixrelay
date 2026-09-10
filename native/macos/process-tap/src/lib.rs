//! Application capture ownership: one control owner, one native producer, atomic observers.
#![cfg(target_os = "macos")]

use std::{
    cell::Cell,
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

use mixrelay_macos_transport::{
    callback_clock::{
        CallbackClock, CallbackClockSnapshot, mr_callback_clock_discontinuities,
        mr_callback_clock_snapshot,
    },
    capture_ring::{
        AudioRing, mr_audio_ring_available_frames, mr_audio_ring_destroy,
        mr_audio_ring_dropped_frames,
    },
};
use process_tap_input::{TapFormat, TapPeaks};
use process_tap_lifecycle::TapLifecycle;

mod process_tap_input;
mod process_tap_lifecycle;

#[cfg(all(test, not(debug_assertions)))]
mod benchmark;
mod control;
mod ffi;
mod native;
#[cfg(test)]
mod tests;

pub use ffi::{
    mr_process_tap_create, mr_process_tap_destroy, mr_process_tap_read,
    mr_process_tap_refresh_format, mr_process_tap_start, mr_process_tap_stats, mr_process_tap_stop,
    mr_process_tap_take_peaks,
};

/// Existing application-capture wire error. A nonzero code explains the failed boundary.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("application capture error {code} (OSStatus {os_status})")]
pub struct TapError {
    /// Stable `MRProcessTapErrorCode` value.
    pub code: u32,
    /// Original `OSStatus`, zero for validation/admission errors.
    pub os_status: i32,
}

impl TapError {
    const fn code(code: u32) -> Self {
        Self { code, os_status: 0 }
    }
    fn status(status: i32) -> Result<(), Self> {
        if status == 0 {
            Ok(())
        } else {
            Err(Self {
                code: 4,
                os_status: status,
            })
        }
    }
}

/// Application capture startup options, borrowed only during creation.
#[repr(C)]
#[derive(Debug)]
pub struct TapConfig {
    /// NUL-terminated UTF-8 application bundle identifier.
    pub bundle_id: *const std::ffi::c_char,
    /// Whether the OS mutes the original application stream while captured.
    pub mute_while_captured: bool,
    /// Power-of-two PCM capacity, from 256 through 1,048,576 frames.
    pub ring_capacity_frames: usize,
}

/// Independently sampled diagnostic values; PCM ownership is never transferred.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TapStats {
    /// Number of native callbacks, including gated/malformed buffers.
    pub callback_count: u64,
    /// Frames successfully stored in the capture ring.
    pub captured_frames: u64,
    /// Valid frames rejected for ring capacity.
    pub dropped_frames: u64,
    /// Currently queued frames.
    pub available_frames: usize,
    /// Immutable channel width.
    pub channels: u32,
    /// Latest validated native sample rate.
    pub sample_rate: f64,
    /// Native start has succeeded and stop/unregister has not completed.
    pub running: bool,
    /// Successfully prepared format generations.
    pub format_generation: u64,
    /// Observed native timestamp discontinuities.
    pub clock_discontinuities: u64,
    /// Last coherent sample position.
    pub sample_time: f64,
    /// Last coherent host timestamp.
    pub host_time: u64,
    /// Whether that timestamp snapshot was valid.
    pub clock_valid: bool,
}

const _: () = assert!(size_of::<TapConfig>() == 24 && align_of::<TapConfig>() == 8);
const _: () = assert!(size_of::<TapError>() == 8 && align_of::<TapError>() == 4);
const _: () = assert!(size_of::<TapStats>() == 96 && align_of::<TapStats>() == 8);

#[derive(Debug, Default, Clone, Copy)]
struct Resources {
    tap: u32,
    aggregate: u32,
    proc: *mut std::ffi::c_void,
    listener: bool,
    resume: bool,
}

#[derive(Debug)]
struct RingOwner(*mut AudioRing);

impl Drop for RingOwner {
    fn drop(&mut self) {
        // SAFETY: Session is reclaimed only after callback/listener removal and drain.
        // The consuming desktop destroy contract has already drained pump/stat readers.
        unsafe { mr_audio_ring_destroy(self.0) };
    }
}

/// Opaque capture owner. Control calls are serialized; callback/listener and meter
/// observers access only immutable fields and atomics. No whole-session mutable
/// reference or `Send`/`Sync` assertion is needed at the native callback boundary.
#[derive(Debug, Default)]
pub struct TapSession {
    retirement_queue: usize,
    resources: Cell<Resources>,
    format: OnceLock<TapFormat>,
    ring: OnceLock<RingOwner>,
    lifecycle: TapLifecycle,
    peaks: TapPeaks,
    clock: CallbackClock,
    running: AtomicBool,
    sample_rate: AtomicU64,
    generation: AtomicU64,
    callbacks: AtomicU64,
    captured: AtomicU64,
    active_callbacks: AtomicU32,
    active_listeners: AtomicU32,
}

impl TapSession {
    fn allowed(&self) -> bool {
        // SAFETY: immutable session borrow retains the initialized atomic gate.
        self.lifecycle.delivery_allowed()
    }

    fn ring(&self) -> *mut AudioRing {
        self.ring.get().map_or(ptr::null_mut(), |owner| owner.0)
    }

    fn stats(&self) -> TapStats {
        let mut clock = CallbackClockSnapshot::default();
        // SAFETY: stats readers retain the owner; all ring observations and clock
        // publication are atomic. Neither reaches control-only resources.
        let (read, clock_discontinuities, dropped_frames, available_frames) = unsafe {
            (
                mr_callback_clock_snapshot(&raw const self.clock, &raw mut clock),
                mr_callback_clock_discontinuities(&raw const self.clock),
                mr_audio_ring_dropped_frames(self.ring()),
                mr_audio_ring_available_frames(self.ring()),
            )
        };
        TapStats {
            callback_count: self.callbacks.load(Ordering::Relaxed),
            captured_frames: self.captured.load(Ordering::Relaxed),
            dropped_frames,
            available_frames,
            channels: self.format.get().map_or(0, |format| format.channels),
            sample_rate: f64::from_bits(self.sample_rate.load(Ordering::Acquire)),
            running: self.running.load(Ordering::Acquire),
            format_generation: self.generation.load(Ordering::Acquire),
            clock_discontinuities,
            sample_time: clock.sample_time,
            host_time: clock.host_time,
            clock_valid: read && clock.valid != 0,
        }
    }
}

impl Drop for TapSession {
    fn drop(&mut self) {
        drop(self.ring.take());
        // SAFETY: only the retirement owner drops after native unregister/drain and
        // aggregate/tap destruction succeed. Admission remains held on every retry.
        self.lifecycle.release_owner();
    }
}
