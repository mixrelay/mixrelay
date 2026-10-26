//! One pump's leased producer endpoint. Storage ownership is shared with the
//! registry implementation; the producer guard serializes remote lease changes.

mod ffi;
pub use ffi::{
    TransportError, TransportErrorCode, mr_application_source_transport_attach,
    mr_application_source_transport_clock, mr_application_source_transport_connect,
    mr_application_source_transport_destroy, mr_application_source_transport_publish_measurements,
    mr_application_source_transport_publish_native,
    mr_application_source_transport_record_source_faults, mr_application_source_transport_reset,
    mr_application_source_transport_write, mr_application_source_transport_write_timed,
};

use crate::{
    application_clock::{ApplicationClock, FRAME_MASK},
    application_source_mapping::Mapping,
    shared_audio_buffer::{
        mr_shared_audio_buffer_channels, mr_shared_audio_buffer_stats,
        mr_shared_audio_buffer_try_begin_epoch, mr_shared_audio_buffer_write,
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
    pub target_backlog_frames: u32,
    pub consumer_burst_frames: u32,
    pub starvation_episodes: u64,
    pub starvation_frames: u64,
    pub rebuffer_frames: u64,
    pub overwrite_skips: u64,
    pub insufficient_headroom: u32,
}

/// Opaque C handle exclusively owned by one desktop pump until its join/drop.
/// Other processes share mapping atomics, never references to this local owner.
pub struct SourceTransport {
    mapping: Mapping,
    next_frame: u64,
    owned_epoch: u64,
    owned_lease: u64,
    /// Retained across transient producer-guard contention, scoped to `owned_epoch`.
    pending_source_faults: u64,
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
    /// Creates one producer directly from the owning registry on control.
    /// # Errors
    /// Propagates registry admission and transport attachment failures.
    pub fn create(
        registry: &crate::source_registry::SourceRegistry,
        request: &crate::source_control::SourceRequest,
    ) -> std::io::Result<Self> {
        use std::os::fd::IntoRawFd;
        let attachment = registry.attach(request)?;
        let mut raw = crate::source_control::SourceAttachment {
            descriptor: attachment.descriptor.into_raw_fd(),
            mapped_size: attachment.mapped_size,
            producer_lease: attachment.producer_lease,
            source_epoch: attachment.source_epoch,
        };
        // SAFETY: the registry owns this immutable backing object and grants one producer lease.
        unsafe { ffi::attach(&mut raw, request.capacity_frames, request.channels) }
            .map_err(std::io::Error::other)
    }
    fn owns_stream(&self) -> bool {
        // SAFETY: the mapping owns the validated buffer for the entire borrow;
        // statistics touch only shared atomics, never local producer state.
        unsafe { mr_shared_audio_buffer_stats(self.mapping.buffer.as_ptr()) }.epoch
            == self.owned_epoch
            && self.mapping.clock().producer_lease.load(Ordering::Acquire) == self.owned_lease
    }

    #[must_use]
    pub fn clock(&self) -> Option<SourceClockSnapshot> {
        let clock = self.mapping.clock();
        let sequence = clock.consumer_sequence.load(Ordering::Acquire);
        let format = clock.format_word.load(Ordering::Acquire);
        let position = clock.consumer_position.load(Ordering::Acquire);
        let ticks = clock.consumer_ticks.load(Ordering::Acquire);
        let running = clock.consumer_running.load(Ordering::Acquire);
        let target = clock.target_backlog_frames.load(Ordering::Acquire);
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
            target_backlog_frames: if current { target } else { 0 },
            consumer_burst_frames: clock.consumer_burst_frames.load(Ordering::Acquire),
            starvation_episodes: clock.starvation_episodes.load(Ordering::Acquire),
            starvation_frames: clock.starvation_frames.load(Ordering::Acquire),
            rebuffer_frames: clock.rebuffer_frames.load(Ordering::Acquire),
            overwrite_skips: clock.overwrite_skips.load(Ordering::Acquire),
            insufficient_headroom: clock.insufficient_headroom.load(Ordering::Acquire),
        })
    }

    pub fn reset(&mut self, format: u64, native_rate: u32) -> bool {
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
        flush_source_faults(clock, &mut self.pending_source_faults);
        clock.acknowledged_format.store(0, Ordering::Release);
        clock.native_running.store(0, Ordering::Release);
        // SAFETY: the claim excludes registry replacement and all other producers.
        // Only this producer rearms; the consumer retains its independent cursor.
        let epoch = unsafe { mr_shared_audio_buffer_try_begin_epoch(self.mapping.buffer.as_ptr()) };
        if epoch == 0 {
            return false;
        }
        self.owned_epoch = epoch;
        self.next_frame = 0;
        clock
            .native_sample_rate
            .store(native_rate, Ordering::Release);
        clock.producer_burst_frames.store(0, Ordering::Release);
        clock.converter_burst_frames.store(0, Ordering::Release);
        clock.publication_drought_frames.store(0, Ordering::Release);
        clock.acknowledged_format.store(format, Ordering::Release);
        true
    }

    pub fn publish_measurements(&self, producer: u32, converter: u32, drought: u32) {
        let clock = self.mapping.clock();
        let Some(_claim) = ProducerClaim::acquire(clock) else {
            return;
        };
        if self.owns_stream() {
            clock
                .producer_burst_frames
                .fetch_max(producer, Ordering::Release);
            clock
                .converter_burst_frames
                .fetch_max(converter, Ordering::Release);
            clock
                .publication_drought_frames
                .fetch_max(drought, Ordering::Release);
        }
    }

    pub fn publish_native(&self, frames: u64, running: bool) {
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
        self.write_pcm(samples, frames, None)
    }

    pub fn write_pcm(&mut self, samples: &[f32], frames: u32, times: Option<&[u64]>) -> bool {
        // SAFETY: mapping attachment validated this immutable channel geometry.
        let channels = unsafe { mr_shared_audio_buffer_channels(self.mapping.buffer.as_ptr()) };
        if frames > self.mapping.capacity() || samples.len() != frames as usize * channels as usize
        {
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
        flush_source_faults(clock, &mut self.pending_source_faults);
        if times.is_some_and(|times| times.len() != frames as usize) {
            return false;
        }
        // SAFETY: claimed producer, retained mapping and matching PCM extent. Untimed
        // writes explicitly invalidate any coordinate left by an unsuccessful attempt.
        unsafe {
            clock.write_times(
                self.mapping.capacity(),
                self.owned_epoch,
                self.next_frame,
                (0..frames as usize).map(|index| times.map_or(0, |times| times[index])),
            );
        }
        // SAFETY: the FFI facade validated the frame extent. This claim is the
        // sole producer; input is borrowed and disjoint from the owned mapping.
        let written = unsafe {
            mr_shared_audio_buffer_write(
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

    pub fn record_source_faults(&mut self, epoch: u64, count: u64) -> bool {
        if epoch != self.owned_epoch || !self.owns_stream() {
            return false;
        }
        self.pending_source_faults = self.pending_source_faults.saturating_add(count);
        let clock = self.mapping.clock();
        if let Some(_claim) = ProducerClaim::acquire(clock)
            && self.owns_stream()
        {
            flush_source_faults(clock, &mut self.pending_source_faults);
        }
        true
    }
}

/// Caller holds the producer claim and has validated its actual lease/epoch.
fn flush_source_faults(clock: &ApplicationClock, pending: &mut u64) {
    clock.source_sample_faults.record(std::mem::take(pending));
}

impl Drop for SourceTransport {
    fn drop(&mut self) {
        let clock = self.mapping.clock();
        let claim = ProducerClaim::acquire(clock);
        if claim.is_some() && self.owns_stream() {
            flush_source_faults(clock, &mut self.pending_source_faults);
        }
        // The sole pump drains its writes/resets before Drop. Leases are unique
        // and never reused, so this CAS publishes closure only for our grant.
        // It works even if a remote grant holds the guard and then fails without
        // changing the lease. Epoch invalidation must not prevent lease closure.
        if clock
            .producer_lease
            .compare_exchange(self.owned_lease, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && claim.is_some()
        {
            clock.acknowledged_format.store(0, Ordering::Release);
            clock.native_frames.store(0, Ordering::Release);
            clock.native_running.store(0, Ordering::Release);
        }
        // Mapping drops after the lease closes: unmap first, then close its FD.
    }
}

#[cfg(test)]
mod tests;
