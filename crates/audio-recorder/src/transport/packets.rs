//! Recorder-only fixed packet slots. No cursor/stamp retry waits for the writer.
use super::{PACKET_FRAMES, PACKETS};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const FREE: usize = 0;
const WRITING: usize = 1;
const READY: usize = 2;
const READING: usize = 3;

/// One scratch packet owned exclusively by the writer, allocated at preparation.
pub(crate) struct Packet {
    pub(crate) samples: Box<[f32]>,
    pub(crate) frames: usize,
    pub(crate) rate: u32,
}

impl Packet {
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            samples: vec![0.0; PACKET_FRAMES * channels].into_boxed_slice(),
            frames: 0,
            rate: 0,
        }
    }
}

#[derive(Debug)]
struct Slot {
    state: AtomicUsize,
    samples: Box<[AtomicU32]>,
    frames: AtomicUsize,
    rate: AtomicU32,
}

#[derive(Debug)]
pub(super) struct PacketRing {
    channels: usize,
    slots: Box<[Slot]>,
    // Only the admitted audio producer advances write; only the recorder worker
    // advances read. Cursors wrap in the fixed ring, never an integer overflow.
    write: AtomicUsize,
    read: AtomicUsize,
    // Producer-owned partial slot; control takes ownership only after cutoff
    // drains all admitted callbacks. Readers never access a WRITING slot.
    pending: AtomicUsize,
}

impl PacketRing {
    pub(super) fn new(channels: usize) -> Self {
        let slots = (0..PACKETS)
            .map(|_| Slot {
                state: AtomicUsize::new(FREE),
                frames: AtomicUsize::new(0),
                rate: AtomicU32::new(0),
                samples: (0..PACKET_FRAMES * channels)
                    .map(|_| AtomicU32::new(0))
                    .collect(),
            })
            .collect();
        Self {
            channels,
            slots,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            pending: AtomicUsize::new(0),
        }
    }

    /// Called only under Transport's producer guard. A busy slot fails recording
    /// immediately, including a worker preempted before releasing its read slot.
    pub(super) fn publish(&self, mut samples: &[f32], rate: u32) -> bool {
        let current = &self.slots[self.write.load(Ordering::Relaxed)];
        if self.pending.load(Ordering::Relaxed) != 0 && current.rate.load(Ordering::Relaxed) != rate
        {
            self.finish_pending();
        }
        while !samples.is_empty() {
            let index = self.write.load(Ordering::Relaxed);
            let slot = &self.slots[index];
            let offset = self.pending.load(Ordering::Relaxed);
            if offset == 0 {
                if slot
                    .state
                    .compare_exchange(FREE, WRITING, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
                {
                    return false;
                }
                slot.rate.store(rate, Ordering::Relaxed);
            }
            let count = samples.len().min(slot.samples.len() - offset);
            for (destination, sample) in slot.samples[offset..offset + count]
                .iter()
                .zip(&samples[..count])
            {
                destination.store(sample.to_bits(), Ordering::Relaxed);
            }
            self.pending.store(offset + count, Ordering::Relaxed);
            samples = &samples[count..];
            if offset + count == slot.samples.len() {
                self.finish_pending();
            }
        }
        true
    }

    /// The producer calls this for full packets/rate changes. The worker may
    /// call it only after closing admission and waiting for every entrant.
    pub(super) fn finish_pending(&self) {
        let count = self.pending.load(Ordering::Relaxed);
        if count == 0 {
            return;
        }
        let index = self.write.load(Ordering::Relaxed);
        let slot = &self.slots[index];
        slot.frames.store(count / self.channels, Ordering::Relaxed);
        slot.state.store(READY, Ordering::Release);
        self.write.store((index + 1) % PACKETS, Ordering::Relaxed);
        self.pending.store(0, Ordering::Relaxed);
    }

    /// One worker consumes FIFO packets. READY acquire observes complete PCM and
    /// rate metadata; FREE release prevents overwrite until the copy completes.
    /// Atomic sample words keep the shared storage entirely in safe Rust.
    pub(super) fn read(&self, packet: &mut Packet) -> bool {
        let index = self.read.load(Ordering::Relaxed);
        let slot = &self.slots[index];
        if slot
            .state
            .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        packet.frames = slot.frames.load(Ordering::Relaxed);
        packet.rate = slot.rate.load(Ordering::Relaxed);
        for (destination, source) in packet.samples[..packet.frames * self.channels]
            .iter_mut()
            .zip(&slot.samples)
        {
            *destination = f32::from_bits(source.load(Ordering::Relaxed));
        }
        slot.state.store(FREE, Ordering::Release);
        self.read.store((index + 1) % PACKETS, Ordering::Relaxed);
        true
    }
}

#[cfg(test)]
mod tests;
