//! Validated native stream selection and PCM projection for a device callback.
use std::{ops::Range, slice};

use crate::audio_buffer::{self, AudioBuffer, AudioBufferList};

/// Immutable selected device slice, resolved before registering the callback.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BufferSelection {
    /// Number of selected graph channels (1..64).
    pub channels: u32,
    /// First selected native channel across all streams.
    pub offset: u32,
    /// Exact aggregate width, or zero for an ordinary device.
    pub total: u32,
    /// Native device width limit: 64, or 2048 for an owned aggregate.
    pub limit: u32,
}

struct Layout<'a> {
    buffers: &'a [AudioBuffer],
    selection: BufferSelection,
    frames: usize,
}

impl<'a> Layout<'a> {
    fn validate(buffers: &'a [AudioBuffer], selection: BufferSelection) -> Option<Self> {
        let end = selection.offset.checked_add(selection.channels)?;
        if !(1..=64).contains(&selection.channels)
            || !matches!(selection.limit, 64 | 2048)
            || end > selection.limit
        {
            return None;
        }
        let mut width = 0_u32;
        let mut frames = 0;
        for buffer in buffers {
            if buffer.channels == 0 || buffer.channels > selection.limit {
                return None;
            }
            let stride = buffer.channels * 4;
            if !buffer.byte_size.is_multiple_of(stride) {
                return None;
            }
            let next = width.checked_add(buffer.channels)?;
            if buffer.data.is_null() {
                if width < end && next > selection.offset {
                    return None;
                }
            } else {
                let count = buffer.byte_size / stride;
                if !buffer.data.is_aligned()
                    || !(1..=4096).contains(&count)
                    || (frames != 0 && frames != count)
                {
                    return None;
                }
                frames = count;
            }
            width = next;
        }
        if frames == 0
            || width < end
            || width > selection.limit
            || (selection.total != 0 && width != selection.total)
        {
            return None;
        }
        Some(Self {
            buffers,
            selection,
            frames: frames as usize,
        })
    }

    fn segments(&self) -> impl Iterator<Item = (&AudioBuffer, Range<usize>, usize)> {
        let selected = self.selection.offset..self.selection.offset + self.selection.channels;
        let mut offset = 0;
        self.buffers.iter().filter_map(move |buffer| {
            let start = offset;
            offset += buffer.channels;
            let first = start.max(selected.start);
            let end = offset.min(selected.end);
            (first < end).then(|| {
                (
                    buffer,
                    (first - start) as usize..(end - start) as usize,
                    (first - selected.start) as usize,
                )
            })
        })
    }

    // SAFETY: native selected PCM and scratch are complete, live and disjoint;
    // selected source storage is immutable for this callback.
    unsafe fn capture(&self, scratch: *mut f32) {
        // SAFETY: callback owns exactly frames*selected channels in scratch.
        let target = unsafe {
            slice::from_raw_parts_mut(scratch, self.frames * self.selection.channels as usize)
        };
        for (buffer, range, selected) in self.segments() {
            // SAFETY: validation proves selected stream is nonnull, aligned and
            // holds this many f32 samples; native callback owns the actual extent.
            let source = unsafe {
                slice::from_raw_parts(buffer.data, self.frames * buffer.channels as usize)
            };
            if source.len() == target.len() && range.len() == self.selection.channels as usize {
                target.copy_from_slice(source);
                return;
            }
            if buffer.channels == 1 {
                // A mono plane needs one scalar store per frame, not a dynamic
                // memcpy per sample. Keep this distinct from mixed stream spans.
                for (&sample, graph) in source
                    .iter()
                    .zip(target.chunks_exact_mut(self.selection.channels as usize))
                {
                    graph[selected] = sample;
                }
                continue;
            }
            for (native, graph) in source
                .chunks_exact(buffer.channels as usize)
                .zip(target.chunks_exact_mut(self.selection.channels as usize))
            {
                graph[selected..selected + range.len()].copy_from_slice(&native[range.clone()]);
            }
        }
    }

    // SAFETY: callback exclusively owns selected output PCM and scratch is a
    // disjoint immutable input. Per-stream mutable borrows do not overlap in time.
    unsafe fn render(&self, scratch: *const f32) {
        // SAFETY: native callback retains a complete initialized scratch block.
        let source = unsafe {
            slice::from_raw_parts(scratch, self.frames * self.selection.channels as usize)
        };
        for (buffer, range, selected) in self.segments() {
            // SAFETY: selected backing PCM was validated and this callback owns
            // its output; the borrow ends before processing another stream.
            let target = unsafe {
                slice::from_raw_parts_mut(buffer.data, self.frames * buffer.channels as usize)
            };
            if target.len() == source.len() && range.len() == self.selection.channels as usize {
                target.copy_from_slice(source);
                return;
            }
            if buffer.channels == 1 {
                for (sample, graph) in target
                    .iter_mut()
                    .zip(source.chunks_exact(self.selection.channels as usize))
                {
                    *sample = graph[selected];
                }
                continue;
            }
            for (native, graph) in target
                .chunks_exact_mut(buffer.channels as usize)
                .zip(source.chunks_exact(self.selection.channels as usize))
            {
                native[range.clone()].copy_from_slice(&graph[selected..selected + range.len()]);
            }
        }
    }
}

/// Validates the complete native layout; returns its frame count or zero.
/// # Safety
/// List owns its complete native trailing metadata array for this call. This
/// function inspects metadata only; PCM must remain live for later transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_device_buffers_frames(
    list: *const AudioBufferList,
    selection: BufferSelection,
) -> u32 {
    // SAFETY: retained callback metadata, bounded by the selected device limit.
    unsafe { audio_buffer::buffers(list, selection.limit as usize) }
        .and_then(|buffers| Layout::validate(buffers, selection))
        .and_then(|layout| u32::try_from(layout.frames).ok())
        .unwrap_or(0)
}

/// Transfers selected native channels to/from interleaved scratch. Invalid layout
/// returns false before copying; does not own, allocate or release storage.
/// # Safety
/// Native list/selected PCM remain live and stable for this call. Scratch is
/// aligned, initialized and sized for frames*selection.channels floats, disjoint
/// from native buffers. Capture owns scratch exclusively; render owns native PCM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_device_buffers_transfer(
    list: *const AudioBufferList,
    selection: BufferSelection,
    scratch: *mut f32,
    input: bool,
) -> bool {
    if scratch.is_null() || !scratch.is_aligned() {
        return false;
    }
    // SAFETY: retained native callback metadata; validation precedes PCM borrows.
    let layout = unsafe { audio_buffer::buffers(list, selection.limit as usize) }
        .and_then(|buffers| Layout::validate(buffers, selection));
    let Some(layout) = layout else {
        return false;
    };
    // SAFETY: complete selected PCM spans and disjoint scratch are callback-owned.
    unsafe {
        if input {
            layout.capture(scratch);
        } else {
            layout.render(scratch);
        }
    }
    true
}

#[cfg(test)]
mod tests;
