//! One bounded virtual-input reader owns mixing, retention, conversion and publication.
//!
//! Every cycle drains each client queue up to the native playhead. While the graph's
//! active plan routes this source, the PCM is retained in the engine's backlog whether
//! or not the graph has admitted a format; while it does not, the queue drains and
//! nothing is retained. Conversion takes the backlog in native order, with each frame
//! at its original native coordinate, within a bounded amount of work per cycle.
mod backlog;
mod report;
#[cfg(test)]
mod tests;

use super::{
    Error, cadence,
    converter::{BRIDGE_FRAMES, Converter, PRIMING_FRAMES},
    scheduling::{self, Scheduling},
};
use backlog::{Backlog, Chunk, Timeline, timeline};
use mixrelay_macos_transport::{
    application_clock::ConsumerRoute,
    application_source_transport::{SourceClockRead, SourceClockSnapshot, SourceTransport},
    client_pcm::{ClientPcmClock, ClientPcmMixer, ClientPcmReader, MixError, QUEUE_FRAMES},
};
use mixrelay_realtime::{Timebase, host_time};
use report::Report;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Converter periods per cycle: the live period and up to seven of catch-up. A source
/// admitted four seconds late catches up in 0.6 s, and a full retention (5.5 s at
/// 192 kHz) in 0.8 s, while a cycle never converts the whole backlog at once. Graph
/// credit grows by the same bound, in publication order.
const FEED_PERIODS: usize = 8;
/// Channel-frames the sinc resampler may convert in one cycle at 48 kHz, scaled by the
/// period at other rates. The 256-tap bridge measured 99 ns per channel-frame in stereo
/// and 32 ns at 64 channels (release build, Apple M2 Pro), so this budget costs 0.41 ms
/// in stereo and 0.13 ms at 64 channels there, against the 1.07 ms computation budget
/// of a 48 kHz period. Slower hosts, the Intel CI runners among them, were not
/// measured; the budget assumes they are at most 2.5 times slower. Stereo keeps all
/// eight periods up to 96 kHz and four at 192 kHz; from 16 channels at 48 kHz the
/// two-period floor applies.
const SINC_FRAMES_AT_48K: usize = 4_096;
/// Catch-up must outpace the live period, or a backlog would never drain.
const MIN_FEED_PERIODS: usize = 2;
/// Mix windows covering a full client queue, so one cycle drains it after any stall.
const DRAIN_WINDOWS: u32 = QUEUE_FRAMES / cadence::FRAMES + 1;
/// Longest wait at a run boundary of a head stream for its consumer to read the current
/// epoch, whose unread PCM a new epoch would discard. Only the head consumer reads an
/// epoch from its first credit, so only its unread catch-up can be large; it reads its
/// credit within a few periods, and a stalled one does not hold the new run longer.
const BOUNDARY_WAIT: Duration = Duration::from_millis(200);
const _: () = assert!(
    BRIDGE_FRAMES >= FEED_PERIODS * cadence::FRAMES as usize + PRIMING_FRAMES + 256,
    "the bridge holds a cycle of catch-up, its priming and the 256-tap sinc history"
);

/// First and longest delay before preparing a converter again after a failure. The
/// delay doubles per failure: an allocation failure under memory pressure clears within
/// that time or keeps retrying once a second, never on every 2.7 ms period.
const RETRY_FIRST: Duration = Duration::from_millis(10);
const RETRY_LONGEST: Duration = Duration::from_secs(1);

/// Bounded exponential backoff for converter preparation, in host ticks.
#[derive(Debug, Default)]
struct Retry {
    at: Option<u64>,
    delay: Option<Duration>,
}
impl Retry {
    fn waiting(&self, now: u64) -> bool {
        self.at.is_some_and(|at| now < at)
    }
    fn failed(&mut self, now: u64, timebase: Timebase) -> Duration {
        let delay = self
            .delay
            .map_or(RETRY_FIRST, |delay| (delay * 2).min(RETRY_LONGEST));
        self.delay = Some(delay);
        self.at = Some(now.saturating_add(timebase.ticks(delay)));
        delay
    }
    fn succeeded(&mut self) {
        *self = Self::default();
    }
}

/// Control-owned signals shared with one pump.
pub(super) struct Signals {
    pub(super) update: Arc<parking_lot::Mutex<Option<Vec<ClientPcmReader>>>>,
    pub(super) updated: Arc<AtomicBool>,
    pub(super) stop: Arc<AtomicBool>,
}

