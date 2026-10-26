//! One bounded virtual-input reader owns mixing, conversion and publication.
use super::{Error, converter::Converter, scheduling::Scheduling};
use mixrelay_macos_transport::{
    application_source_transport::SourceTransport,
    client_pcm::{ClientPcmMixer, ClientPcmReader, MixError, MixWindow},
};
use mixrelay_realtime::{Timebase, host_time};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
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
    native_frames: u64,
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
            native_frames: 0,
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
                let now = host_time();
                let Some(window) = self.mixer.window(&self.queues, |anchor| {
                    timebase.nanoseconds(now.saturating_sub(anchor))
                }) else {
                    if self
                        .queues
                        .iter()
                        .any(|queue| queue.clock().is_some_and(|clock| !clock.running))
                    {
                        self.transport.publish_native(self.native_frames, false);
                    }
                    std::thread::sleep(Duration::from_micros(250));
                    continue;
                };
                let clock = self.transport.clock().ok_or(Error::Stale)?;
                if clock.format_word == 0 {
                    std::thread::sleep(Duration::from_micros(250));
                    continue;
                }
                if !self.configure(window, clock, &scheduling)?
                    || !self.transfer(window, clock, timebase)?
                {
                    std::thread::sleep(Duration::from_micros(250));
                }
            }
            Ok(())
        })();
        // Complete demotion before conversion buffers and VM mappings leave scope.
        let demoted = scheduling.finish();
        self.transport.publish_native(self.native_frames, false);
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
                let converter = Converter::new(&mut self.transport, clock, native)?;
                self.converter = Some(converter);
                Ok::<(), Error>(())
            });
            match result {
                Ok(()) => {}
                // A changed format retains this native window for the next
                // current producer acknowledgment; PCM is not advanced here.
                Err(Error::Stale) => return Ok(false),
                Err(error) => return Err(error),
            }
            self.native_frames = 0;
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
        self.native_frames += u64::from(window.frames);
        self.transport.publish_native(self.native_frames, true);
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
