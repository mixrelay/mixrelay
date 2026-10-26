//! Leased source conversion keeps native timestamps and consumer feedback together.
#[cfg(test)]
mod tests;
use super::Error;
use mixrelay_audio_io::clock_bridge::{
    AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
};
use mixrelay_macos_transport::application_source_transport::{
    SourceClockSnapshot, SourceTransport,
};

pub(super) struct Converter {
    bridge: AsyncClockBridge,
    format: u64,
    rate: u32,
    master: bool,
    epoch: u64,
    ticks: u64,
    output: Vec<f32>,
    times: Vec<u64>,
    pending: usize,
}
impl Converter {
    pub(super) fn new(
        transport: &mut SourceTransport,
        clock: SourceClockSnapshot,
        native: (u32, usize, u32),
    ) -> Result<Self, Error> {
        let (rate, channels, capacity) = native;
        let output_rate = u32::try_from(clock.format_word & u64::from(u32::MAX)).unwrap_or(0);
        let mut bridge = AsyncClockBridge::new(BridgeConfig {
            channels,
            input_rate: rate,
            output_rate,
            max_input_frames: 128,
            max_output_frames: 128,
            capacity_frames: capacity as usize,
            priming_frames: 128,
            clock_relation: if clock.clock_master != 0 {
                ClockRelation::ProvenSameClock
            } else {
                ClockRelation::Independent
            },
            feedback_queue: FeedbackQueue::Output,
            max_correction_ppm: 2_000.0,
        })?;
        let output =
            mixrelay_audio_io::pcm_buffer::zeroed(128 * channels).map_err(std::io::Error::other)?;
        let times =
            mixrelay_audio_io::pcm_buffer::timestamps(128).map_err(std::io::Error::other)?;
        if !transport.reset(clock.format_word, rate) {
            return Err(Error::Stale);
        }
        let epoch = transport.clock().ok_or(Error::Stale)?.epoch;
        bridge.reset(epoch, Discontinuity::StreamRestart);
        Ok(Self {
            bridge,
            format: clock.format_word,
            rate,
            master: clock.clock_master != 0,
            epoch,
            ticks: clock.consumer_ticks,
            output,
            times,
            pending: 0,
        })
    }
    pub(super) fn output_quantum(&self) -> usize {
        self.bridge.processing_quantum().1
    }
    pub(super) fn current(&self, clock: SourceClockSnapshot, rate: u32) -> bool {
        self.format == clock.format_word
            && self.rate == rate
            && self.master == (clock.clock_master != 0)
    }
    pub(super) fn observe(&mut self, clock: SourceClockSnapshot) -> Result<(), Error> {
        let elapsed = clock.consumer_ticks.saturating_sub(self.ticks);
        self.ticks = clock.consumer_ticks;
        let rate = self.format & u64::from(u32::MAX);
        self.bridge.observe_clock(ClockFeedback {
            queue: FeedbackQueue::Output,
            epoch: self.epoch,
            queued_frames: usize::try_from(
                clock.produced_frames.saturating_sub(clock.consumed_frames),
            )
            .unwrap_or(usize::MAX),
            target_frames: clock.target_backlog_frames.max(1) as usize,
            elapsed_frames: usize::try_from(elapsed.min(rate)).unwrap_or(0),
            clock_running: clock.consumer_running != 0
                && clock.target_backlog_frames != 0
                && elapsed > 0
                && elapsed <= rate,
        })?;
        Ok(())
    }
    pub(super) fn flush(&mut self, transport: &mut SourceTransport) -> Result<bool, Error> {
        if self.pending != 0 {
            let channels = self.output.len() / 128;
            let frames = u32::try_from(self.pending).unwrap_or(0);
            if !transport.write_pcm(
                &self.output[..self.pending * channels],
                frames,
                Some(&self.times[..self.pending]),
            ) {
                return Ok(false);
            }
            self.pending = 0;
        }
        let report = self
            .bridge
            .pull_timed_interleaved(&mut self.output, &mut self.times)?;
        self.pending = report.transferred_frames;
        if self.pending != 0 {
            let channels = self.output.len() / 128;
            if !transport.write_pcm(
                &self.output[..self.pending * channels],
                u32::try_from(self.pending).unwrap_or(0),
                Some(&self.times[..self.pending]),
            ) {
                return Ok(false);
            }
            self.pending = 0;
        }
        Ok(true)
    }
    pub(super) fn push(
        &mut self,
        transport: &mut SourceTransport,
        input: &mut [f32],
        times: &[u64],
    ) -> Result<(), Error> {
        let faults = mixrelay_audio_io::pcm::sanitize_samples(input);
        if !transport.record_source_faults(self.epoch, faults) {
            return Err(Error::Stale);
        }
        self.bridge.push_timed_interleaved(input, times)?;
        Ok(())
    }
}
