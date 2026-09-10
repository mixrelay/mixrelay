//! One pump's leased producer endpoint. Storage ownership is shared with the
//! registry implementation; the producer guard serializes remote lease changes.

mod ffi;
pub use ffi::{
    TransportError, TransportErrorCode, tn_application_source_transport_attach,
    tn_application_source_transport_clock, tn_application_source_transport_connect,
    tn_application_source_transport_destroy, tn_application_source_transport_publish_native,
    tn_application_source_transport_reset, tn_application_source_transport_write,
};

use crate::{
    application_clock::{ApplicationClock, FRAME_MASK},
    application_source_mapping::Mapping,
    shared_audio_buffer::{
        tn_shared_audio_buffer_channels, tn_shared_audio_buffer_stats,
        tn_shared_audio_buffer_try_begin_epoch, tn_shared_audio_buffer_write,
    },
};
use std::sync::atomic::Ordering;

/// Coherent consumer observation and the local producer position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct SourceClockSnapshot {
    /// Requested generation and sample rate, encoded by the clock contract.
    pub format_word: u64,
    /// Current mapping-local stream generation.
    pub epoch: u64,
    /// Frames accepted from this producer in the current generation.
    pub produced_frames: u64,
    /// Frames consumed in this generation, or zero for a stale consumer tuple.
    pub consumed_frames: u64,
    /// Accumulated consumer callback frames.
    pub consumer_ticks: u64,
    /// Whether the consumer is running in this generation.
    pub consumer_running: u32,
    /// Whether the source drives the graph clock.
    pub clock_master: u32,
}

/// Opaque C handle exclusively owned by one desktop pump until its join/drop.
/// Other processes share mapping atomics, never references to this local owner.
pub struct SourceTransport {
    mapping: Mapping,
    next_frame: u64,
    owned_epoch: u64,
    owned_lease: u64,
}

struct ProducerClaim<'a>(&'a ApplicationClock);

impl ProducerClaim<'_> {
    fn acquire(clock: &ApplicationClock) -> Option<ProducerClaim<'_>> {
        clock
            .producer_guard
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| ProducerClaim(clock))
    }
}

impl Drop for ProducerClaim<'_> {
    fn drop(&mut self) {
        self.0.producer_guard.store(0, Ordering::Release);
    }
}

impl SourceTransport {
    fn owns_stream(&self) -> bool {
        // SAFETY: the mapping owns the validated buffer for the entire borrow;
        // statistics touch only shared atomics, never local producer state.
        unsafe { tn_shared_audio_buffer_stats(self.mapping.buffer.as_ptr()) }.epoch
            == self.owned_epoch
            && self.mapping.clock().producer_lease.load(Ordering::Acquire) == self.owned_lease
    }

    fn clock(&self) -> Option<SourceClockSnapshot> {
        let clock = self.mapping.clock();
        let sequence = clock.consumer_sequence.load(Ordering::Acquire);
        let format = clock.format_word.load(Ordering::Acquire);
        let position = clock.consumer_position.load(Ordering::Acquire);
        let ticks = clock.consumer_ticks.load(Ordering::Acquire);
        let running = clock.consumer_running.load(Ordering::Acquire);
        if sequence & 1 != 0
            || sequence != clock.consumer_sequence.load(Ordering::Acquire)
            || format != clock.format_word.load(Ordering::Acquire)
            || !self.owns_stream()
        {
            return None;
        }
        let current = position >> 48 == self.owned_epoch;
        Some(SourceClockSnapshot {
            format_word: format,
            epoch: self.owned_epoch,
            produced_frames: self.next_frame,
            consumed_frames: if current { position & FRAME_MASK } else { 0 },
            consumer_ticks: ticks,
            consumer_running: if current { running } else { 0 },
            clock_master: clock.clock_master.load(Ordering::Acquire),
        })
    }

    fn reset(&mut self, format: u64, native_rate: u32) -> bool {
        if format == 0 || !(8000..=192_000).contains(&native_rate) {
            return false;
        }
        let clock = self.mapping.clock();
        let Some(_claim) = ProducerClaim::acquire(clock) else {
            return false;
        };
        if !self.owns_stream() || clock.format_word.load(Ordering::Acquire) != format {
            return false;
        }
        clock.acknowledged_format.store(0, Ordering::Release);
        clock.native_running.store(0, Ordering::Release);
        // SAFETY: the claim excludes registry replacement and all other producers.
        // Only this producer rearms; the consumer retains its independent cursor.
        let epoch = unsafe { tn_shared_audio_buffer_try_begin_epoch(self.mapping.buffer.as_ptr()) };
        if epoch == 0 {
            return false;
        }
        self.owned_epoch = epoch;
        self.next_frame = 0;
        clock
            .native_sample_rate
            .store(native_rate, Ordering::Release);
        clock.acknowledged_format.store(format, Ordering::Release);
        true
    }

    fn publish_native(&self, frames: u64, running: bool) {
        let clock = self.mapping.clock();
        let Some(_claim) = ProducerClaim::acquire(clock) else {
            return;
        };
        if self.owns_stream() {
            clock.native_frames.store(frames, Ordering::Release);
            clock
                .native_running
                .store(u32::from(running), Ordering::Release);
        }
    }

    fn write(&mut self, samples: &[f32], frames: u32) -> bool {
        // SAFETY: mapping attachment validated this immutable channel geometry.
        let channels = unsafe { tn_shared_audio_buffer_channels(self.mapping.buffer.as_ptr()) };
        if samples.len() != frames as usize * channels as usize {
            return false;
        }
        let clock = self.mapping.clock();
        let Some(_claim) = ProducerClaim::acquire(clock) else {
            return false;
        };
        let acknowledged = clock.acknowledged_format.load(Ordering::Acquire);
        if !self.owns_stream()
            || acknowledged == 0
            || acknowledged != clock.format_word.load(Ordering::Acquire)
        {
            return false;
        }
        // SAFETY: the FFI facade validated the frame extent. This claim is the
        // sole producer; input is borrowed and disjoint from the owned mapping.
        let written = unsafe {
            tn_shared_audio_buffer_write(
                self.mapping.buffer.as_ptr(),
                self.owned_epoch,
                self.next_frame,
                samples.as_ptr(),
                frames,
            )
        };
        if written {
            self.next_frame += u64::from(frames);
        }
        written
    }
}

impl Drop for SourceTransport {
    fn drop(&mut self) {
        self.publish_native(0, false);
        // Mapping drops after this publication: unmap first, then close its FD.
    }
}

#[cfg(test)]
mod tests;
