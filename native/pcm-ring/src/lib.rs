//! Callback SPSC device PCM, independent of `CoreAudio` and `PipeWire` policy.
//! Supports x86-64/AArch64 macOS/Linux native consumers.
#![cfg(any(target_os = "macos", target_os = "linux"))]

// The embedded C layout and anonymous-mapping ABI are defined for these targets.
// Do not silently use their constants on a different Unix ABI.
#[cfg(not(all(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_pointer_width = "64"
)))]
compile_error!("native PCM storage supports only 64-bit x86-64/AArch64 macOS/Linux");

use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
mod ffi;
mod queue;
mod sample_format;
mod storage;
pub use ffi::{
    mr_pcm_ring_clear, mr_pcm_ring_discard, mr_pcm_ring_init, mr_pcm_ring_read,
    mr_pcm_ring_read_available, mr_pcm_ring_read_pcm, mr_pcm_ring_restart, mr_pcm_ring_stats,
    mr_pcm_ring_write, mr_pcm_ring_write_pcm, mr_pcm_ring_write_silence,
};
pub use sample_format::SampleFormat;
pub use storage::prefault_pcm;

/// Embedded opaque native storage. Zero-initialization owns no resources; init
/// must finish before callbacks start. Never copy, clear or reinitialize it live.
/// Producer and consumer have distinct private cells; observers use atomics only.
#[repr(C)]
#[derive(Debug, Default)]
pub struct PcmRing {
    storage: storage::Storage,
    channels: u32,
    capacity: u32,
    publication: Publication,
    writer_epoch: UnsafeCell<u64>,
    reader: UnsafeCell<Reader>,
}

#[repr(C)]
#[derive(Debug, Default)]
struct Publication {
    written: AtomicU64,
    read: AtomicU64,
    quantum: AtomicU32,
    overflows: AtomicU64,
    starvations: AtomicU64,
    requested_epoch: AtomicU64,
    epoch_sequence: AtomicU64,
    published_epoch: AtomicU64,
    epoch_start: AtomicU64,
}

#[repr(C)]
#[derive(Debug, Default)]
struct Reader {
    epoch: u64,
    primed: bool,
}

/// Diagnostic loads are independent observations, not a transactional snapshot.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct RingStats {
    /// Consumer's released frame position.
    pub read: u64,
    /// Producer's published frame position.
    pub written: u64,
    /// Number of capacity-rejected write calls, not dropped frames.
    pub overflows: u64,
    /// Number of primed-to-starved transitions.
    pub starvations: u64,
    /// Immutable initialized channel width.
    pub channels: u32,
    /// Available frames, or zero for an incoherent/out-of-range observation.
    pub queued: u32,
}

const _: () = assert!(size_of::<PcmRing>() == 120 && align_of::<PcmRing>() == 8);
const _: () = assert!(size_of::<RingStats>() == 40 && align_of::<RingStats>() == 8);
const _: () = assert!(cfg!(target_has_atomic = "64") && cfg!(target_has_atomic = "32"));

impl PcmRing {
    /// Allocates and prefaults a device-owned ring before producer/consumer publication.
    /// Returns `None` for unsupported geometry or mapping failure. Dropping the
    /// owner releases its mapping and requires all ring users to have drained.
    #[must_use]
    pub fn new(channels: u32, capacity: u32) -> Option<Self> {
        if !(1..=64).contains(&channels) || !(4..=65536).contains(&capacity) {
            return None;
        }
        Some(Self {
            storage: storage::Storage::new(channels as usize * capacity as usize).ok()?,
            channels,
            capacity,
            ..Self::default()
        })
    }

    /// Reads immutable geometry and independent atomic diagnostic counters.
    /// # Panics
    /// Panics only if the ring's private capacity invariant is violated: a queued
    /// observation bounded by its u32 capacity must itself fit a u32.
    #[must_use]
    pub fn stats(&self) -> RingStats {
        let written = self.publication.written.load(Ordering::Acquire);
        let read = self.publication.read.load(Ordering::Acquire);
        let available = written.wrapping_sub(read);
        RingStats {
            read,
            written,
            overflows: self.publication.overflows.load(Ordering::Relaxed),
            starvations: self.publication.starvations.load(Ordering::Relaxed),
            channels: self.channels,
            queued: if available <= u64::from(self.capacity) {
                u32::try_from(available).expect("bounded queue fits u32")
            } else {
                0
            },
        }
    }
}

#[cfg(test)]
mod tests;