pub(super) struct Pump {
    queues: Vec<ClientPcmReader>,
    transport: SourceTransport,
    signals: Signals,
    input: Vec<f32>,
    scratch: Vec<f32>,
    times: Vec<u64>,
    mixer: ClientPcmMixer,
    backlog: Backlog,
    converter: Option<Converter>,
    /// The native timeline of the PCM last pushed into the converter.
    stream: Option<Timeline>,
    /// Whether the current stream started for the plan's head consumer.
    head: bool,
    /// When the feed first stopped at the current run boundary.
    boundary: Option<u64>,
    /// Queue skips while the source was not routed, which are not losses.
    unwanted_skips: u64,
    /// Backoff after conversion storage could not be prepared.
    retry: Retry,
    #[cfg(test)]
    failing_converters: u32,
    report: Report,
}
impl Pump {
    pub(super) fn prepare(
        queues: Vec<ClientPcmReader>,
        transport: SourceTransport,
        signals: Signals,
    ) -> Result<Self, Error> {
        let channels = queues.first().ok_or(Error::Geometry)?.capture_channels() as usize;
        let input =
            mixrelay_audio_io::pcm_buffer::zeroed(128 * channels).map_err(std::io::Error::other)?;
        let scratch =
            mixrelay_audio_io::pcm_buffer::zeroed(128 * 64).map_err(std::io::Error::other)?;
        let times =
            mixrelay_audio_io::pcm_buffer::timestamps(128).map_err(std::io::Error::other)?;
        Ok(Self {
            queues,
            transport,
            signals,
            input,
            scratch,
            times,
            mixer: ClientPcmMixer::default(),
            backlog: Backlog::new(channels)?,
            converter: None,
            stream: None,
            head: false,
            boundary: None,
            unwanted_skips: 0,
            retry: Retry::default(),
            #[cfg(test)]
            failing_converters: 0,
            report: Report::new(host_time()),
        })
    }
    pub(super) fn run(mut self) -> Result<(), Error> {
        let timebase = Timebase::current();
        let scheduling = Scheduling::new();
        let result = (|| {
            while !self.signals.stop.load(Ordering::Acquire) {
                // AcqRel consumes a control notification; the replacement mutex
                // is entered only after demotion, outside the PCM transfer.
                if self.signals.updated.swap(false, Ordering::AcqRel) {
                    scheduling.outside(|| {
                        if let Some(queues) = self.signals.update.lock().take() {
                            self.queues = queues;
                        }
                    });
                }
                let wait = self.cycle(host_time(), timebase, &scheduling)?;
                scheduling::wait_for_period(wait, timebase);
            }
            Ok(())
        })();
        // Complete demotion before conversion buffers and VM mappings leave scope.
        let demoted = scheduling.finish();
        self.transport.stop_native();
        if let Some(losses) = self.report.final_losses(self.losses()) {
            losses.log();
        }
        result.and(demoted)
    }

    /// One period: native stop, then the plan's route, retention of every readable
    /// client frame, and bounded conversion. Returns the native clock whose next period
    /// ends the wait, or `None` for a control poll during a handshake.
    fn cycle(
        &mut self,
        now: u64,
        timebase: Timebase,
        scheduling: &Scheduling,
    ) -> Result<Option<ClientPcmClock>, Error> {
        let source = self.transport.observe_clock();
        if source == SourceClockRead::Revoked {
            return Err(Error::Stale);
        }
        let native = self.queues.iter().find_map(ClientPcmReader::clock);
        if let Some(native) = native.filter(|native| !native.running) {
            // Stop precedes the format and backlog gates. A stopped virtual source's
            // unpublished PCM is not read, so its retained run ends here as well.
            self.transport.stop_native();
            self.report.stopped(self.backlog.clear());
            self.stream = None;
            self.boundary = None;
            return Ok(Some(native));
        }
        let route = self.transport.consumer_route();
        let routed = matches!(route, ConsumerRoute::Routed { .. });
        self.drain(now, timebase, routed)?;
        match route {
            // A plan publishes each source's route on its first cycle. Until then the
            // pump keeps what one client queue would hold, without converting it, so a
            // head consumer still finds the client's first frames.
            ConsumerRoute::Unknown => {
                self.backlog.keep_newest(u64::from(QUEUE_FRAMES));
                return Ok(native);
            }
            // An unrouted source still publishes for meters and pacing, at its live edge
            // and without retaining: only what one cycle can convert stays.
            ConsumerRoute::Unrouted => self
                .backlog
                .keep_newest((FEED_PERIODS * cadence::FRAMES as usize) as u64),
            ConsumerRoute::Routed { .. } => {}
        }
        if routed && let Some(losses) = self.report.due_losses(now, timebase, self.losses()) {
            scheduling.outside(|| losses.log());
        }
        let SourceClockRead::Ready(clock) = source else {
            return Ok(None);
        };
        if clock.format_word == 0 {
            return Ok(None);
        }
        self.report.format_requested(now);
        Ok(if self.feed(clock, now, timebase, scheduling)? {
            native
        } else {
            None
        })
    }

