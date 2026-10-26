//! One bounded virtual-input reader owns mixing, conversion and publication.
use super::{
    Error, cadence,
    converter::Converter,
    scheduling::{self, Scheduling},
};
use mixrelay_macos_transport::{
    application_source_transport::{SourceClockRead, SourceTransport},
    client_pcm::{ClientPcmMixer, ClientPcmReader, MixError, MixWindow},
};
use mixrelay_realtime::{Timebase, host_time};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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
                let clock = match self.transport.observe_clock() {
                    SourceClockRead::Ready(clock) => clock,
                    SourceClockRead::Deferred => {
                        scheduling::wait_for_period(None, timebase);
                        continue;
                    }
                    SourceClockRead::Revoked => return Err(Error::Stale),
                };
                let now = host_time();
                let Some(window) = self.mixer.window(&self.queues, |clock| {
                    cadence::frame(
                        timebase.nanoseconds(now.saturating_sub(clock.anchor)),
                        clock.rate,
                    )
                }) else {
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
                    scheduling::wait_for_period(native, timebase);
                    continue;
                };
                if clock.format_word == 0 {
                    scheduling::wait_for_period(None, timebase);
                    continue;
                }
                if !self.configure(window, clock, &scheduling)?
                    || !self.transfer(window, clock, timebase)?
                {
                    scheduling::wait_for_period(None, timebase);
                }
            }
            Ok(())
        })();
        // Complete demotion before conversion buffers and VM mappings leave scope.
        let demoted = scheduling.finish();
        self.transport.stop_native();
        result.and(demoted)
    }
    fn configure(
        &mut self,
        window: MixWindow,
        clock: mixrelay_macos_transport::application_source_transport::SourceClockSnapshot,
        scheduling: &Scheduling,
    ) -> Result<bool, Error> {
        if window.discontinuity
            || self
                .converter
                .as_ref()
                .is_none_or(|converter| !converter.current(clock, window.clock.rate))
        {
            let native = (
                window.clock.rate,
                self.input.len() / 128,
                self.queues[0].capacity(),
            );
            let result = scheduling.outside(|| {
                scheduling.retune(window.clock.rate);
                let Some(converter) = Converter::new(&mut self.transport, clock, native)? else {
                    return Ok::<bool, Error>(false);
                };
                self.converter = Some(converter);
                Ok(true)
            });
            // Deferred format acknowledgment keeps this native window and
            // cannot leave an old converter attached to a newly committed epoch.
            if !result? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn transfer(
        &mut self,
        window: MixWindow,
        clock: mixrelay_macos_transport::application_source_transport::SourceClockSnapshot,
        timebase: Timebase,
    ) -> Result<bool, Error> {
        let converter = self.converter.as_mut().ok_or(Error::Stale)?;
        converter.observe(clock)?;
        if !converter.flush(&mut self.transport)? {
            return Ok(false);
        }
        let channels = self.input.len() / 128;
        let frames = window.frames as usize;
        let input = &mut self.input[..frames * channels];
        match ClientPcmMixer::mix(&self.queues, window, input, &mut self.scratch) {
            Ok(()) => {}
            Err(MixError::Changed) => return Ok(false),
            Err(MixError::Geometry) => return Err(Error::Geometry),
        }
        for (index, time) in self.times[..frames].iter_mut().enumerate() {
            let offset = u128::from(window.first + index as u64) * 1_000_000_000
                / u128::from(window.clock.rate);
            *time = timebase
                .nanoseconds(window.clock.anchor)
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        }
        converter.push(&mut self.transport, input, &self.times[..frames])?;
        self.mixer.commit(window);
        let graph_rate = clock.format_word & u64::from(u32::MAX);
        let producer = u32::try_from(
            (u64::from(window.frames) * graph_rate).div_ceil(u64::from(window.clock.rate)),
        )
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
