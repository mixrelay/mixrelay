//! Time-addressed reads use coordinates published with PCM, under the same lease/epoch.
use super::{Source, SourceRegistry, stream_epoch};
use crate::shared_audio_buffer::{SharedAudioBufferStats, mr_shared_audio_buffer_stats};
use std::sync::atomic::Ordering;

/// The publication a pacing clock observed when it granted graph-frame credit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishedCredit {
    /// The stream identity, as reported by `SourceClock::epoch`.
    pub epoch: u64,
    /// Published graph frames that the credit covers.
    pub through: u64,
}

/// Graph-frame credit that a time-addressed acquisition must cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphCredit {
    /// Frames of this graph read.
    pub frames: u32,
    /// Remaining graph-frame credit, including this read.
    pub available_frames: u32,
    /// The pacing publication that granted this credit. A source that does not
    /// pace the graph has none; its newest publication bounds the coordinate.
    pub published: Option<PublishedCredit>,
}

/// Why a read produced no PCM. No reason advances the source cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadMiss {
    /// No current lease, acknowledged format or PCM epoch, or a busy consumer claim.
    Unavailable,
    /// Priming or rebuffering below the consumer's latency target.
    NotReady,
    /// The requested coordinates extend past the newest publication.
    Ahead,
    /// The requested coordinate precedes retained or unread PCM.
    Behind,
    /// Coordinates inside the retained window are missing or outside one frame period.
    Mismatch,
    /// A concurrent overwrite or lease change invalidated the copied PCM.
    Incomplete,
}
impl std::fmt::Display for ReadMiss {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "source stream unavailable",
            Self::NotReady => "source below its latency target",
            Self::Ahead => "requested PCM is not yet published",
            Self::Behind => "requested PCM is evicted or already consumed",
            Self::Mismatch => "acquisition coordinates do not match the request",
            Self::Incomplete => "source PCM changed during the copy",
        })
    }
}
impl std::error::Error for ReadMiss {}

impl SourceRegistry {
    /// Latest safe start coordinate for an active source at the requested graph rate.
    #[must_use]
    pub fn source_time(&self, token: u64, rate: u32, frames: u32) -> Option<u64> {
        self.source_time_for_credit(
            token,
            rate,
            GraphCredit {
                frames,
                available_frames: frames,
                published: None,
            },
        )
    }

    /// Start coordinate covering the clock's remaining graph-frame credit.
    /// Read quantum and outstanding credit are distinct; the coordinate must
    /// remain inside retained PCM even after a delayed consumer drains a burst.
    /// A credited publication bounds the coordinate: PCM committed after the
    /// pacing snapshot is not yet credited and never advances it.
    #[must_use]
    pub fn source_time_for_credit(
        &self,
        token: u64,
        rate: u32,
        credit: GraphCredit,
    ) -> Option<u64> {
        let GraphCredit {
            frames,
            available_frames,
            published,
        } = credit;
        let read = self.read(token);
        let source = read.source()?;
        if frames > source.capacity || available_frames > source.capacity {
            return None;
        }
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
            )
            .max(available_frames)
            .min(source.capacity);
        let through = match published {
            None => stats.published_through_frame,
            Some(published)
                if published.epoch
                    == stream_epoch(clock.producer_lease.load(Ordering::Acquire), stats.epoch) =>
            {
                // Publication is monotonic within one stream; min only guards the bound.
                published.through.min(stats.published_through_frame)
            }
            // The credit described a replaced stream; the pacer restarts on its next poll.
            Some(_) => return None,
        };
        let frame = through.checked_sub(u64::from(reserve))?;
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
        self.read_audio_time(
            token,
            output,
            frames,
            super::content_latency::ReadTiming {
                rate,
                first_nanos: Some(first_nanos),
                host_ticks: None,
            },
        )
    }
}

pub(super) fn locate(
    source: &Source,
    stats: SharedAudioBufferStats,
    nanos: u64,
    rate: u32,
    extent: (u32, u64),
) -> Result<u64, ReadMiss> {
    let (frames, minimum) = extent;
    if stats.epoch == 0 || rate == 0 || nanos == 0 {
        return Err(ReadMiss::Unavailable);
    }
    // One output-frame quantization bound; scheduling delay never relaxes this bound.
    let tolerance = 1_000_000_000 / u64::from(rate) + 2;
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
    // Every readable coordinate is newer than the request: it was evicted, or
    // this forward-only reader already consumed it.
    let oldest = time(start).or_else(|| end.checked_sub(1).and_then(time));
    if nearest == start && oldest.is_some_and(|oldest| oldest > nanos.saturating_add(tolerance)) {
        return Err(ReadMiss::Behind);
    }
    if nearest
        .checked_add(u64::from(frames))
        .is_none_or(|last| last > end)
    {
        return Err(ReadMiss::Ahead);
    }
    for offset in 0..frames {
        let expected = nanos
            .checked_add(u64::from(offset) * 1_000_000_000 / u64::from(rate))
            .ok_or(ReadMiss::Mismatch)?;
        let actual = time(nearest + u64::from(offset)).ok_or(ReadMiss::Mismatch)?;
        if actual.abs_diff(expected) > tolerance {
            return Err(ReadMiss::Mismatch);
        }
    }
    Ok(nearest)
}

pub(super) fn select(
    source: &Source,
    stats: SharedAudioBufferStats,
    previous: super::Latency,
    policy: &mut super::Latency,
    (rate, frames, requested): (u32, u32, Option<u64>),
) -> Result<(), ReadMiss> {
    let Some(time) = requested else { return Ok(()) };
    let minimum = if previous.started && previous.epoch == stats.epoch {
        previous.next
    } else {
        0
    };
    let next = locate(source, stats, time, rate, (frames, minimum))?;
    policy.next = next;
    policy.target = u32::try_from(stats.published_through_frame.saturating_sub(next))
        .unwrap_or(source.capacity);
    Ok(())
}