    fn losses(&self) -> (u64, u64) {
        (
            self.mixer.skipped_frames() - self.unwanted_skips,
            self.backlog.dropped_frames(),
        )
    }

    /// Moves every published client frame up to the native playhead into the backlog.
    /// Queue skips count as loss only for a routed source.
    fn drain(&mut self, now: u64, timebase: Timebase, routed: bool) -> Result<(), Error> {
        let channels = self.input.len() / 128;
        let skipped = self.mixer.skipped_frames();
        for _ in 0..DRAIN_WINDOWS {
            let Some(window) = self.mixer.window(&self.queues, |clock| {
                cadence::frame(
                    timebase.nanoseconds(now.saturating_sub(clock.anchor)),
                    clock.rate,
                )
            }) else {
                break;
            };
            let input = &mut self.input[..window.frames as usize * channels];
            match ClientPcmMixer::mix(&self.queues, window, input, &mut self.scratch) {
                Ok(()) => {}
                // A clock or membership change in progress; the next cycle sees it settled.
                Err(MixError::Changed) => break,
                Err(MixError::Geometry) => return Err(Error::Geometry),
            }
            self.backlog.push(window, input);
            self.report.retained(now);
            self.mixer.commit(window);
        }
        if !routed {
            self.unwanted_skips += self.mixer.skipped_frames() - skipped;
        }
        Ok(())
    }

    /// Periods one cycle may convert: all of `FEED_PERIODS` for a same-clock copy, and
    /// a channel-frame budget scaled by the native period through the sinc resampler.
    fn feed_periods(&self, clock: SourceClockSnapshot, rate: u32) -> usize {
        if clock.clock_master != 0 && clock.format_word & u64::from(u32::MAX) == u64::from(rate) {
            return FEED_PERIODS;
        }
        let channels = self.input.len() / 128;
        (SINC_FRAMES_AT_48K * 48_000 / rate.max(1) as usize / (128 * channels.max(1)))
            .clamp(MIN_FEED_PERIODS, FEED_PERIODS)
    }

    /// Converts retained PCM in native order within the cycle's bound. Each window
    /// observes the source again: a new stream commits a new epoch.
    fn feed(
        &mut self,
        mut clock: SourceClockSnapshot,
        now: u64,
        timebase: Timebase,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        let mut published = 0;
        let mut ready = true;
        let periods = self
            .backlog
            .peek(128)
            .map_or(0, |first| self.feed_periods(clock, first.clock.rate));
        for period in 0..periods {
            let Some(mut chunk) = self.backlog.peek(128) else {
                break;
            };
            if period != 0 {
                clock = match self.transport.observe_clock() {
                    SourceClockRead::Ready(clock) => clock,
                    SourceClockRead::Deferred => {
                        ready = false;
                        break;
                    }
                    SourceClockRead::Revoked => return Err(Error::Stale),
                };
            }
            let restart = chunk.restart
                || self
                    .converter
                    .as_ref()
                    .is_none_or(|converter| !converter.current(clock, chunk.clock.rate));
            if restart {
                // A new epoch discards unread PCM of the current one: end this cycle's
                // feed at the boundary, then wait for the consumer to read it.
                if published != 0 || !self.boundary_read(clock, now, timebase) {
                    break;
                }
                let head = self.transport.consumer_route() == ConsumerRoute::Routed { head: true };
                if !head {
                    // Only the plan's head consumer reads from its first credit. Any other
                    // joins at the live edge, so its cursor and the shared capture
                    // frontier never move back to a retained head.
                    self.backlog.keep_newest(u64::from(cadence::FRAMES));
                    let Some(newest) = self.backlog.peek(128) else {
                        break;
                    };
                    chunk = newest;
                }
                if !self.restart(chunk, clock, now, scheduling) {
                    ready = false;
                    break;
                }
                self.head = head;
            }
            if !self.transfer(chunk, clock, timebase)? {
                ready = false;
                break;
            }
            published += chunk.frames;
        }
        if published == 0 {
            if ready {
                self.flush_idle(clock)?;
            }
        } else if self.report.awaiting_publication() {
            scheduling.outside(|| self.report.log_admission(now, timebase, published));
        }
        Ok(ready)
    }

    /// Whether a new epoch may start: the current stream is not a head stream, or its
    /// consumer has read it, has stopped reading it, or has had `BOUNDARY_WAIT`.
    fn boundary_read(&mut self, clock: SourceClockSnapshot, now: u64, timebase: Timebase) -> bool {
        if !self.head
            || self.converter.is_none()
            || clock.produced_frames == 0
            || clock.consumer_running == 0
            || clock.consumed_frames >= clock.produced_frames
        {
            return true;
        }
        let since = *self.boundary.get_or_insert(now);
        now.saturating_sub(since) >= timebase.ticks(BOUNDARY_WAIT)
    }

