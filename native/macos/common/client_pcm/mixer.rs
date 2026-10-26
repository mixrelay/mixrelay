//! Mixes retained client intervals at native sample positions, independently of HAL reads.
use super::{ClientPcmClock, ClientPcmReader};

/// One endpoint's monotonic consumer; it never advances because another reader did.
#[derive(Debug, Default)]
pub struct ClientPcmMixer {
    timeline: Option<([u8; 16], u64, u64, u32)>,
    next: u64,
    discontinuity: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct MixWindow {
    pub clock: ClientPcmClock,
    pub first: u64,
    pub frames: u32,
    pub discontinuity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixError {
    Geometry,
    Changed,
}
impl std::fmt::Display for MixError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Geometry => "invalid virtual PCM mix geometry",
            Self::Changed => "virtual PCM clock or membership changed",
        })
    }
}
impl std::error::Error for MixError {}

impl ClientPcmMixer {
    /// Selects published PCM no later than the current native playhead. A stalled
    /// client contributes silence at its unpublished positions, without holding peers.
    pub fn window(
        &mut self,
        queues: &[ClientPcmReader],
        now_nanos: impl Fn(u64) -> u64,
    ) -> Option<MixWindow> {
        let mut clocks = queues
            .iter()
            .filter_map(ClientPcmReader::clock)
            .filter(|clock| clock.running && clock.first.is_some());
        let mut clock = clocks.next()?;
        let mut first = clock.first?;
        for candidate in clocks {
            if (
                candidate.incarnation,
                candidate.epoch,
                candidate.anchor,
                candidate.rate,
            ) != (clock.incarnation, clock.epoch, clock.anchor, clock.rate)
            {
                return None;
            }
            first = first.min(candidate.first?);
            clock.through = clock.through.max(candidate.through);
        }
        let playhead = u64::try_from(
            u128::from(now_nanos(clock.anchor)) * u128::from(clock.rate) / 1_000_000_000,
        )
        .ok()?;
        let published = clock.through;
        clock.through = clock.through.min(playhead);
        let capacity = u64::from(queues.iter().map(ClientPcmReader::capacity).min()?);
        let oldest = first.max(published.saturating_sub(capacity));
        let timeline = (clock.incarnation, clock.epoch, clock.anchor, clock.rate);
        let discontinuity = !self.is_current(clock) || self.next < oldest;
        if discontinuity {
            self.timeline = Some(timeline);
            self.next = oldest;
            self.discontinuity = true;
        }
        let frames = u32::try_from(clock.through.saturating_sub(self.next).min(128)).ok()?;
        (frames != 0).then_some(MixWindow {
            clock,
            first: self.next,
            frames,
            discontinuity: self.discontinuity,
        })
    }

    /// Sum each registered client's committed frames once. Unpublished source frames contribute silence.
    /// # Errors
    /// Rejects invalid buffer geometry. Concurrent clock or membership changes
    /// return `Changed`; the caller retains the cursor for a current window.
    pub fn mix(
        queues: &[ClientPcmReader],
        window: MixWindow,
        output: &mut [f32],
        scratch: &mut [f32],
    ) -> Result<(), MixError> {
        output.fill(0.0);
        let Some(first) = queues.first() else {
            return Err(MixError::Geometry);
        };
        let channels = first.capture_channels() as usize;
        let frames = window.frames as usize;
        if channels == 0 || output.len() != frames * channels || frames > 128 {
            return Err(MixError::Geometry);
        }
        let mut coverage = [false; 128];
        for queue in queues {
            let source_channels = queue.channels() as usize;
            if queue.endpoint() != first.endpoint()
                || queue.capture_channels() as usize != channels
                || source_channels * frames > scratch.len()
            {
                output.fill(0.0);
                return Err(MixError::Geometry);
            }
            let scratch = &mut scratch[..source_channels * frames];
            if !queue.read(window.clock, window.first, scratch, &mut coverage[..frames]) {
                output.fill(0.0);
                return Err(MixError::Changed);
            }
            for (target, source) in output
                .chunks_exact_mut(channels)
                .zip(scratch.chunks_exact(source_channels))
            {
                for (target, source) in target.iter_mut().zip(source) {
                    *target += source;
                }
            }
        }
        if queues
            .iter()
            .any(|queue| !super::mapping::same_clock(queue.clock(), window.clock))
        {
            output.fill(0.0);
            return Err(MixError::Changed);
        }
        Ok(())
    }
    /// Whether pending converted PCM still belongs to the active native timeline.
    #[must_use]
    pub fn is_current(&self, clock: ClientPcmClock) -> bool {
        self.timeline == Some((clock.incarnation, clock.epoch, clock.anchor, clock.rate))
    }

    pub fn commit(&mut self, window: MixWindow) {
        self.next = window.first + u64::from(window.frames);
        self.discontinuity = false;
    }
}
