//! Native-rate client PCM the pump has drained but the graph side cannot take yet.
//!
//! HAL client queues hold only a playback target. Until the graph acknowledges a format
//! and the converter is ready, the pump still drains them every period into this
//! engine-owned ring, which retains `retention::RING_FRAMES` frames: four seconds at the
//! highest supported rate.
use mixrelay_macos_transport::client_pcm::{ClientPcmClock, MixWindow};
use std::{alloc::Layout, io};

/// Frames of one timeline, oldest first, in a ring that restarts at slot zero whenever
/// it empties, so steady-state draining touches only its first pages.
pub(super) struct Backlog {
    samples: Box<[f32]>,
    channels: usize,
    head: usize,
    frames: usize,
    clock: Option<ClientPcmClock>,
    first: u64,
    discontinuity: bool,
}

/// One contiguous run of retained frames, ready for the converter.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pending {
    pub(super) clock: ClientPcmClock,
    pub(super) first: u64,
    pub(super) frames: usize,
    /// The run starts a new timeline or follows a gap; the converter restarts.
    pub(super) discontinuity: bool,
}

impl Backlog {
    pub(super) const FRAMES: usize = mixrelay_audio_io::retention::RING_FRAMES as usize;

    /// Reserves the whole ring on control. Pages stay unfaulted until first written.
    /// # Errors
    /// Returns `OutOfMemory` when the ring cannot be reserved.
    pub(super) fn new(channels: usize) -> io::Result<Self> {
        let length = Self::FRAMES
            .checked_mul(channels)
            .filter(|length| *length != 0)
            .ok_or(io::ErrorKind::InvalidInput)?;
        let layout = Layout::array::<f32>(length).map_err(|_| io::ErrorKind::OutOfMemory)?;
        // SAFETY: the layout is nonzero; a null result reports allocation failure.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "the allocation uses the f32 array layout, so it is f32-aligned"
        )]
        let memory = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<f32>();
        if memory.is_null() {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        // SAFETY: the global allocator returned `length` zeroed, aligned f32 values that
        // this box now owns and frees with the same layout.
        let samples = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(memory, length)) };
        Ok(Self {
            samples,
            channels,
            head: 0,
            frames: 0,
            clock: None,
            first: 0,
            discontinuity: false,
        })
    }

    /// Retains one mixed window. A window from another timeline, or after a gap, replaces
    /// what is pending. Returns the frames discarded: replaced or overflowed ones.
    pub(super) fn push(&mut self, window: MixWindow, input: &[f32]) -> u64 {
        let frames = window.frames as usize;
        let mut dropped = 0;
        let continues = !window.discontinuity
            && self
                .clock
                .is_some_and(|clock| same_timeline(clock, window.clock))
            && self.first + self.frames as u64 == window.first;
        if !continues {
            dropped += self.frames as u64;
            self.head = 0;
            self.frames = 0;
            self.clock = Some(window.clock);
            self.first = window.first;
            self.discontinuity = true;
        }
        let overflow = (self.frames + frames).saturating_sub(Self::FRAMES);
        if overflow != 0 {
            self.consume(overflow);
            self.discontinuity = true;
            dropped += overflow as u64;
        }
        for (index, frame) in input.chunks_exact(self.channels).take(frames).enumerate() {
            let slot = (self.head + self.frames + index) % Self::FRAMES;
            self.samples[slot * self.channels..][..self.channels].copy_from_slice(frame);
        }
        self.frames += frames;
        dropped
    }

    /// The oldest contiguous run, at most `limit` frames, without consuming it.
    pub(super) fn pending(&self, limit: usize) -> Option<Pending> {
        let clock = self.clock.filter(|_| self.frames != 0)?;
        Some(Pending {
            clock,
            first: self.first,
            frames: self.frames.min(limit).min(Self::FRAMES - self.head),
            discontinuity: self.discontinuity,
        })
    }

    /// Copies the run `pending` described into `output`.
    pub(super) fn copy(&self, pending: Pending, output: &mut [f32]) {
        let samples = pending.frames * self.channels;
        output[..samples].copy_from_slice(&self.samples[self.head * self.channels..][..samples]);
    }

    /// Releases frames the converter accepted.
    pub(super) fn consume(&mut self, frames: usize) {
        let frames = frames.min(self.frames);
        self.head = (self.head + frames) % Self::FRAMES;
        self.frames -= frames;
        self.first += frames as u64;
        self.discontinuity = false;
        if self.frames == 0 {
            self.head = 0;
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.frames == 0
    }
}

fn same_timeline(left: ClientPcmClock, right: ClientPcmClock) -> bool {
    (left.incarnation, left.epoch, left.anchor, left.rate)
        == (right.incarnation, right.epoch, right.anchor, right.rate)
}

#[cfg(test)]
mod tests;