    /// Starts a new source epoch for `chunk`, reusing the converter's storage when its
    /// geometry is unchanged. Returns false when the format acknowledgement defers or
    /// conversion storage waits for its next attempt.
    fn restart(
        &mut self,
        chunk: Chunk,
        clock: SourceClockSnapshot,
        now: u64,
        scheduling: &Scheduling,
    ) -> bool {
        let reused = match self.converter.as_mut() {
            Some(converter) if converter.reusable(clock, chunk.clock.rate) => {
                Some(converter.restart(&mut self.transport, clock))
            }
            _ => None,
        };
        let started = match reused {
            Some(started) => started,
            None if self.retry.waiting(now) => return false,
            None => scheduling.outside(|| {
                scheduling.retune(chunk.clock.rate);
                let native = (chunk.clock.rate, self.input.len() / 128);
                match self.new_converter(clock, native) {
                    Ok(Some(converter)) => {
                        self.converter = Some(converter);
                        self.retry.succeeded();
                        true
                    }
                    Ok(None) => false,
                    // Conversion storage could not be prepared: the source keeps its
                    // retained PCM and tries again later instead of ending the endpoint.
                    Err(error) => {
                        let delay = self.retry.failed(now, Timebase::current());
                        tracing::warn!(
                            boundary = "hal_shared_pcm",
                            stage = "source_admission",
                            %error,
                            retry_ms = delay.as_millis(),
                            "virtual input converter preparation failed"
                        );
                        false
                    }
                }
            }),
        };
        if !started {
            // Deferred acknowledgement keeps the retained PCM and cannot leave an old
            // converter attached to a newly committed epoch.
            self.report.deferred();
            return false;
        }
        self.report.converter_ready(
            now,
            chunk.clock,
            clock.format_word & u64::from(u32::MAX),
            self.backlog.frames(),
        );
        self.backlog.restarted();
        self.boundary = None;
        true
    }

    fn new_converter(
        &mut self,
        clock: SourceClockSnapshot,
        native: (u32, usize),
    ) -> Result<Option<Converter>, Error> {
        #[cfg(test)]
        if self.failing_converters != 0 {
            self.failing_converters -= 1;
            return Err(Error::Geometry);
        }
        Converter::new(&mut self.transport, clock, native)
    }

    /// Writes converted output left by a failed publication while its stream is still
    /// wanted. A stopped or unwanted timeline ends the stream, so its output can never
    /// restore credit or running state after `stop_native`.
    fn flush_idle(&mut self, clock: SourceClockSnapshot) -> Result<(), Error> {
        if let Some(converter) = self.converter.as_mut()
            && converter.acknowledged(clock)
            && self.stream.is_some()
        {
            let _ = converter.flush(&mut self.transport)?;
        }
        Ok(())
    }

    fn transfer(
        &mut self,
        chunk: Chunk,
        clock: SourceClockSnapshot,
        timebase: Timebase,
    ) -> Result<bool, Error> {
        let converter = self.converter.as_mut().ok_or(Error::Stale)?;
        converter.observe(&self.transport, clock)?;
        if !converter.flush(&mut self.transport)? {
            return Ok(false);
        }
        let channels = self.input.len() / 128;
        let input = &mut self.input[..chunk.frames * channels];
        self.backlog.copy(input);
        for (index, time) in self.times[..chunk.frames].iter_mut().enumerate() {
            let offset = u128::from(chunk.first + index as u64) * 1_000_000_000
                / u128::from(chunk.clock.rate);
            *time = timebase
                .nanoseconds(chunk.clock.anchor)
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        }
        converter.push(&mut self.transport, input, &self.times[..chunk.frames])?;
        self.backlog.consume(chunk.frames);
        self.stream = Some(timeline(chunk.clock));
        let graph_rate = clock.format_word & u64::from(u32::MAX);
        // One period's publication, also during catch-up: the consumer keeps the largest
        // burst as its reserve for the whole epoch, and its credit already spans catch-up.
        let producer =
            u32::try_from((chunk.frames as u64 * graph_rate).div_ceil(u64::from(chunk.clock.rate)))
                .unwrap_or(u32::MAX);
        self.transport.publish_measurements(
            producer,
            u32::try_from(converter.output_quantum()).unwrap_or(u32::MAX),
            0,
        );
        let _ = converter.flush(&mut self.transport)?;
        Ok(true)
    }
}
