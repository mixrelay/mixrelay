//! WASAPI microphone/device capture, distinct from process-loopback capture.
//! The native worker owns COM, retries invalidation, and produces one bounded
//! PCM ring. The serialized graph worker is its only consumer.
use super::{
    ACTIVATION_TIMEOUT, SpscSampleRing, clock_bridge, drain_selected_capture_packets, render,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};
use tonenari_audio_graph::{AudioDeviceInputConfig, DeviceSelector};
use tonenari_audio_io::process::TransferOutcome;
use windows::{
    Win32::{
        Media::Audio::{
            AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
            eConsole,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree},
    },
    core::PCWSTR,
};

pub(super) type InputCache = BTreeMap<String, (AudioDeviceInputConfig, Weak<DeviceInput>)>;
#[cfg(test)]
pub(super) fn prepare_inputs(
    requests: &[(String, AudioDeviceInputConfig)],
    cache: &mut InputCache,
) -> Result<BTreeMap<String, Arc<DeviceInput>>, String> {
    prepare_inputs_at_rate(requests, cache, super::ENGINE_SAMPLE_RATE)
}

pub(super) fn prepare_inputs_at_rate(
    requests: &[(String, AudioDeviceInputConfig)],
    cache: &mut InputCache,
    graph_rate: u32,
) -> Result<BTreeMap<String, Arc<DeviceInput>>, String> {
    let mut inputs = BTreeMap::new();
    for (id, config) in requests {
        let stream = cache
            .get(id)
            .filter(|(old, _)| same_stream(old, config))
            .and_then(|(_, stream)| stream.upgrade())
            .filter(|stream| stream.graph_rate == graph_rate);
        inputs.insert(
            id.clone(),
            match stream {
                Some(stream) => stream,
                None => Arc::new(DeviceInput::open_at_rate(config.clone(), graph_rate)?),
            },
        );
    }
    cache.clear();
    for (id, config) in requests {
        cache.insert(id.clone(), (config.clone(), Arc::downgrade(&inputs[id])));
    }
    Ok(inputs)
}

fn same_stream(old: &AudioDeviceInputConfig, next: &AudioDeviceInputConfig) -> bool {
    // Gain belongs to graph DSP, not the native capture endpoint lifetime.
    old.selector == next.selector && old.output_channels == next.output_channels
}

pub(super) struct DeviceInput {
    ring: Arc<SpscSampleRing>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    consumed_epoch: AtomicU64,
    primed: AtomicBool,
    channels: usize,
    graph_rate: u32,
    worker: Option<thread::JoinHandle<()>>,
    #[cfg(test)]
    restart: Arc<AtomicBool>,
}
impl DeviceInput {
    #[cfg(test)]
    fn open(config: AudioDeviceInputConfig) -> Result<Self, String> {
        Self::open_at_rate(config, super::ENGINE_SAMPLE_RATE)
    }

