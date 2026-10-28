//! One bounded virtual-input reader owns mixing, retention, conversion and publication.
//!
//! Every cycle drains each client queue up to the native playhead into the engine's
//! backlog, whether or not the graph has admitted a format, so the HAL queues only
//! cover pump scheduling. Conversion takes the backlog in native order, with each
//! frame at its original native coordinate, a bounded number of periods per cycle.
mod backlog;
mod report;
#[cfg(test)]
mod tests;

use super::{
    Error, cadence,
    converter::Converter,
    scheduling::{self, Scheduling},
};
use backlog::{Backlog, Chunk};
use mixrelay_macos_transport::{
    application_source_transport::{SourceClockRead, SourceClockSnapshot, SourceTransport},
    client_pcm::{ClientPcmClock, ClientPcmMixer, ClientPcmReader, MixError, QUEUE_FRAMES},
};
use mixrelay_realtime::{Timebase, host_time};
use report::Report;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Converter periods per cycle: the live period and seven of catch-up. A source admitted
/// four seconds late catches up in 0.6 s, and a full retention (5.5 s at 192 kHz) in
/// 0.8 s, while one cycle converts at most 1,024 native frames, never the whole backlog.
/// Graph credit grows by the same bound, in publication order.
const FEED_PERIODS: usize = 8;
/// Mix windows covering a full client queue, so one cycle drains it after any stall.
const DRAIN_WINDOWS: u32 = QUEUE_FRAMES / cadence::FRAMES + 1;

pub(super) struct Pump {
    queues: Vec<ClientPcmReader>,
    transport: SourceTransport,
    update: Arc<parking_lot::Mutex<Option<Vec<ClientPcmReader>>>>,
    updated: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    input: Vec<f32>,
    scratch: Vec<f32>,
    times: Vec<u64>,
    mixer: ClientPcmMixer,
    backlog: Backlog,
    converter: Option<Converter>,
    report: Report,
}
impl Pump {
    pub(super) fn prepare(
        queues: Vec<ClientPcmReader>,
        transport: SourceTransport,
        update: Arc<parking_lot::Mutex<Option<Vec<ClientPcmReader>>>>,
        stop: Arc<AtomicBool>,
        updated: Arc<AtomicBool>,
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
            update,
            updated,
            stop,
            input,
            scratch,
            times,
            mixer: ClientPcmMixer::default(),
            backlog: Backlog::new(channels)?,
            converter: None,
            report: Report::new(host_time()),
        })
    }
    pub(super) fn run(mut self) -> Result<(), Error> {
        let timebase = Timebase::current();
        let scheduling = Scheduling::new();
        let result = (|| {
            while !self.stop.load(Ordering::Acquire) {
                // AcqRel consumes a control notification; the replacement mutex
                // is entered only after demotion, outside the PCM transfer.
                if self.updated.swap(false, Ordering::AcqRel) {
                    scheduling.outside(|| {
                        if let Some(queues) = self.update.lock().take() {
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

    /// One period: native stop, then retention of every readable client frame, then
    /// bounded conversion. Returns the native clock whose next period ends the wait,
    /// or `None` for a control poll while the source handshake is pending.
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
            return Ok(Some(native));
        }
        self.retain(now, timebase)?;
        if let Some(losses) = self.report.due_losses(now, timebase, self.losses()) {
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
        (self.mixer.skipped_frames(), self.backlog.dropped_frames())
    }

    /// Moves every published client frame up to the native playhead into the backlog.
    fn retain(&mut self, now: u64, timebase: Timebase) -> Result<(), Error> {
        let channels = self.input.len() / 128;
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
            self.mixer.commit(window);
            self.report.retained(now);
        }
        Ok(())
    }

    /// Converts retained PCM in native order, at most `FEED_PERIODS` windows. Each
    /// window observes the source again: a new converter commits a new epoch.
    fn feed(
        &mut self,
        mut clock: SourceClockSnapshot,
        now: u64,
        timebase: Timebase,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        let mut published = 0;
        let mut ready = true;
        for period in 0..FEED_PERIODS {
            let Some(chunk) = self.backlog.peek(128) else {
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
            ready = self.configure(chunk, clock, now, scheduling)?
                && self.transfer(chunk, clock, timebase)?;
            if !ready {
                break;
            }
            published += chunk.frames;
        }
        if published != 0 {
            if self.report.awaiting_publication() {
                scheduling.outside(|| self.report.log_admission(now, timebase, published));
            }
        } else if ready
            && let Some(converter) = self.converter.as_mut()
            && converter.acknowledged(clock)
        {
            // Drain a deferred final output even when no retained input remains.
            let _ = converter.flush(&mut self.transport)?;
        }
        Ok(ready)
    }

    fn configure(
        &mut self,
        chunk: Chunk,
        clock: SourceClockSnapshot,
        now: u64,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        if chunk.restart
            || self
                .converter
                .as_ref()
                .is_none_or(|converter| !converter.current(clock, chunk.clock.rate))
        {
            let native = (
                chunk.clock.rate,
                self.input.len() / 128,
                mixrelay_audio_io::retention::RING_FRAMES,
            );
            let result = scheduling.outside(|| {
                scheduling.retune(chunk.clock.rate);
                let Some(converter) = Converter::new(&mut self.transport, clock, native)? else {
                    self.report.deferred();
                    return Ok::<bool, Error>(false);
                };
                self.converter = Some(converter);
                self.report.converter_ready(
                    now,
                    chunk.clock,
                    clock.format_word & u64::from(u32::MAX),
                    self.backlog.frames(),
                );
                Ok(true)
            });
            // Deferred format acknowledgment keeps the retained PCM and cannot leave
            // an old converter attached to a newly committed epoch.
            if !result? {
                return Ok(false);
            }
            self.backlog.restarted();
        }
        Ok(true)
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
