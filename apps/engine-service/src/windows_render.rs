//! WASAPI hardware rendering. COM and endpoint buffers stay on their owning worker;
//! the engine only writes to a bounded SPSC ring and never waits for a device.

#[cfg(test)]
use super::ENGINE_SAMPLE_RATE;
use super::{ACTIVATION_TIMEOUT, SpscSampleRing, clock_bridge};
use std::{
    collections::BTreeMap,
    mem::size_of,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};
use tonenari_audio_engine::RuntimeCounters;
use tonenari_audio_graph::{AudioDeviceOutputConfig, DeviceSelector};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Media::{
            Audio::{
                AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
                WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0, eConsole, eRender,
            },
            Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        },
        System::{
            Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize,
            },
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
    core::PCWSTR,
};

pub(super) type RenderCache = BTreeMap<String, (AudioDeviceOutputConfig, Weak<DeviceRender>)>;
const RENDER_RING_FRAMES: u32 = 4096;

#[cfg(test)]
pub(super) fn prepare_outputs(
    requests: &[(String, AudioDeviceOutputConfig)],
    cache: &mut RenderCache,
    counters: &Arc<RuntimeCounters>,
) -> Result<BTreeMap<String, Arc<DeviceRender>>, String> {
    prepare_outputs_at_rate(requests, cache, counters, ENGINE_SAMPLE_RATE)
}

pub(super) fn prepare_outputs_at_rate(
    requests: &[(String, AudioDeviceOutputConfig)],
    cache: &mut RenderCache,
    counters: &Arc<RuntimeCounters>,
    graph_rate: u32,
) -> Result<BTreeMap<String, Arc<DeviceRender>>, String> {
    let mut outputs = BTreeMap::new();
    for (id, config) in requests {
        let reusable = cache
            .get(id)
            .filter(|(old, _)| old == config)
            .and_then(|(_, stream)| stream.upgrade())
            .filter(|stream| stream.graph_rate == graph_rate);
        let stream = match reusable {
            Some(stream) => stream,
            None => Arc::new(DeviceRender::open(config.clone(), counters, graph_rate)?),
        };
        outputs.insert(id.clone(), stream);
    }
    // Commit weak cache entries only after every endpoint was successfully prepared.
    cache.clear();
    for (id, config) in requests {
        cache.insert(id.clone(), (config.clone(), Arc::downgrade(&outputs[id])));
    }
    Ok(outputs)
}

pub(super) struct DeviceRender {
    ring: Arc<SpscSampleRing>,
    channels: usize,
    graph_rate: u32,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    demand: Arc<RenderDemand>,
    master: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

/// The native worker publishes only completed render-buffer transfers. The
/// graph owns the cursor and is the sole producer of the corresponding ring.
/// Notifications may coalesce; the cumulative counter cannot lose frames.
struct RenderDemand {
    id: AtomicU64,
    frames: AtomicU64,
    native_rate: std::sync::atomic::AtomicU32,
    consumer: OnceLock<thread::Thread>,
    #[cfg(test)]
    restart: AtomicBool,
}

impl RenderDemand {
    fn new() -> Self {
        Self {
            id: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            native_rate: std::sync::atomic::AtomicU32::new(0),
            consumer: OnceLock::new(),
            #[cfg(test)]
            restart: AtomicBool::new(false),
        }
    }
    fn begin_connection(&self) {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        // Keep cumulative demand, but identify the new native clock epoch.
        // The failed flag remains set until connected() publishes this epoch.
        self.id
            .store(NEXT_ID.fetch_add(1, Ordering::Relaxed), Ordering::Release);
    }

