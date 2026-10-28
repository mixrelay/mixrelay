//! HAL-owned client PCM mappings with control-only XPC grants and absolute clocks.
//! One `MixOutput` writer owns each queue. Readers retain independent VM mappings;
//! removing a client retires its clock before the producer releases its mapping.

mod clock;
mod mapping;
pub use clock::{ClientPcmClockOwner, ClientPcmTimeline, ClockPublication};
mod mixer;
mod snapshot;
pub use mapping::{ClientPcmReader, ClientPcmWriter};
pub use mixer::{ClientPcmMixer, MixError, MixWindow};
pub use snapshot::{ClientPcmSnapshot, SnapshotError};

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

const MAGIC: u64 = 0x4d52_434c_5043_4d31;
const VERSION: u32 = 1;
/// Admission for one endpoint's registered client queues, shared with the HAL owner.
pub const MAX_CLIENTS_PER_ENDPOINT: usize = 256;
/// Existing HAL client-PCM budget, including each frame-tagged ring's header.
pub const MAX_PCM_BYTES_PER_ENDPOINT: usize = 1 << 30;
/// Frames in every HAL client render queue. Its readers consume near the device position:
/// `ReadInput` mixes the device coordinates a writer just published, and the engine pump
/// drains through the native playhead every 128 frames. A writer may lead its slowest
/// reader by one playback target plus one maximum IO buffer; twice that keeps the queue
/// policy's half-capacity headroom. Capture retention lives in the engine's source ring,
/// never here, so this size does not grow with `retention::RING_FRAMES`.
pub const QUEUE_FRAMES: u32 = 32_768;
const _: () = assert!(
    QUEUE_FRAMES.is_power_of_two()
        && QUEUE_FRAMES as usize
            >= 2 * (mixrelay_audio_io::buffer_policy::MAXIMUM_TARGET_FRAMES
                + mixrelay_audio_io::formats::MAX_BUFFER_FRAMES)
);

#[repr(C)]
struct Header {
    magic: u64,
    version: u32,
    bytes: u32,
    endpoint: [u8; 16],
    client: u32,
    channels: u32,
    capacity: u32,
    capture_channels: u32,
    sequence: AtomicU64,
    epoch: AtomicU64,
    anchor: AtomicU64,
    rate: AtomicU32,
    running: AtomicU32,
    first: AtomicU64,
    retired: AtomicU64,
}

/// Device sample zero and rate come from the same native clock as HAL timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientPcmClock {
    pub incarnation: [u8; 16],
    pub epoch: u64,
    pub anchor: u64,
    pub rate: u32,
    pub running: bool,
    pub first: Option<u64>,
    pub through: u64,
}

impl Header {
    fn observe(&self, through: impl FnOnce() -> u64) -> Option<ClientPcmClock> {
        let sequence = self.sequence.load(Ordering::Acquire);
        if sequence & 1 != 0 || self.retired.load(Ordering::Acquire) != 0 {
            return None;
        }
        let epoch = self.epoch.load(Ordering::Relaxed);
        let anchor = self.anchor.load(Ordering::Relaxed);
        let rate = self.rate.load(Ordering::Relaxed);
        let running = self.running.load(Ordering::Relaxed) != 0;
        let first = self.first.load(Ordering::Acquire);
        let through = through();
        // Pair with the control writer's sequence publication, including rearm.
        fence(Ordering::Acquire);
        (self.sequence.load(Ordering::Acquire) == sequence
            && self.retired.load(Ordering::Acquire) == 0)
            .then_some(ClientPcmClock {
                incarnation: [0; 16],
                epoch,
                anchor,
                rate,
                running,
                first: (first != u64::MAX).then_some(first),
                through,
            })
    }
}

fn valid_rate(rate: u32) -> bool {
    mixrelay_audio_io::formats::SAMPLE_RATES.contains(&rate)
}

#[cfg(test)]
mod tests;
