//! One bounded virtual-input reader owns mixing, conversion and publication.
//!
//! Every period the pump drains client queues into its engine-owned backlog, whether
//! or not the graph side is ready, then feeds the backlog to the converter. The HAL
//! queues therefore only need to cover pump scheduling, not graph format handshakes.
mod backlog;

use super::{
    Error, cadence,
    converter::Converter,
    scheduling::{self, Scheduling},
};
use backlog::{Backlog, Pending};
use mixrelay_macos_transport::{
    application_source_transport::{SourceClockRead, SourceClockSnapshot, SourceTransport},
    client_pcm::{ClientPcmClock, ClientPcmMixer, ClientPcmReader, MixError},
};
use mixrelay_realtime::{Timebase, host_time};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// What the pump does after one cycle.
enum Next {
    /// More retained PCM is ready for the converter.
    Continue,
    /// Wait for the next period of this native clock, or briefly without one.
    Wait(Option<ClientPcmClock>),
}

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
    /// Whether the current overflow episode was reported; cleared once the backlog empties.
    overflowing: bool,
    converter: Option<Converter>,
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
            overflowing: false,
            converter: None,
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
                if let Next::Wait(clock) = self.cycle(timebase, &scheduling)? {
                    scheduling::wait_for_period(clock, timebase);
                }
            }
            Ok(())
        })();
        // Complete demotion before conversion buffers and VM mappings leave scope.
        let demoted = scheduling.finish();
        self.transport.stop_native();
        result.and(demoted)
    }

    fn cycle(&mut self, timebase: Timebase, scheduling: &Scheduling) -> Result<Next, Error> {
        self.retain(timebase, scheduling)?;
        let clock = match self.transport.observe_clock() {
            SourceClockRead::Ready(clock) => clock,
            SourceClockRead::Deferred => return Ok(Next::Wait(None)),
            SourceClockRead::Revoked => return Err(Error::Stale),
        };
        if clock.format_word == 0 {
            return Ok(Next::Wait(None));
        }
        if self.backlog.is_empty() {
            let native = self.queues.iter().find_map(ClientPcmReader::clock);
            if let Some(native) = native {
                if !native.running {
                    self.transport.stop_native();
                } else if self.mixer.is_current(native)
                    && let Some(converter) = self.converter.as_mut()
                    && converter.acknowledged(clock)
                {
                    // Drain a deferred final output even when no new native input arrived.
                    let _ = converter.flush(&mut self.transport)?;
                }
            }
            return Ok(Next::Wait(native));
        }
        Ok(if self.transfer(clock, timebase, scheduling)? {
            Next::Continue
        } else {
            Next::Wait(None)
        })
    }

    /// Moves every published window up to the native playhead into the backlog.
    fn retain(&mut self, timebase: Timebase, scheduling: &Scheduling) -> Result<(), Error> {
        let now = host_time();
        let channels = self.input.len() / 128;
        let mut dropped = 0;
        while let Some(window) = self.mixer.window(&self.queues, |clock| {
            cadence::frame(
                timebase.nanoseconds(now.saturating_sub(clock.anchor)),
                clock.rate,
            )
        }) {
            let input = &mut self.input[..window.frames as usize * channels];
            match ClientPcmMixer::mix(&self.queues, window, input, &mut self.scratch) {
                Ok(()) => {}
                // A membership or clock change in progress; the next cycle sees it settled.
                Err(MixError::Changed) => break,
                Err(MixError::Geometry) => return Err(Error::Geometry),
            }
            dropped += self.backlog.push(window, input);
            self.mixer.commit(window);
        }
        if dropped != 0 && !self.overflowing {
            self.overflowing = true;
            scheduling.outside(|| {
                tracing::warn!(
                    boundary = "virtual_pcm",
                    dropped_frames = dropped,
                    retained_frames = Backlog::FRAMES,
                    "virtual input backlog exceeded retention or changed timeline; oldest PCM dropped"
                );
            });
        }
        Ok(())
    }

    fn configure(
        &mut self,
        pending: Pending,
        clock: SourceClockSnapshot,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        if pending.discontinuity
            || self
                .converter
                .as_ref()
                .is_none_or(|converter| !converter.current(clock, pending.clock.rate))
        {
            let native = (
                pending.clock.rate,
                self.input.len() / 128,
                mixrelay_audio_io::retention::RING_FRAMES,
            );
            let result = scheduling.outside(|| {
                scheduling.retune(pending.clock.rate);
                let Some(converter) = Converter::new(&mut self.transport, clock, native)? else {
                    return Ok::<bool, Error>(false);
                };
                self.converter = Some(converter);
                Ok(true)
            });
            // Deferred format acknowledgment keeps the retained PCM and cannot leave an
            // old converter attached to a newly committed epoch.
            if !result? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Converts the oldest retained run, at most one period, into the source.
    fn transfer(
        &mut self,
        clock: SourceClockSnapshot,
        timebase: Timebase,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        let Some(pending) = self.backlog.pending(128) else {
            return Ok(true);
        };
        if !self.configure(pending, clock, scheduling)? {
            return Ok(false);
        }
        let converter = self.converter.as_mut().ok_or(Error::Stale)?;
        converter.observe(clock)?;
        if !converter.flush(&mut self.transport)? {
            return Ok(false);
        }
        let channels = self.input.len() / 128;
        let frames = pending.frames;
        let input = &mut self.input[..frames * channels];
        self.backlog.copy(pending, input);
        for (index, time) in self.times[..frames].iter_mut().enumerate() {
            let offset = u128::from(pending.first + index as u64) * 1_000_000_000
                / u128::from(pending.clock.rate);
            *time = timebase
                .nanoseconds(pending.clock.anchor)
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        }
        converter.push(&mut self.transport, input, &self.times[..frames])?;
        self.backlog.consume(frames);
        if self.backlog.is_empty() {
            self.overflowing = false;
        }
        let graph_rate = clock.format_word & u64::from(u32::MAX);
        let producer =
            u32::try_from((frames as u64 * graph_rate).div_ceil(u64::from(pending.clock.rate)))
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
