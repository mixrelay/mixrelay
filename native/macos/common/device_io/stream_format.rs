//! Device stream admission, independent of tap packet geometry.

use crate::device_buffers::BufferSelection;

/// Scalar facts copied from a native stream description. No PCM or object ownership.
#[derive(Debug, Clone, Copy)]
pub(super) struct StreamFormat {
    /// Actual native frames per second; never a requested replacement rate.
    pub rate: f64,
    /// Native format identifier.
    pub format: u32,
    /// Native format flags.
    pub flags: u32,
    /// Significant bits in a sample.
    pub bits: u32,
    /// Channels in this stream.
    pub channels: u32,
    /// Bytes in an interleaved frame or one planar channel frame.
    pub frame_bytes: u32,
}

/// Accumulator used by one synchronous native format query.
/// Failed admission leaves the previously accepted prefix unchanged.
#[derive(Debug, Default)]
pub(super) struct StreamFormats {
    channels: u32,
    rate: f64,
}

impl StreamFormats {
    pub(crate) fn accept(&mut self, format: &StreamFormat, limit: u32) -> bool {
        if !matches!(limit, 64 | 2048)
            || format.format != u32::from_be_bytes(*b"lpcm")
            || format.flags & 3 != 1
            || format.bits != 32
            || !format.rate.is_finite()
            || !(8000.0..=384_000.0).contains(&format.rate)
            || format.rate.fract() != 0.0
            || !(1..=limit).contains(&format.channels)
        {
            return false;
        }
        let frame_channels = if format.flags & 32 != 0 {
            1
        } else {
            format.channels
        };
        let Some(channels) = self.channels.checked_add(format.channels) else {
            return false;
        };
        if format.frame_bytes != 4 * frame_channels
            || (self.rate != 0.0 && self.rate.to_bits() != format.rate.to_bits())
            || channels > limit
        {
            return false;
        }
        self.channels = channels;
        self.rate = format.rate;
        true
    }

    pub(crate) fn selected_rate(&self, selection: BufferSelection) -> Option<f64> {
        let end = selection.offset.checked_add(selection.channels)?;
        (matches!(selection.limit, 64 | 2048)
            && (1..=64).contains(&selection.channels)
            && self.rate != 0.0
            && end <= self.channels
            && self.channels <= selection.limit
            && (selection.total == 0 || selection.total == self.channels))
            .then_some(self.rate)
    }
}

#[cfg(test)]
mod tests;
