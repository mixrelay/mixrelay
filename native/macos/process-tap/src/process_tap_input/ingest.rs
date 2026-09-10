use super::{MAX_CHANNELS, TapFormat, TapInputResult, TapPeaks};
use mixrelay_macos_transport::{
    audio_buffer::{AudioBuffer, AudioBufferList, buffers},
    capture_ring::{AudioRing, mr_audio_ring_write_interleaved, mr_audio_ring_write_planar},
};
use std::{ptr, slice};

fn frame_count(format: &TapFormat, parts: &[AudioBuffer]) -> Option<usize> {
    let first = parts.first()?;
    let channels = usize::try_from(format.channels).ok()?;
    let expected_parts = if format.planar() { channels } else { 1 };
    if !format.supported()
        || parts.len() != expected_parts
        || !first.byte_size.is_multiple_of(format.bytes_per_frame)
        || parts.iter().any(|part| {
            part.data.is_null()
                || !part.data.is_aligned()
                || part.byte_size != first.byte_size
                || part.channels != if format.planar() { 1 } else { format.channels }
        })
    {
        return None;
    }
    usize::try_from(first.byte_size / format.bytes_per_frame).ok()
}

// SAFETY: frame_count validated non-null/aligned native parts, equal complete
// Float32 extents and channels. CoreAudio retains readable bytes for this callback.
unsafe fn record(peaks: &TapPeaks, format: &TapFormat, parts: &[AudioBuffer]) {
    if format.planar() {
        for (channel, part) in parts.iter().enumerate() {
            // SAFETY: each validated planar part contains byte_size / 4 floats.
            let samples = unsafe { slice::from_raw_parts(part.data, part.byte_size as usize / 4) };
            for &sample in samples {
                peaks.record(channel, sample);
            }
        }
    } else {
        let part = &parts[0];
        // SAFETY: validated interleaved extent consists of whole channel frames.
        let samples = unsafe { slice::from_raw_parts(part.data, part.byte_size as usize / 4) };
        for frame in samples.chunks_exact(format.channels as usize) {
            for (channel, &sample) in frame.iter().enumerate() {
                peaks.record(channel, sample);
            }
        }
    }
}

/// Validates one tap block, records its peaks and writes its PCM without allocation.
/// Null/malformed input returns zero frames without reading PCM or changing meters.
/// # Safety
/// `MRProcessTap` retains the native list and its declared readable PCM extents for
/// the serialized `IOProc`. Format is immutable and matches the live ring's channel
/// width. Ring has this sole producer; peaks are initialized and owner-retained.
/// All buffers are disjoint from ring storage. No destruction overlaps this call.
pub(crate) unsafe fn ingest(
    format: &TapFormat,
    input: *const AudioBufferList,
    ring: *mut AudioRing,
    peaks: &TapPeaks,
) -> TapInputResult {
    // SAFETY: CoreAudio supplies the readable descriptor table for this callback.
    let Some(parts) = (unsafe { buffers(input, MAX_CHANNELS) }) else {
        return TapInputResult::default();
    };
    let Some(frames) = frame_count(format, parts).filter(|frames| *frames != 0) else {
        return TapInputResult::default();
    };
    // SAFETY: frame_count validated geometry before any PCM dereference.
    unsafe { record(peaks, format, parts) };
    let written = if format.planar() {
        let mut planes = [ptr::null(); MAX_CHANNELS];
        for (plane, part) in planes.iter_mut().zip(parts) {
            *plane = part.data.cast_const();
        }
        // SAFETY: bounded table matches the ring's immutable channel width.
        unsafe { mr_audio_ring_write_planar(ring, planes.as_ptr(), frames) }
    } else {
        // SAFETY: validated interleaved extent is readable for frames*channels.
        unsafe { mr_audio_ring_write_interleaved(ring, parts[0].data, frames) }
    };
    TapInputResult { frames, written }
}