    fn publish(&self, frames: u32) {
        if frames == 0 {
            return;
        }
        self.frames.fetch_add(u64::from(frames), Ordering::Release);
        if let Some(consumer) = self.consumer.get() {
            consumer.unpark();
        }
    }
}

impl DeviceRender {
    fn open(
        config: AudioDeviceOutputConfig,
        counters: &Arc<RuntimeCounters>,
        graph_rate: u32,
    ) -> Result<Self, String> {
        if config.input_channels == 0 || config.input_channels > 64 {
            return Err("invalid render channel count".into());
        }
        let channels = usize::from(config.input_channels);
        let ring = Arc::new(SpscSampleRing::new(channels * RENDER_RING_FRAMES as usize));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let demand = Arc::new(RenderDemand::new());
        let master = Arc::new(AtomicBool::new(false));
        let worker_master = master.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let (worker_ring, worker_stop, worker_failed) =
            (ring.clone(), stop.clone(), failed.clone());
        let (worker_active, worker_counters) = (active.clone(), counters.clone());
        let worker_demand = demand.clone();
        let worker = thread::Builder::new()
            .name("tonenari-wasapi-render".into())
            .spawn(move || {
                let context = RenderWorker {
                    ring: &worker_ring,
                    stop: &worker_stop,
                    active: &worker_active,
                    counters: &worker_counters,
                    demand: &worker_demand,
                    master: &worker_master,
                    graph_rate,
                };
                tonenari_engine_service::device_worker::supervise(
                    &worker_stop,
                    &worker_failed,
                    &tx,
                    |connected| run(&config, &context, connected),
                );
            })
            .map_err(|error| error.to_string())?;
        let mut result = Self {
            ring,
            channels,
            graph_rate,
            stop,
            failed,
            active,
            demand,
            master,
            worker: Some(worker),
        };
        match rx.recv_timeout(ACTIVATION_TIMEOUT) {
            Ok(Ok(())) => Ok(result),
            error => {
                result.stop.store(true, Ordering::Release);
                // A timed-out synchronous driver call cannot be cancelled safely. Detach only
                // this failed startup; the worker owns all resources and releases them on return.
                if error.is_err() {
                    result.worker.take();
                }
                Err(format!("WASAPI output initialization failed: {error:?}"))
            }
        }
    }

    pub(super) fn write(&self, source: &[f32]) -> u32 {
        if self.failed() {
            return 0;
        }
        let written = self.ring.push(source, self.channels);
        self.active.store(true, Ordering::Release);
        // Graph ProcessReport counts a short sink transfer exactly once.
        u32::try_from(written / self.channels).expect("bounded ring frame count fits u32")
    }
    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(super) fn set_master(&self, master: bool) {
        self.master.store(master, Ordering::Release);
    }

    pub(super) fn native_rate(&self) -> Option<u32> {
        let rate = self.demand.native_rate.load(Ordering::Acquire);
        (!self.failed() && rate != 0).then_some(rate)
    }

    #[cfg(test)]
    pub(super) fn written_frames(&self) -> u64 {
        self.ring.write.load(Ordering::Acquire) / self.channels as u64
    }

