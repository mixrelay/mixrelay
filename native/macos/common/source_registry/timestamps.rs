//! Time-addressed reads use coordinates published with PCM, under the same lease/epoch.
use super::{Source, SourceRegistry};
use crate::shared_audio_buffer::{SharedAudioBufferStats, mr_shared_audio_buffer_stats};
use std::sync::atomic::Ordering;

impl SourceRegistry {
    /// Latest safe start coordinate for an active source at the requested graph rate.
    #[must_use]
    pub fn source_time(&self, token: u64, rate: u32, frames: u32) -> Option<u64> {
        let read = self.read(token);
        let source = read.source()?;
        let clock = source.mapping.clock();
        // SAFETY: the graph consumer owns the requested rate; this publishes
        // only atomics so a waiting pump can acknowledge the initial format.
        let format = unsafe {
            crate::application_clock::mr_application_clock_request_rate(
                source.mapping.clock.as_ptr(),
                rate,
            )
        };
        if format & u64::from(u32::MAX) != u64::from(rate)
            || clock.acknowledged_format.load(Ordering::Acquire) != format
            || clock.native_running.load(Ordering::Acquire) == 0
        {
            return None;
        }
        // SAFETY: read pins the immutable mapping until this observation ends.
        let stats = unsafe { mr_shared_audio_buffer_stats(source.mapping.buffer.as_ptr()) };
        let producer = clock.producer_burst_frames.load(Ordering::Acquire);
        let reserve = (if producer == 0 { 512 } else { producer })
            .max(clock.converter_burst_frames.load(Ordering::Acquire))
            .saturating_add(clock.publication_drought_frames.load(Ordering::Acquire))
            .saturating_add(
                clock
                    .consumer_burst_frames
                    .load(Ordering::Acquire)
                    .max(frames),
            );
        let frame = stats
            .published_through_frame
            .checked_sub(u64::from(reserve))?;
        // SAFETY: the mapping includes the epoch-tagged coordinate sidecar.
        unsafe { clock.time_at(source.capacity, stats.epoch, frame) }
    }

    /// Actual graph-frame period of the source selected as the application pacing clock.
    #[must_use]
    pub fn source_period(&self, token: u64) -> Option<f64> {
        let read = self.read(token);
        let source = read.source()?;
        // SAFETY: read pins the initialized mapping throughout both coordinate observations.
        let stats = unsafe { mr_shared_audio_buffer_stats(source.mapping.buffer.as_ptr()) };
        let last = stats.published_through_frame.checked_sub(1)?;
        let first = last.checked_sub(128)?;
        let clock = source.mapping.clock();
        let (first, last) = unsafe {
            (
                clock.time_at(source.capacity, stats.epoch, first)?,
                clock.time_at(source.capacity, stats.epoch, last)?,
            )
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "bounded 128-frame host interval"
        )]
        Some(last.checked_sub(first)? as f64 / 128.0)
    }

    /// Reads one graph block at a common acquisition coordinate shared by all inputs.
    /// Unknown/stale coordinates produce silence rather than reusing the latest timestamp.
    pub fn read_at(&self, token: u64, output: &mut [f32], rate: u32, first_nanos: u64) -> u32 {
        let read = self.read(token);
        let Some(source) = read.source() else {
            output.fill(0.0);
            return 0;
        };
        let channels = source.channels as usize;
        let Ok(frames) = u32::try_from(output.len() / channels) else {
            output.fill(0.0);
            return 0;
        };
        self.read_audio_time(token, output, frames, (rate, Some(first_nanos)))
    }
}

pub(super) fn locate(
    source: &Source,
    stats: SharedAudioBufferStats,
    nanos: u64,
    rate: u32,
    extent: (u32, u64),
) -> Option<u64> {
    let (frames, minimum) = extent;
    if stats.epoch == 0 || rate == 0 || nanos == 0 {
        return None;
    }
    let clock = source.mapping.clock();
    let time = |frame| {
        // SAFETY: caller pins this mapping and validates the lease/epoch after copying PCM.
        unsafe { clock.time_at(source.capacity, stats.epoch, frame) }
    };
    let end = stats.published_through_frame;
    let start = end.saturating_sub(u64::from(source.capacity)).max(minimum);
    let mut low = start;
    let mut high = end;
    while low < high {
        let middle = low + (high - low) / 2;
        if time(middle).is_some_and(|time| time >= nanos) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let mut nearest = low;
    if low > start
        && time(low - 1).is_some_and(|previous| {
            previous.abs_diff(nanos) < time(low).map_or(u64::MAX, |next| next.abs_diff(nanos))
        })
    {
        nearest -= 1;
    }
    if nearest.checked_add(u64::from(frames))? > end {
        return None;
    }
    for offset in 0..frames {
        let expected = nanos.checked_add(u64::from(offset) * 1_000_000_000 / u64::from(rate))?;
        let actual = time(nearest + u64::from(offset))?;
        // One output-frame quantization bound; scheduling delay never relaxes this bound.
        if actual.abs_diff(expected) > 1_000_000_000 / u64::from(rate) + 2 {
            return None;
        }
    }
    Some(nearest)
}