    fn open_at_rate(config: AudioDeviceInputConfig, graph_rate: u32) -> Result<Self, String> {
        if !(1..=64).contains(&config.output_channels)
            || matches!(config.selector, DeviceSelector::DefaultOutput)
        {
            return Err("invalid input channel count or selector direction".into());
        }
        let channels = usize::from(config.output_channels);
        let ring = Arc::new(SpscSampleRing::new(channels * 4096));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let restart = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let worker_restart = restart.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let (worker_ring, worker_stop, worker_failed, worker_epoch) =
            (ring.clone(), stop.clone(), failed.clone(), epoch.clone());
        let worker = thread::Builder::new()
            .name("tonenari-wasapi-input".into())
            .spawn(move || {
                tonenari_engine_service::device_worker::supervise(
                    &worker_stop,
                    &worker_failed,
                    &tx,
                    |connected| {
                        let _apartment = render::ComApartment::new()?;
                        let stream = open_stream(&config)?;
                        let mut bridge = clock_bridge::CaptureBridge::new(
                            channels,
                            stream.rate,
                            graph_rate,
                            &worker_ring,
                        )?;
                        if worker_stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        unsafe { stream.client.Start() }.map_err(|e| e.to_string())?;
                        worker_epoch.fetch_add(1, Ordering::Release);
                        connected();
                        while !worker_stop.load(Ordering::Acquire) {
                            let _ = stream.ready.wait()?;
                            #[cfg(test)]
                            if worker_restart.swap(false, Ordering::AcqRel) {
                                return Err("native reconnect acceptance injection".into());
                            }
                            // Query on timeouts too: unplug can stop event delivery.
                            unsafe {
                                drain_selected_capture_packets(
                                    &stream.capture,
                                    usize::from(stream.channels),
                                    &worker_ring,
                                    &mut bridge,
                                    &worker_epoch,
                                )?;
                            }
                        }
                        Ok(())
                    },
                );
            })
            .map_err(|e| e.to_string())?;
        let mut result = Self {
            ring,
            stop,
            failed,
            epoch,
            consumed_epoch: AtomicU64::new(0),
            primed: AtomicBool::new(false),
            channels,
            graph_rate,
            worker: Some(worker),
            #[cfg(test)]
            restart,
        };
        match rx.recv_timeout(ACTIVATION_TIMEOUT) {
            Ok(Ok(())) => Ok(result),
            error => {
                result.stop.store(true, Ordering::Release);
                if error.is_err() {
                    result.worker.take();
                }
                Err(format!("WASAPI input initialization failed: {error:?}"))
            }
        }
    }
    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub(super) fn read(&self, destination: &mut [f32]) -> TransferOutcome {
        if self.failed() {
            destination.fill(0.0);
            self.ring.discard();
            self.primed.store(false, Ordering::Relaxed);
            return super::application_capture::unknown_transfer();
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        if self.consumed_epoch.swap(epoch, Ordering::Relaxed) != epoch {
            self.ring.discard();
            self.primed.store(false, Ordering::Relaxed);
        }
        if !self.primed.load(Ordering::Relaxed) {
            if self.ring.available_frames(self.channels) < clock_bridge::TARGET_FRAMES {
                destination.fill(0.0);
                return super::application_capture::unknown_transfer();
            }
            self.primed.store(true, Ordering::Relaxed);
        }
        let frames = self.ring.pop(destination, self.channels) / self.channels;
        if self.failed() || self.epoch.load(Ordering::Acquire) != epoch {
            destination.fill(0.0);
            return super::application_capture::unknown_transfer();
        }
        if frames == 0 {
            super::application_capture::unknown_transfer()
        } else {
            TransferOutcome::complete(u32::try_from(frames).expect("bounded input ring"))
        }
    }
}

impl Drop for DeviceInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
struct InputStream {
    channels: u16,
    rate: u32,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    ready: render::BufferReadyEvent,
}
impl Drop for InputStream {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}
fn open_stream(config: &AudioDeviceInputConfig) -> Result<InputStream, String> {
    // Resolve the same exact UID/direction/width contract as Linux before opening.
    // In particular, GetDevice alone does not distinguish capture/render IDs.
    config
        .selector
        .resolve(
            &tonenari_audio_io::windows::devices().map_err(|e| e.to_string())?,
            tonenari_audio_io::DeviceDirection::Input,
            config.output_channels,
        )
        .map_err(|e| e.to_string())?;
    // SAFETY: all interfaces and their buffers stay in this owning MTA worker.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let device = match &config.selector {
            DeviceSelector::DefaultInput => enumerator.GetDefaultAudioEndpoint(eCapture, eConsole),
            DeviceSelector::PersistentUid { uid } => {
                let uid: Vec<u16> = uid.encode_utf16().chain(Some(0)).collect();
                enumerator.GetDevice(PCWSTR(uid.as_ptr()))
            }
            DeviceSelector::DefaultOutput => {
                return Err("an output selector cannot capture audio".into());
            }
        }
        .map_err(|e| e.to_string())?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        let mix = client.GetMixFormat().map_err(|e| e.to_string())?;
        if mix.is_null() {
            return Err("device returned no input mix format".into());
        }
        let channels = (*mix).nChannels;
        let rate = (*mix).nSamplesPerSec;
        CoTaskMemFree(Some(mix.cast()));
        if channels < config.output_channels || channels > 64 {
            return Err(
                "input width differs from native device; implicit remix is disabled".into(),
            );
        }
        if !(8_000..=384_000).contains(&rate) {
            return Err("native input sample rate is outside the prepared SRC contract".into());
        }
        let format = render::float_format_at_rate(channels, rate);
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                200_000,
                0,
                &raw const format.Format,
                None,
            )
            .map_err(|e| e.to_string())?;
        let capacity = client.GetBufferSize().map_err(|e| e.to_string())?;
        if capacity == 0 || capacity as usize > clock_bridge::NATIVE_MAX_FRAMES {
            return Err("native input quantum exceeds prepared capacity".into());
        }
        let capture = client
            .GetService::<IAudioCaptureClient>()
            .map_err(|e| e.to_string())?;
        let ready = render::BufferReadyEvent::new(&client)?;
        Ok(InputStream {
            channels,
            rate,
            client,
            capture,
            ready,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn gain_changes_do_not_change_native_stream_identity() {
        let first = AudioDeviceInputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: "fixture".into(),
            },
            output_channels: 2,
            gain_db: 0.0,
        };
        let mut next = first.clone();
        next.gain_db = -24.0;
        assert!(same_stream(&first, &next));
        next.output_channels = 1;
        assert!(!same_stream(&first, &next));
        next = first.clone();
        next.selector = DeviceSelector::DefaultInput;
        assert!(!same_stream(&first, &next));
    }
    #[test]
    fn rejects_wrong_direction_before_opening_native_capture() {
        assert!(
            DeviceInput::open(AudioDeviceInputConfig {
                selector: DeviceSelector::DefaultOutput,
                output_channels: 2,
                gain_db: 0.0
            })
            .is_err()
        );
    }
    #[test]
    #[ignore = "requires authorized interactive Windows VM; uses only ToneNari LAB capture endpoint"]
    fn native_wasapi_device_input_reopen_and_remove() {
        assert_eq!(std::env::var("TONENARI_LAB_AUDIO_TEST").as_deref(), Ok("1"));
        let devices = tonenari_audio_io::windows::devices().unwrap();
        let device = devices
            .iter()
            .find(|d| d.input_channels > 0 && d.display_name.contains("ToneNari"))
            .expect("installed ToneNari LAB input");
        let config = AudioDeviceInputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: device.uid.clone(),
            },
            output_channels: u16::try_from(device.input_channels).unwrap(),
            gain_db: 0.0,
        };
        let mut cache = InputCache::new();
        let requests = vec![("probe".into(), config)];
        let inputs = prepare_inputs(&requests, &mut cache).unwrap();
        let input = &inputs["probe"];
        let mut samples = vec![0.0; input.channels * 256];
        for cycle in 0..4 {
            let previous = input.epoch.load(Ordering::Acquire);
            input.restart.store(true, Ordering::Release);
            let start = Instant::now();
            loop {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "native input failed to reopen"
                );
                if input.epoch.load(Ordering::Acquire) > previous && !input.failed() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let before = input.ring.write.load(Ordering::Acquire);
            let start = Instant::now();
            while input.ring.write.load(Ordering::Acquire) <= before {
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "no native capture packets after recovery"
                );
                let _ = input.read(&mut samples);
                assert!(samples.iter().all(|sample| sample.is_finite()));
                thread::sleep(Duration::from_millis(5));
            }
            let reused = prepare_inputs(&requests, &mut cache).unwrap();
            assert!(Arc::ptr_eq(input, &reused["probe"]));
            eprintln!(
                "native capture cycle={cycle} epoch={} same graph owner; PCM packets resumed",
                input.epoch.load(Ordering::Acquire)
            );
        }
        let weak = Arc::downgrade(input);
        drop(inputs);
        assert!(
            weak.upgrade().is_none(),
            "removed input retained its worker"
        );
    }

    #[test]
    #[ignore = "requires elevated interactive authorized VM; temporarily disables LAB input or its authorized parent"]
    fn native_wasapi_input_pnp_recovery_and_offline_removal() {
        use super::super::pnp_tests::{DisabledEndpoint, endpoint, wait_until};
        let device = endpoint(true);
        let config = AudioDeviceInputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: device.uid.clone(),
            },
            output_channels: u16::try_from(device.input_channels).unwrap(),
            gain_db: 0.0,
        };
        let mut cache = InputCache::new();
        let requests = vec![("pnp-input".into(), config)];
        let inputs = prepare_inputs(&requests, &mut cache).unwrap();
        let input = &inputs["pnp-input"];
        let mut samples = vec![0.0; input.channels * 256];
        for cycle in 0..4 {
            let before = input.epoch.load(Ordering::Acquire);
            let mut disabled = DisabledEndpoint::new(&device);
            wait_until("capture native invalidation", || input.failed());
            samples.fill(1.0);
            let _ = input.read(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));
            let start = Instant::now();
            disabled.restore();
            wait_until("capture native reappearance", || {
                !input.failed() && input.epoch.load(Ordering::Acquire) > before
            });
            // First read discards pre-reconnect PCM. Observe a subsequent native
            // packet, not a buffered packet from the disconnected endpoint.
            let _ = input.read(&mut samples);
            let position = input.ring.write.load(Ordering::Acquire);
            wait_until("new capture PCM after PnP enable", || {
                let _ = input.read(&mut samples);
                assert!(samples.iter().all(|sample| sample.is_finite()));
                input.ring.write.load(Ordering::Acquire) > position
            });
            let reused = prepare_inputs(&requests, &mut cache).unwrap();
            assert!(Arc::ptr_eq(input, &reused["pnp-input"]));
            eprintln!(
                "PnP capture cycle={cycle} epoch={} reenable_to_pcm_ms={} same owner",
                input.epoch.load(Ordering::Acquire),
                start.elapsed().as_millis()
            );
        }
        let weak = Arc::downgrade(input);
        let mut disabled = DisabledEndpoint::new(&device);
        wait_until("capture invalidation before removal", || input.failed());
        let removed = Instant::now();
        drop(inputs);
        assert!(weak.upgrade().is_none());
        assert!(
            removed.elapsed() < Duration::from_secs(2),
            "offline worker failed to stop promptly"
        );
        disabled.restore();
        thread::sleep(Duration::from_millis(750));
        assert!(weak.upgrade().is_none(), "removed capture was resurrected");
        eprintln!("PnP capture removal: worker joined; no resurrection");
    }
}