    pub(super) fn next_cycle(
        &self,
        cursor: &mut tonenari_engine_service::render_demand::RenderDemandCursor,
    ) -> Option<tonenari_audio_io::process::ProcessCycle> {
        if self.failed() {
            return None;
        }
        // Registration is once per prepared stream, outside DSP processing.
        self.demand.consumer.get_or_init(thread::current);
        self.set_master(true);
        cursor.observe(
            self.demand.id.load(Ordering::Acquire),
            self.demand.frames.load(Ordering::Acquire),
            self.graph_rate,
            RENDER_RING_FRAMES,
        )
    }
}

impl Drop for DeviceRender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

pub(super) struct ComApartment;
impl ComApartment {
    pub(super) fn new() -> Result<Self, String> {
        // A successful S_OK or S_FALSE initialization must have one matching
        // uninitialize on this thread. A conflicting apartment is not success.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
            .map_err(|error| error.to_string())?;
        Ok(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct RenderStream {
    channels: u16,
    rate: u32,
    client: IAudioClient,
    render: IAudioRenderClient,
    capacity: u32,
    buffer_ready: BufferReadyEvent,
}
impl Drop for RenderStream {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

struct RenderWorker<'a> {
    ring: &'a SpscSampleRing,
    stop: &'a AtomicBool,
    active: &'a AtomicBool,
    counters: &'a RuntimeCounters,
    demand: &'a RenderDemand,
    master: &'a AtomicBool,
    graph_rate: u32,
}

/// The WASAPI worker owns the event for the complete stream lifetime. The
/// system signals buffer readiness; an unrelated timer never schedules writes.
pub(super) struct BufferReadyEvent(HANDLE);

impl BufferReadyEvent {
    pub(super) fn new(client: &IAudioClient) -> Result<Self, String> {
        // SAFETY: unnamed auto-reset event; no borrowed security/name storage.
        let event =
            Self(unsafe { CreateEventW(None, false, false, None) }.map_err(|e| e.to_string())?);
        // Initialize(EVENTCALLBACK) precedes this call, Start follows it.
        unsafe { client.SetEventHandle(event.0) }.map_err(|e| e.to_string())?;
        Ok(event)
    }

    pub(super) fn wait(&self) -> Result<bool, String> {
        // Bounded wait lets the owner observe shutdown even if the device stops
        // signaling. Timeout is not permission to render an unrequested block.
        match unsafe { WaitForSingleObject(self.0, 50) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err("WASAPI buffer-ready event wait failed".into()),
        }
    }
}

impl Drop for BufferReadyEvent {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner; RenderStream stops its client first.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn run(
    config: &AudioDeviceOutputConfig,
    context: &RenderWorker<'_>,
    connected: &mut dyn FnMut(),
) -> Result<(), String> {
    let RenderWorker {
        ring,
        stop,
        active,
        counters,
        demand,
        master,
        graph_rate,
    } = *context;
    // SAFETY: this dedicated worker owns its COM apartment and all interfaces below.
    let _apartment = ComApartment::new()?;
    let stream = open_stream(config)?;
    let mut bridge = clock_bridge::RenderBridge::new(
        usize::from(config.input_channels),
        graph_rate,
        stream.rate,
        ring,
    )?;
    let mut mapper = tonenari_audio_io::frame_budget::FrameBudgetMapper::new(graph_rate)
        .map_err(|error| error.to_string())?;
    let mut native_position = 0;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    // Only this native worker consumes the output ring. Never replay queued
    // pre-disconnect audio into a newly opened endpoint.
    ring.discard();
    // Prime with silence. The graph has not published this prepared stream yet.
    fill_available(&stream, ring, &mut bridge, master, active, counters)?;
    unsafe { stream.client.Start() }.map_err(|e| e.to_string())?;
    demand.begin_connection();
    demand.native_rate.store(stream.rate, Ordering::Release);
    connected();
    while !stop.load(Ordering::Acquire) {
        #[cfg(test)]
        if demand.restart.swap(false, Ordering::AcqRel) {
            return Err("native reconnect acceptance injection".into());
        }
        if stream.buffer_ready.wait()? && !stop.load(Ordering::Acquire) {
            let frames = fill_available(&stream, ring, &mut bridge, master, active, counters)?;
            // Refill one native quantum ahead, without a second free-running
            // graph clock. The initial silence prime is not graph demand.
            if frames > 0 {
                let cycle = mapper
                    .map(tonenari_audio_io::process::ProcessCycle {
                        clock: tonenari_audio_io::process::ClockDomain {
                            authority: tonenari_audio_io::session::ClockAuthority::Device,
                            id: demand.id.load(Ordering::Acquire),
                            epoch: 0,
                        },
                        sample_rate: stream.rate,
                        frames,
                        first_frame: native_position,
                        monotonic_ns: None,
                        discontinuity: native_position == 0,
                    })
                    .map_err(|error| error.to_string())?;
                native_position += u64::from(frames);
                demand.publish(cycle.frames);
            }
        } else {
            // Unplug may stop event delivery entirely. Probe the native client
            // on timeouts so invalidation cannot leave an apparently healthy
            // graph permanently waiting for a notification that never arrives.
            unsafe { stream.client.GetCurrentPadding() }.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn open_stream(config: &AudioDeviceOutputConfig) -> Result<RenderStream, String> {
    config
        .selector
        .resolve(
            &tonenari_audio_io::windows::devices().map_err(|e| e.to_string())?,
            tonenari_audio_io::DeviceDirection::Output,
            config.input_channels,
        )
        .map_err(|e| e.to_string())?;
    // SAFETY: only this MTA worker creates/accesses these COM interfaces. Borrowed UTF-16
    // and format storage survive their synchronous calls; GetMixFormat is always freed.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let device = match &config.selector {
            DeviceSelector::DefaultOutput => enumerator.GetDefaultAudioEndpoint(eRender, eConsole),
            DeviceSelector::PersistentUid { uid } => {
                let uid: Vec<u16> = uid.encode_utf16().chain(Some(0)).collect();
                enumerator.GetDevice(PCWSTR(uid.as_ptr()))
            }
            DeviceSelector::DefaultInput => {
                return Err("an input selector cannot render audio".into());
            }
        }
        .map_err(|e| e.to_string())?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        let mix = client.GetMixFormat().map_err(|e| e.to_string())?;
        if mix.is_null() {
            return Err("device returned no mix format".into());
        }
        let native_channels = (*mix).nChannels;
        let rate = (*mix).nSamplesPerSec;
        CoTaskMemFree(Some(mix.cast()));
        if native_channels < config.input_channels || native_channels > 64 {
            return Err(
                "device channel count changed; refresh the output node (no implicit downmix)"
                    .into(),
            );
        }
        if !(8_000..=384_000).contains(&rate) {
            return Err("native output sample rate is outside the prepared SRC contract".into());
        }
        let format = float_format_at_rate(native_channels, rate);
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
            return Err("native render quantum exceeds the prepared transport capacity".into());
        }
        let render = client
            .GetService::<IAudioRenderClient>()
            .map_err(|e| e.to_string())?;
        let buffer_ready = BufferReadyEvent::new(&client)?;
        Ok(RenderStream {
            channels: native_channels,
            rate,
            client,
            render,
            capacity,
            buffer_ready,
        })
    }
}

pub(super) fn float_format_at_rate(channels: u16, sample_rate: u32) -> WAVEFORMATEXTENSIBLE {
    let block_align = channels * 4;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: 0xfffe,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: 32,
            cbSize: u16::try_from(size_of::<WAVEFORMATEXTENSIBLE>() - size_of::<WAVEFORMATEX>())
                .expect("format size fits u16"),
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: 32,
        },
        dwChannelMask: match channels {
            1 => 4,
            2 => 3,
            _ => 0,
        },
        SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    }
}

#[expect(
    clippy::cast_ptr_alignment,
    reason = "WASAPI returns storage aligned for the negotiated Float32 format"
)]
fn fill_available(
    stream: &RenderStream,
    ring: &SpscSampleRing,
    bridge: &mut clock_bridge::RenderBridge,
    master: &AtomicBool,
    active: &AtomicBool,
    counters: &RuntimeCounters,
) -> Result<u32, String> {
    // SAFETY: the render client owns a buffer of exactly `available * channels` Float32
    // samples. No fallible operation occurs between GetBuffer and ReleaseBuffer.
    unsafe {
        let padding = stream
            .client
            .GetCurrentPadding()
            .map_err(|e| e.to_string())?;
        let available = stream.capacity.saturating_sub(padding);
        if available == 0 {
            return Ok(0);
        }
        let samples = (available as usize)
            .checked_mul(usize::from(stream.channels))
            .ok_or("render size overflow")?;
        let buffer = stream
            .render
            .GetBuffer(available)
            .map_err(|e| e.to_string())?;
        if buffer.is_null() {
            let _ = stream.render.ReleaseBuffer(available, 2);
            return Err("null WASAPI render buffer".into());
        }
        let was_steady = bridge.steady();
        let read = bridge.fill(
            std::slice::from_raw_parts_mut(buffer.cast::<f32>(), samples),
            usize::from(stream.channels),
            ring,
            master.load(Ordering::Acquire),
        );
        // Return the native buffer even when bounded SRC/feedback rejects a
        // packet. fill() zeroes it first, so the failure path cannot play junk.
        stream
            .render
            .ReleaseBuffer(available, 0)
            .map_err(|e| e.to_string())?;
        if read? < available as usize && was_steady && active.load(Ordering::Acquire) {
            counters.record_underrun();
        }
        Ok(available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn native_float_format_retains_requested_rate_and_channel_stride() {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000, 384_000] {
            for channels in [1, 2, 16, 64] {
                let format = float_format_at_rate(channels, rate).Format;
                let actual_rate = format.nSamplesPerSec;
                let bytes = format.nAvgBytesPerSec;
                let stride = format.nBlockAlign;
                assert_eq!(actual_rate, rate);
                assert_eq!(stride, channels * 4);
                assert_eq!(bytes, rate * u32::from(stride));
            }
        }
    }
    #[test]
    fn failed_render_does_not_schedule_stale_native_demand() {
        let output = DeviceRender {
            ring: Arc::new(SpscSampleRing::new(1024)),
            channels: 2,
            graph_rate: ENGINE_SAMPLE_RATE,
            stop: Arc::default(),
            failed: Arc::default(),
            active: Arc::default(),
            demand: Arc::new(RenderDemand::new()),
            master: Arc::default(),
            worker: None,
        };
        let mut cursor = tonenari_engine_service::render_demand::RenderDemandCursor::default();
        output.demand.begin_connection();
        assert!(output.next_cycle(&mut cursor).is_none());
        output.demand.publish(480);
        assert_eq!(output.next_cycle(&mut cursor).unwrap().frames, 480);
        output.failed.store(true, Ordering::Release);
        output.demand.publish(480);
        assert!(output.next_cycle(&mut cursor).is_none());
        output.demand.begin_connection();
        output.failed.store(false, Ordering::Release);
        assert!(output.next_cycle(&mut cursor).is_none());
        output.demand.publish(480);
        assert_eq!(output.next_cycle(&mut cursor).unwrap().frames, 480);
    }
    #[test]
    #[ignore = "requires authorized interactive Windows VM; writes silence only"]
    fn native_wasapi_output_recovers_with_new_clock_epoch() {
        assert_eq!(std::env::var("TONENARI_LAB_AUDIO_TEST").as_deref(), Ok("1"));
        let mut cache = RenderCache::new();
        let requests = vec![(
            "probe".into(),
            AudioDeviceOutputConfig {
                selector: DeviceSelector::DefaultOutput,
                input_channels: 2,
            },
        )];
        let outputs = prepare_outputs(&requests, &mut cache, &Arc::default()).unwrap();
        let output = &outputs["probe"];
        let mut cursor = tonenari_engine_service::render_demand::RenderDemandCursor::default();
        for cycle in 0..4 {
            let previous = output.demand.id.load(Ordering::Acquire);
            output.demand.restart.store(true, Ordering::Release);
            let start = Instant::now();
            loop {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "native render failed to reopen"
                );
                if output.demand.id.load(Ordering::Acquire) > previous && !output.failed() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let start = Instant::now();
            while output.next_cycle(&mut cursor).is_none() {
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "native render demand stalled"
                );
                thread::sleep(Duration::from_millis(5));
            }
            assert!(output.write(&[0.0; 512]) > 0);
            let reused = prepare_outputs(&requests, &mut cache, &Arc::default()).unwrap();
            assert!(Arc::ptr_eq(output, &reused["probe"]));
            eprintln!(
                "native render recovery={cycle} clock={} same graph owner",
                output.demand.id.load(Ordering::Acquire)
            );
        }
        let weak = Arc::downgrade(output);
        drop(outputs);
        assert!(
            weak.upgrade().is_none(),
            "removed output retained its worker"
        );
    }

    #[test]
    #[ignore = "requires elevated interactive authorized VM; temporarily disables LAB output or its authorized parent"]
    fn native_wasapi_output_pnp_recovery_and_offline_removal() {
        use super::super::pnp_tests::{DisabledEndpoint, endpoint, wait_until};
        let device = endpoint(false);
        let mut cache = RenderCache::new();
        let requests = vec![(
            "pnp-output".into(),
            AudioDeviceOutputConfig {
                selector: DeviceSelector::PersistentUid {
                    uid: device.uid.clone(),
                },
                input_channels: u16::try_from(device.output_channels).unwrap(),
            },
        )];
        let outputs = prepare_outputs(&requests, &mut cache, &Arc::default()).unwrap();
        let output = &outputs["pnp-output"];
        let samples = vec![0.0; output.channels * 256];
        let mut cursor = tonenari_engine_service::render_demand::RenderDemandCursor::default();
        for cycle in 0..4 {
            let before = output.demand.id.load(Ordering::Acquire);
            let mut disabled = DisabledEndpoint::new(&device);
            wait_until("render native invalidation", || output.failed());
            assert!(
                output.next_cycle(&mut cursor).is_none(),
                "failed device drove the graph clock"
            );
            let start = Instant::now();
            disabled.restore();
            wait_until("render native reappearance", || {
                !output.failed() && output.demand.id.load(Ordering::Acquire) > before
            });
            wait_until("new native render demand after PnP enable", || {
                output.next_cycle(&mut cursor).is_some()
            });
            assert!(output.write(&samples) > 0);
            let reused = prepare_outputs(&requests, &mut cache, &Arc::default()).unwrap();
            assert!(Arc::ptr_eq(output, &reused["pnp-output"]));
            eprintln!(
                "PnP render cycle={cycle} clock={} reenable_to_demand_ms={} same owner",
                output.demand.id.load(Ordering::Acquire),
                start.elapsed().as_millis()
            );
        }
        let weak = Arc::downgrade(output);
        let mut disabled = DisabledEndpoint::new(&device);
        wait_until("render invalidation before removal", || output.failed());
        let removed = Instant::now();
        drop(outputs);
        assert!(weak.upgrade().is_none());
        assert!(
            removed.elapsed() < Duration::from_secs(2),
            "offline worker failed to stop promptly"
        );
        disabled.restore();
        thread::sleep(Duration::from_millis(750));
        assert!(weak.upgrade().is_none(), "removed render was resurrected");
        eprintln!("PnP render removal: worker joined; no resurrection");
    }
}
