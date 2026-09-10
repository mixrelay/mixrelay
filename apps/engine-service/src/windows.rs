#![allow(
    clippy::inline_always,
    clippy::ref_as_ptr,
    reason = "the windows-rs implement macro generates these patterns"
)]

use std::{
    collections::BTreeMap,
    ffi::c_void,
    mem::{ManuallyDrop, size_of},
    ptr,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, ExternalIo, PeakMeterBank, ProcessorExchange, RenderPlan,
};
#[cfg(test)]
use mixrelay_audio_graph::ApplicationSelector;
use mixrelay_audio_graph::{
    AUDIO_PORT_ID, ApplicationCaptureRequest, CompileContext, CompileLimits, GraphCompiler,
    NodeConfig, RenderFormat,
};
use mixrelay_audio_io::process::{CycleBlocks, ProcessCycle, TransferOutcome};
use mixrelay_audio_io::session::{AudioBackendKind, CURRENT_TIMER_FORMAT};
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities, VirtualEndpointSpec,
};
use mixrelay_control_server::router_with_backend;
use mixrelay_engine_service::resource_requirements::ResourceRequirements;
use mixrelay_engine_service::{
    EngineGraphController, EngineGraphControllerConfig, GraphResourceBackend,
    default_peak_meter_bank, session::AdapterRuntimeStatus,
};
use mixrelay_processor_host::ProcessorCatalog;
use windows::{
    Win32::{
        Media::{
            Audio::{
                AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
                AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
                AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
                AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
                ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
                IActivateAudioInterfaceCompletionHandler,
                IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
            },
            Multimedia::WAVE_FORMAT_IEEE_FLOAT,
        },
        System::Com::{
            BLOB,
            StructuredStorage::{PROPVARIANT, PROPVARIANT_0_0},
        },
        System::Variant::VT_BLOB,
    },
    core::{HRESULT, IUnknown, Interface, Ref, implement},
};

#[path = "windows_application_capture.rs"]
mod application_capture;
#[cfg(test)]
#[path = "windows_application_signal_tests.rs"]
mod application_signal_tests;
#[path = "windows_clock_bridge.rs"]
mod clock_bridge;
#[path = "windows_format.rs"]
mod format_control;
#[path = "windows_input.rs"]
mod input;
#[cfg(test)]
#[path = "windows_pnp_tests.rs"]
mod pnp_tests;
#[cfg(test)]
#[path = "windows_recovery_tests.rs"]
mod recovery_tests;
#[path = "windows_render.rs"]
mod render;
#[path = "windows_transport.rs"]
mod transport;

fn lab_bridge_enabled() -> bool {
    std::env::var("MIXRELAY_WINDOWS_SYSVAD_LAB").as_deref() == Ok("1")
}

const ENGINE_BLOCK_FRAMES: u32 = CURRENT_TIMER_FORMAT.block_frames;
fn adapter_profile() -> mixrelay_engine_service::session::AdapterProfile {
    mixrelay_engine_service::session::AdapterProfile {
        initial_format: mixrelay_audio_io::session::CURRENT_TIMER_FORMAT,
        capabilities: mixrelay_audio_io::session::AudioSessionCapabilities {
            backend: AudioBackendKind::Wasapi,
            supported_sample_rates: format_control::GRAPH_RATES.to_vec(),
            supported_block_frames: vec![ENGINE_BLOCK_FRAMES],
            can_reconfigure_live: true,
            hardware_input: true,
            hardware_output: true,
            virtual_endpoint: lab_bridge_enabled(),
            application_capture: true,
            sample_rate_conversion: false,
            drift_correction: false,
        },
        application_capture_rates: format_control::GRAPH_RATES.to_vec(),
        clock_authority: mixrelay_audio_io::session::ClockAuthority::EngineTimer,
        transport_latency_frames: None,
        requires_observed_device_rate: false,
    }
}
const ENGINE_SAMPLE_RATE: u32 = CURRENT_TIMER_FORMAT.sample_rate;
const MAX_ENDPOINT_CHANNELS: u16 = 64;
const CONTROL_SERVER_PORT: u16 = 19_341;
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn run() -> Result<(), String> {
    let meters = default_peak_meter_bank();
    let mut resources = WindowsGraphResources::default();
    // Restore only the accepted graph format. Native clients independently query
    // actual mix formats; this must not pretend to reconfigure the fixed LAB driver.
    let format = Arc::get_mut(&mut resources.runtime_status)
        .expect("startup owns the runtime status")
        .restore_preferences();
    resources
        .active_spec
        .lock()
        .map_err(|_| "format state poisoned")?
        .graph_rate = format.sample_rate;
    let processor = create_initial_processor_at_format(Arc::clone(&meters), format)?;
    let exchange = Arc::new(ProcessorExchange::new(processor.plan().revision()));
    let resources = Arc::new(resources);
    let endpoint_backend = Arc::new(WindowsVirtualEndpointBackend::default());
    let controller = Arc::new(
        EngineGraphController::new_with_initial_document(
            Arc::clone(&exchange),
            EngineGraphControllerConfig {
                format,
                meters,
                initial_bindings: processor.plan().meter_bindings().to_vec(),
                endpoint_backend,
                required_endpoint_node_id: None,
                processor_catalog: ProcessorCatalog::discover(),
                graph_resources: resources.clone(),
                runtime_status: resources.clone(),
            },
            mixrelay_audio_graph::GraphDocument::new(1, vec![], vec![]),
        )
        .with_graph_catalog(),
    );

    spawn_reclaimer(&exchange, &resources)?;
    let worker_status = resources.runtime_status.clone();
    let audio_thread = thread::Builder::new()
        .name("mixrelay-wasapi-audio".to_owned())
        .spawn(move || run_audio_worker(processor, exchange, resources))
        .map_err(|error| format!("could not start WASAPI audio worker: {error}"))?;
    let _worker = mixrelay_engine_service::session::AudioWorker::new(worker_status, audio_thread);
    let maintenance = controller.start_control_maintenance()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not initialize Windows async runtime: {error}"))?;
    let result = runtime.block_on(async move {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], CONTROL_SERVER_PORT));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| format!("could not bind local control server: {error}"))?;
        axum::serve(listener, router_with_backend(controller))
            .with_graceful_shutdown(async {
                let _ = mixrelay_engine_service::session::shutdown_signal().await;
            })
            .await
            .map_err(|error| format!("local control server failed: {error}"))
    });
    result.and(maintenance.finish())
}

#[cfg(test)]
fn create_initial_processor(meters: Arc<PeakMeterBank>) -> Result<Box<BlockProcessor>, String> {
    create_initial_processor_at_format(meters, render_format())
}

fn create_initial_processor_at_format(
    meters: Arc<PeakMeterBank>,
    format: RenderFormat,
) -> Result<Box<BlockProcessor>, String> {
    let execution = GraphCompiler::default()
        .compile(
            // No installed virtual driver: start with a valid empty graph, not silent sinks.
            &mixrelay_audio_graph::GraphDocument::new(1, vec![], vec![]),
            CompileContext {
                format,
                limits: CompileLimits::default(),
            },
        )
        .map_err(|error| error.to_string())?;
    let render = RenderPlan::prepare(&execution).map_err(|error| error.to_string())?;
    Ok(Box::new(BlockProcessor::with_peak_meters(
        Arc::new(render),
        meters,
    )))
}

#[cfg(test)]
const fn render_format() -> RenderFormat {
    RenderFormat {
        sample_rate: ENGINE_SAMPLE_RATE,
        block_frames: ENGINE_BLOCK_FRAMES,
    }
}

fn spawn_reclaimer(
    exchange: &Arc<ProcessorExchange>,
    resources: &Arc<WindowsGraphResources>,
) -> Result<(), String> {
    let weak_exchange = Arc::downgrade(exchange);
    let weak_resources = Arc::downgrade(resources);
    thread::Builder::new()
        .name("mixrelay-windows-reclaimer".to_owned())
        .spawn(move || {
            loop {
                let Some(exchange) = weak_exchange.upgrade() else {
                    break;
                };
                exchange.reclaim_retired();
                drop(exchange);
                let Some(resources) = weak_resources.upgrade() else {
                    break;
                };
                resources.reclaim_retired();
                drop(resources);
                thread::sleep(Duration::from_millis(100));
            }
        })
        .map(|_| ())
        .map_err(|error| format!("could not start Windows resource reclaimer: {error}"))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the non-returning worker owns both Arcs for the complete audio lifetime"
)]
fn run_audio_worker(
    mut processor: Box<BlockProcessor>,
    exchange: Arc<ProcessorExchange>,
    resources: Arc<WindowsGraphResources>,
) {
    let Ok(_lease) = resources.runtime_status.begin_processing() else {
        return;
    };
    let mut active = Box::<WindowsCaptureSet>::default();
    let mut format = CURRENT_TIMER_FORMAT;
    let mut deadline = Instant::now();
    let mut timeline = mixrelay_audio_io::process::TimerTimeline::default();
    let mut demand = mixrelay_engine_service::render_demand::RenderDemandCursor::default();
    let mut device_driven = false;
    while !resources.runtime_status.stop_requested() {
        if resources.runtime_status.checkpoint() {
            thread::park_timeout(Duration::from_millis(1));
            continue;
        }
        // A busy retirement slot defers only the swap, never rendering of the active graph.
        let _ = exchange.activate_pending_configured(&mut processor, |revision| {
            resources.activate_pending(&mut active, revision)
        });
        resources
            .runtime_status
            .activate_format(processor.plan().format());
        let block_period = sync_clock_format(
            &mut format,
            processor.plan().format(),
            &mut timeline,
            &mut demand,
            &mut deadline,
        );
        // The mapped owner is reused across graph revisions. Only this audio
        // worker takes mutable access, never a control thread or meter reader.
        let mut virtual_io = match active.virtual_io.as_ref().map(|io| io.try_lock()) {
            Some(Ok(io)) => Some(io),
            Some(Err(_)) => {
                thread::park_timeout(Duration::from_millis(1));
                continue;
            }
            None => None,
        };
        // A stable output ordering chooses one pacing authority, not one clock
        // per sink. Secondary devices still need asynchronous drift correction.
        let primary = active.select_pacing_output(&resources, virtual_io.is_some());
        let cycle = if let Some(io) = virtual_io.as_mut() {
            if !resources.virtual_cycle_ready(io) {
                let wake = io.fault().is_none().then(|| io.waiter());
                drop(virtual_io);
                if let Some(wake) = wake {
                    wake.wait();
                } else {
                    thread::park_timeout(Duration::from_millis(50));
                }
                continue;
            }
            device_driven = true;
            resources.device_clock_active.store(true, Ordering::Release);
            let mut cycle = timeline.next_cycle(format);
            cycle.clock.authority = mixrelay_audio_io::session::ClockAuthority::Device;
            cycle.clock.id = 0x4d52;
            cycle
        } else if let Some(output) = primary {
            device_driven = true;
            resources.device_clock_active.store(true, Ordering::Release);
            let Some(cycle) = output.next_cycle(&mut demand) else {
                // Wakeups are hints. Timeout only checks shutdown/configuration;
                // without new native frames it never runs a DSP block.
                thread::park_timeout(Duration::from_millis(50));
                continue;
            };
            cycle
        } else {
            if device_driven {
                timeline.reset();
                demand.reset();
                deadline = Instant::now();
                device_driven = false;
            }
            resources
                .device_clock_active
                .store(false, Ordering::Release);
            timeline.next_cycle(format)
        };
        if let Err(error) = process_graph_cycle(
            &mut processor,
            &resources,
            &active,
            virtual_io.as_deref_mut(),
            cycle,
            format,
        ) {
            eprintln!("MixRelay WASAPI graph stopped: {error}");
            return;
        }
        if device_driven {
            continue;
        }
        deadline += block_period;
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline.duration_since(now));
        } else if now.duration_since(deadline) > block_period.saturating_mul(4) {
            deadline = now;
        }
    }
}

fn sync_clock_format(
    format: &mut mixrelay_audio_io::session::SessionFormat,
    prepared: RenderFormat,
    timeline: &mut mixrelay_audio_io::process::TimerTimeline,
    demand: &mut mixrelay_engine_service::render_demand::RenderDemandCursor,
    deadline: &mut Instant,
) -> Duration {
    if prepared.sample_rate != format.sample_rate || prepared.block_frames != format.block_frames {
        *format = mixrelay_audio_io::session::SessionFormat {
            sample_rate: prepared.sample_rate,
            block_frames: prepared.block_frames,
        };
        timeline.reset();
        demand.reset();
        *deadline = Instant::now();
    }
    Duration::from_secs_f64(f64::from(format.block_frames) / f64::from(format.sample_rate))
}

fn process_graph_cycle(
    processor: &mut BlockProcessor,
    resources: &WindowsGraphResources,
    active: &WindowsCaptureSet,
    mut virtual_io: Option<&mut transport::Transport>,
    cycle: ProcessCycle,
    format: mixrelay_audio_io::session::SessionFormat,
) -> Result<(), String> {
    for block in CycleBlocks::new(cycle, format).map_err(|error| error.to_string())? {
        let report = processor
            .process_external_cycle(
                block,
                &mut WindowsExternalIo {
                    application_captures: active,
                    virtual_io: virtual_io.as_deref_mut(),
                },
            )
            .map_err(|error| error.to_string())?;
        resources.runtime_status.record_process(report);
    }
    Ok(())
}

struct WindowsExternalIo<'a> {
    application_captures: &'a WindowsCaptureSet,
    virtual_io: Option<&'a mut transport::Transport>,
}

impl ExternalIo for WindowsExternalIo<'_> {
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if self.application_captures.endpoint_node.as_deref() == Some(binding.node_id().as_str())
            && binding.port_id().as_str() == mixrelay_audio_graph::FROM_OS_PORT_ID
        {
            return TransferOutcome::complete(
                self.virtual_io
                    .as_mut()
                    .map_or(0, |io| io.read(destination)),
            );
        }
        if binding.port_id().as_str() == AUDIO_PORT_ID {
            return self
                .application_captures
                .read(binding.node_id().as_str(), destination);
        }
        TransferOutcome::complete(0)
    }

    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if self.application_captures.endpoint_node.as_deref() == Some(binding.node_id().as_str())
            && binding.port_id().as_str() == mixrelay_audio_graph::TO_OS_PORT_ID
        {
            return TransferOutcome::complete(
                self.virtual_io.as_mut().map_or(0, |io| io.write(source)),
            );
        }
        if let Some(output) = self
            .application_captures
            .outputs
            .get(binding.node_id().as_str())
        {
            return TransferOutcome::complete(output.write(source));
        }
        TransferOutcome::complete(0)
    }
}

struct WindowsVirtualEndpointBackend {
    capabilities: VirtualEndpointCapabilities,
}

impl Default for WindowsVirtualEndpointBackend {
    fn default() -> Self {
        Self {
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: 2,
                max_capture_channels: 1,
                supported_sample_rates: vec![ENGINE_SAMPLE_RATE],
                supports_duplex: true,
                supports_dynamic_creation: false,
            },
        }
    }
}

impl VirtualEndpointBackend for WindowsVirtualEndpointBackend {
    fn settings(&self) -> Option<mixrelay_audio_io::VirtualEndpointSettings> {
        // The diagnostic driver only accepts a fixed duplex LAB graph. Do not
        // expose generic independent-side creation as a supported product path.
        Some(mixrelay_audio_io::VirtualEndpointSettings {
            can_create: false,
            can_rename: false,
            render_channels: vec![2],
            capture_channels: vec![1],
            creation_template: None,
            devices: Vec::new(),
        })
    }

    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }

    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        if desired.is_empty() {
            return Ok(());
        }
        if lab_bridge_enabled() && desired.len() == 1 {
            let spec = &desired[0];
            if spec.id.as_str() == "mixrelay.sysvad.lab"
                && spec.display_name == "MixRelay SysVAD Bridge LAB"
                && spec.render.is_some_and(|s| s.channels == 2)
                && spec.capture.is_some_and(|s| s.channels == 1)
                && spec.default_sample_rate == 48_000
                && spec.supported_sample_rates == [48_000]
            {
                return Ok(());
            }
        }
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability:
                "Windows virtual endpoint unavailable: the explicit SysVAD LAB opt-in supports only its fixed stereo-render/mono-capture pair at 48 kHz; product configuration is not qualified"
                    .to_owned(),
        })
    }
}

struct WindowsGraphResources {
    runtime_status: Arc<AdapterRuntimeStatus>,
    device_clock_active: AtomicBool,
    virtual_clock_active: AtomicBool,
    virtual_failed: AtomicBool,
    active_spec: Arc<Mutex<WindowsResourceSpec>>,
    render_cache: Arc<Mutex<render::RenderCache>>,
    input_cache: Arc<Mutex<input::InputCache>>,
    capture_cache: Arc<Mutex<application_capture::CaptureCache>>,
    render_counters: Arc<mixrelay_audio_engine::RuntimeCounters>,
    virtual_cache: Mutex<std::sync::Weak<Mutex<transport::Transport>>>,
    virtual_overruns: AtomicU64,
    virtual_underruns: AtomicU64,
    mailbox: Arc<format_control::ResourceMailbox>,
}

impl Default for WindowsGraphResources {
    fn default() -> Self {
        Self {
            runtime_status: Arc::new(
                AdapterRuntimeStatus::new(adapter_profile()).with_graph_format_control(),
            ),
            device_clock_active: AtomicBool::new(false),
            virtual_clock_active: AtomicBool::new(false),
            virtual_failed: AtomicBool::new(false),
            active_spec: Arc::default(),
            render_cache: Arc::default(),
            input_cache: Arc::default(),
            capture_cache: Arc::default(),
            render_counters: Arc::default(),
            virtual_cache: Mutex::default(),
            virtual_overruns: AtomicU64::new(0),
            virtual_underruns: AtomicU64::new(0),
            mailbox: Arc::default(),
        }
    }
}

#[derive(PartialEq)]
struct WindowsResourceSpec {
    graph_rate: u32,
    inputs: Vec<(String, mixrelay_audio_graph::AudioDeviceInputConfig)>,
    captures: Vec<(String, ApplicationCaptureRequest)>,
    outputs: Vec<(String, mixrelay_audio_graph::AudioDeviceOutputConfig)>,
    endpoint: Option<(String, VirtualEndpointSpec)>,
}

impl Default for WindowsResourceSpec {
    fn default() -> Self {
        Self {
            graph_rate: ENGINE_SAMPLE_RATE,
            inputs: vec![],
            captures: vec![],
            outputs: vec![],
            endpoint: None,
        }
    }
}

impl mixrelay_engine_service::RuntimeStatusBackend for WindowsGraphResources {
    fn capabilities_for_format(
        &self,
        format: RenderFormat,
    ) -> Option<mixrelay_audio_io::session::AudioSessionCapabilities> {
        self.runtime_status.capabilities_for_format(format)
    }

    fn reconfigure(
        &self,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<(), HostAudioBackendError> {
        self.runtime_status.reconfigure(current, requested)
    }

    fn resume(&self) {
        self.runtime_status.resume();
    }

    fn status(&self, format: RenderFormat) -> mixrelay_control_protocol::AppStatus {
        let mut status = self.runtime_status.status(format);
        let processing_failed = status.audio_session.as_ref().is_some_and(|session| {
            session.issue == Some(mixrelay_audio_io::session::SessionIssue::ProcessingFailed)
        });
        if self.device_clock_active.load(Ordering::Acquire)
            && let Some(session) = &mut status.audio_session
        {
            session.clock_authority = mixrelay_audio_io::session::ClockAuthority::Device;
            if self.virtual_clock_active.load(Ordering::Acquire) {
                session.transport_latency_frames = Some(
                    u32::try_from(transport::STARTUP_SILENCE_FRAMES)
                        .expect("bounded startup queue"),
                );
            }
            // Pacing authority is not proof that every independent capture and
            // secondary output clock is synchronized. Preserve that warning.
        }
        let counters = self.render_counters.snapshot();
        status.underruns = status.underruns.saturating_add(counters.underruns);
        status.overruns = status.overruns.saturating_add(counters.overruns);
        // A busy audio cycle must not make counters transiently appear as zero.
        status.overruns = status
            .overruns
            .saturating_add(self.virtual_overruns.load(Ordering::Acquire));
        status.underruns = status
            .underruns
            .saturating_add(self.virtual_underruns.load(Ordering::Acquire));
        let failed = self.render_cache.lock().map_or(true, |cache| {
            cache
                .values()
                .any(|(_, stream)| stream.upgrade().is_some_and(|stream| stream.failed()))
        });
        let input_failed = self.input_cache.lock().map_or(true, |cache| {
            cache
                .values()
                .any(|(_, stream)| stream.upgrade().is_some_and(|stream| stream.failed()))
        });
        if (failed || input_failed)
            && let Some(session) = &mut status.audio_session
        {
            session.issue = Some(mixrelay_audio_io::session::SessionIssue::DeviceUnavailable);
        }
        if let Some(issue) = self
            .capture_cache
            .lock()
            .ok()
            .and_then(|cache| application_capture::issue(&cache))
            && let Some(session) = &mut status.audio_session
            && session.issue != Some(mixrelay_audio_io::session::SessionIssue::ProcessingFailed)
        {
            session.issue = Some(issue);
        }
        if self.virtual_failed.load(Ordering::Acquire) {
            status.driver_state = mixrelay_control_protocol::DriverState::Unavailable;
            if let Some(session) = &mut status.audio_session {
                session.issue = Some(mixrelay_audio_io::session::SessionIssue::DeviceUnavailable);
                session.observed_device_sample_rate = None;
                session.transport_latency_frames = None;
            }
        }
        if processing_failed && let Some(session) = &mut status.audio_session {
            session.issue = Some(mixrelay_audio_io::session::SessionIssue::ProcessingFailed);
        }
        status
    }
}

impl WindowsGraphResources {
    fn virtual_cycle_ready(&self, io: &mut transport::Transport) -> bool {
        let (overruns, underruns) = io.counters();
        self.virtual_overruns.store(overruns, Ordering::Release);
        self.virtual_underruns.store(underruns, Ordering::Release);
        match io.ready(ENGINE_BLOCK_FRAMES) {
            Ok(ready) => {
                // Evidence is published only by the active owner, not by a staged graph.
                self.runtime_status.observe_endpoint(true);
                self.runtime_status
                    .observe_device_rate(Some(ENGINE_SAMPLE_RATE));
                self.virtual_clock_active.store(true, Ordering::Release);
                self.device_clock_active.store(true, Ordering::Release);
                ready
            }
            Err(error) => {
                if !self.virtual_failed.swap(true, Ordering::AcqRel) {
                    eprintln!(
                        "MixRelay virtual transport suspended: {error:?}; remove and reattach the endpoint"
                    );
                }
                self.virtual_clock_active.store(false, Ordering::Release);
                self.device_clock_active.store(false, Ordering::Release);
                self.runtime_status.forget_endpoint_observation();
                // No fake timer DSP and no worker exit. The caller releases the
                // mapping lock before its bounded configuration/shutdown wait.
                false
            }
        }
    }
    fn prepare_virtual(
        &self,
        node: &str,
        captures: &mut WindowsCaptureSet,
    ) -> Result<(), HostAudioBackendError> {
        if self.virtual_failed.load(Ordering::Acquire) {
            return Err(HostAudioBackendError::BackendFailure {
                message: "virtual transport is faulted; remove the endpoint, then attach it again"
                    .into(),
            });
        }
        let mut cache =
            self.virtual_cache
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "virtual cache poisoned".into(),
                })?;
        let io = if let Some(io) = cache.upgrade() {
            let usable = io
                .lock()
                .is_ok_and(|io| io.fault().is_none() && io.online());
            if !usable {
                return Err(HostAudioBackendError::BackendFailure {
                    message:
                        "virtual transport is faulted; remove the endpoint, then attach it again"
                            .into(),
                });
            }
            io
        } else {
            Arc::new(Mutex::new(transport::Transport::open_lab().map_err(
                |message| HostAudioBackendError::BackendFailure { message },
            )?))
        };
        *cache = Arc::downgrade(&io);
        captures.virtual_io = Some(io);
        captures.endpoint_node = Some(node.to_owned());
        Ok(())
    }
    fn activate_pending(
        &self,
        active: &mut Box<WindowsCaptureSet>,
        revision: u64,
    ) -> Result<(), &'static str> {
        if !self.mailbox.retired.load(Ordering::Acquire).is_null() {
            return Err("the previous WASAPI capture set is still being reclaimed");
        }
        let pending = self.mailbox.pending.load(Ordering::Acquire);
        if pending.is_null() {
            return Ok(());
        }
        // Only the audio worker removes this immutable owner. Publication may
        // precede the matching DSP plan; never activate another revision's I/O.
        if unsafe { &*pending }.revision != revision {
            return Err("Windows native resources await their matching graph revision");
        }
        let pending = self.mailbox.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: the audio worker atomically removed the only published owner.
        let next = unsafe { Box::from_raw(pending) };
        if next.virtual_io.is_none() {
            self.virtual_failed.store(false, Ordering::Release);
            self.virtual_clock_active.store(false, Ordering::Release);
            self.runtime_status.forget_endpoint_observation();
        }
        let previous = std::mem::replace(active, next);
        self.mailbox
            .retired
            .store(Box::into_raw(previous), Ordering::Release);
        Ok(())
    }

    fn reclaim_retired(&self) {
        let retired = self.mailbox.retired.swap(ptr::null_mut(), Ordering::AcqRel);
        if !retired.is_null() {
            // SAFETY: the reclaimer atomically removed the sole published owner.
            drop(unsafe { Box::from_raw(retired) });
        }
    }

    fn publish(&self, captures: WindowsCaptureSet) -> Result<(), HostAudioBackendError> {
        let _reservation = self.mailbox.reserve()?;
        let pointer = Box::into_raw(Box::new(captures));
        if self
            .mailbox
            .pending
            .compare_exchange(
                ptr::null_mut(),
                pointer,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            // SAFETY: publication failed, so this thread still owns the allocation.
            drop(unsafe { Box::from_raw(pointer) });
            return Err(HostAudioBackendError::BackendFailure {
                message: "a WASAPI capture update is already pending".to_owned(),
            });
        }
        Ok(())
    }
}

impl GraphResourceBackend for WindowsGraphResources {
    fn device_inventory(
        &self,
    ) -> Result<Vec<mixrelay_audio_io::AudioDeviceDescriptor>, HostAudioBackendError> {
        mixrelay_audio_io::windows::devices()
    }

    fn prepare_format_change(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<Box<dyn mixrelay_engine_service::PreparedGraphFormat>, HostAudioBackendError> {
        format_control::prepare(self, document, current, requested)
    }
    fn reconcile(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
    ) -> Result<(), HostAudioBackendError> {
        self.reconcile_devices(document, ResourceRequirements::from_document(document))
    }
}

impl WindowsGraphResources {
    fn reconcile_devices(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
        requirements: ResourceRequirements,
    ) -> Result<(), HostAudioBackendError> {
        validate_virtual_attachments(document, &requirements.endpoints)?;
        let next = WindowsResourceSpec {
            graph_rate: self
                .active_spec
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "WASAPI format state is poisoned".into(),
                })?
                .graph_rate,
            inputs: requirements.devices.inputs,
            captures: requirements.captures,
            outputs: requirements.devices.outputs,
            endpoint: requirements.endpoints.into_iter().next(),
        };
        let mut active =
            self.active_spec
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "WASAPI capture state is poisoned".to_owned(),
                })?;
        if next.endpoint.is_some() && next.graph_rate != ENGINE_SAMPLE_RATE {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "installed Windows LAB endpoint requires a 48 kHz graph".into(),
            });
        }
        if *active == next {
            return Ok(());
        }
        let mut next_capture_cache = self
            .capture_cache
            .lock()
            .map_err(|_| HostAudioBackendError::BackendFailure {
                message: "application capture cache poisoned".into(),
            })?
            .clone();
        let mut captures = WindowsCaptureSet {
            revision: document.revision,
            captures: application_capture::prepare_at_rate(
                &next.captures,
                &mut next_capture_cache,
                next.graph_rate,
            )
            .map_err(|message| HostAudioBackendError::BackendFailure { message })?,
            ..Default::default()
        };
        captures.inputs = input::prepare_inputs_at_rate(
            &next.inputs,
            &mut *self
                .input_cache
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "WASAPI input cache poisoned".into(),
                })?,
            next.graph_rate,
        )
        .map_err(|message| HostAudioBackendError::BackendFailure { message })?;
        let mut cache =
            self.render_cache
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "WASAPI render cache poisoned".into(),
                })?;
        captures.outputs = render::prepare_outputs_at_rate(
            &next.outputs,
            &mut cache,
            &self.render_counters,
            next.graph_rate,
        )
        .map_err(|message| HostAudioBackendError::BackendFailure { message })?;
        if let Some((node, _)) = &next.endpoint {
            self.prepare_virtual(node, &mut captures)?;
        }
        self.publish(captures)?;
        // Publish cache identity only after the complete graph resource transaction.
        // A later device preparation failure must not orphan the old capture owner.
        if let Ok(mut cache) = self.capture_cache.lock() {
            *cache = next_capture_cache;
        }
        *active = next;
        Ok(())
    }
}

fn validate_virtual_attachments(
    document: &mixrelay_audio_graph::GraphDocument,
    endpoints: &[(String, VirtualEndpointSpec)],
) -> Result<(), HostAudioBackendError> {
    WindowsVirtualEndpointBackend::default().prepare_graph_attachments(
        &endpoints
            .iter()
            .map(|(_, spec)| spec.clone())
            .collect::<Vec<_>>(),
    )?;
    if !endpoints.is_empty()
        && document.nodes.iter().any(|node| {
            matches!(
                node.config,
                NodeConfig::ApplicationCapture(_)
                    | NodeConfig::AudioDeviceOutput(_)
                    | NodeConfig::AudioDeviceInput(_)
            )
        })
    {
        return Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "LAB virtual-clock mixing with independent native devices is not qualified"
                .into(),
        });
    }
    Ok(())
}

#[derive(Default)]
struct WindowsCaptureSet {
    revision: u64,
    inputs: BTreeMap<String, Arc<input::DeviceInput>>,
    captures: BTreeMap<String, Arc<ProcessLoopbackCapture>>,
    outputs: BTreeMap<String, Arc<render::DeviceRender>>,
    virtual_io: Option<Arc<Mutex<transport::Transport>>>,
    endpoint_node: Option<String>,
}

impl WindowsCaptureSet {
    fn select_pacing_output(
        &self,
        resources: &WindowsGraphResources,
        virtual_clock: bool,
    ) -> Option<&Arc<render::DeviceRender>> {
        let primary = self.outputs.values().find(|output| !output.failed());
        if !virtual_clock {
            // Actual GetMixFormat observation, never the requested graph rate.
            resources
                .runtime_status
                .observe_device_rate(primary.and_then(|output| output.native_rate()));
        }
        for output in self.outputs.values() {
            output.set_master(
                !virtual_clock && primary.is_some_and(|first| Arc::ptr_eq(first, output)),
            );
        }
        primary
    }

    fn read(&self, node_id: &str, destination: &mut [f32]) -> TransferOutcome {
        destination.fill(0.0);
        if let Some(input) = self.inputs.get(node_id) {
            return input.read(destination);
        }
        if let Some(capture) = self.captures.get(node_id) {
            return capture.read(destination);
        }
        TransferOutcome::complete(0)
    }
}

struct ProcessLoopbackCapture {
    ring: Arc<SpscSampleRing>,
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU32>,
    epoch: Arc<AtomicU64>,
    consumed_epoch: AtomicU64,
    primed: AtomicBool,
    channels: usize,
    graph_rate: u32,
    worker: Option<thread::JoinHandle<()>>,
}

impl ProcessLoopbackCapture {
    #[cfg(test)]
    fn open(process_id: u32, include_descendants: bool, channels: u16) -> Result<Self, String> {
        Self::open_selector(
            &ApplicationSelector::ProcessId {
                process_id,
                include_descendants,
            },
            channels,
        )
    }

    #[cfg(test)]
    fn open_selector(selector: &ApplicationSelector, channels: u16) -> Result<Self, String> {
        let target = application_capture::Target::new(selector)?;
        Self::open_target(target, channels)
    }

    #[cfg(test)]
    fn open_target(target: application_capture::Target, channels: u16) -> Result<Self, String> {
        Self::open_target_at_rate(target, channels, ENGINE_SAMPLE_RATE)
    }

    fn open_target_at_rate(
        target: application_capture::Target,
        channels: u16,
        graph_rate: u32,
    ) -> Result<Self, String> {
        let channel_count = usize::from(channels);
        let sample_capacity = channel_count
            .checked_mul(clock_bridge::GRAPH_FIFO_FRAMES)
            .ok_or_else(|| "WASAPI capture ring size overflowed".to_owned())?;
        let ring = Arc::new(SpscSampleRing::new(sample_capacity));
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(AtomicU32::new(application_capture::UNAVAILABLE));
        let epoch = Arc::new(AtomicU64::new(0));
        if matches!(target, application_capture::Target::Reselection) {
            // A persisted PID cannot become valid through polling. It needs an
            // explicit replacement request, so retain no idle native worker.
            return Ok(Self {
                ring,
                stop,
                state,
                epoch,
                consumed_epoch: AtomicU64::new(0),
                primed: AtomicBool::new(false),
                channels: channel_count,
                graph_rate,
                worker: None,
            });
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker_ring = Arc::clone(&ring);
        let worker_stop = Arc::clone(&stop);
        let worker_state = Arc::clone(&state);
        let worker_epoch = Arc::clone(&epoch);
        let worker = thread::Builder::new()
            .name("mixrelay-wasapi-application".into())
            .spawn(move || {
                let config = CaptureWorkerConfig {
                    target,
                    channels,
                    graph_rate,
                    ring: worker_ring,
                    stop: worker_stop,
                    state: worker_state,
                    epoch: worker_epoch,
                };
                application_capture::supervise(&config, &ready_tx);
            })
            .map_err(|error| format!("could not start process capture worker: {error}"))?;
        match ready_rx.recv_timeout(ACTIVATION_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                ring,
                stop,
                state,
                epoch,
                consumed_epoch: AtomicU64::new(0),
                primed: AtomicBool::new(false),
                channels: channel_count,
                graph_rate,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(format!(
                    "WASAPI process capture did not initialize: {error}"
                ))
            }
        }
    }

    fn read(&self, destination: &mut [f32]) -> TransferOutcome {
        if self.state.load(Ordering::Acquire) != application_capture::ACTIVE {
            destination.fill(0.0);
            self.ring.discard();
            return application_capture::unknown_transfer();
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        if self.consumed_epoch.swap(epoch, Ordering::Relaxed) != epoch {
            self.ring.discard();
            self.primed.store(false, Ordering::Relaxed);
        }
        if !self.primed.load(Ordering::Relaxed) {
            if self.ring.available_frames(self.channels) < clock_bridge::TARGET_FRAMES {
                destination.fill(0.0);
                return application_capture::unknown_transfer();
            }
            self.primed.store(true, Ordering::Relaxed);
        }
        let samples = self.ring.pop(destination, self.channels);
        // A control-side disconnect/rebind cannot leak one last old block.
        if self.state.load(Ordering::Acquire) != application_capture::ACTIVE
            || self.epoch.load(Ordering::Acquire) != epoch
        {
            destination.fill(0.0);
            return application_capture::unknown_transfer();
        }
        if samples == 0 {
            application_capture::unknown_transfer()
        } else {
            TransferOutcome::complete(u32::try_from(samples / self.channels).unwrap_or(0))
        }
    }
}

impl Drop for ProcessLoopbackCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

struct CaptureWorkerConfig {
    target: application_capture::Target,
    channels: u16,
    graph_rate: u32,
    ring: Arc<SpscSampleRing>,
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU32>,
    epoch: Arc<AtomicU64>,
}

fn run_process_capture(
    config: &CaptureWorkerConfig,
    identity: &mixrelay_audio_io::windows::process_identity::ProcessIdentity,
    connected: &mut dyn FnMut(),
) -> Result<(), String> {
    let _apartment = render::ComApartment::new()?;
    if !identity.alive() {
        return Err("application exited before activation".into());
    }
    let client = activate_process_audio_client(identity.pid(), true)?;
    if !identity.alive() {
        return Err("application exited during activation".into());
    }
    config.target.verify(identity).map_err(|state| {
        config.state.store(state, Ordering::Release);
        "application selection changed during activation".to_owned()
    })?;
    let format = WAVEFORMATEX {
        wFormatTag: u16::try_from(WAVE_FORMAT_IEEE_FLOAT).expect("format tag fits u16"),
        nChannels: config.channels,
        nSamplesPerSec: ENGINE_SAMPLE_RATE,
        nAvgBytesPerSec: ENGINE_SAMPLE_RATE * u32::from(config.channels) * 4,
        nBlockAlign: config.channels * 4,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    // Process-loopback has no endpoint mix format. Its explicitly negotiated
    // 48 kHz domain still needs asynchronous synchronization to the graph clock.
    let mut bridge = clock_bridge::CaptureBridge::new(
        usize::from(config.channels),
        ENGINE_SAMPLE_RATE,
        config.graph_rate,
        &config.ring,
    )?;
    // SAFETY: the COM client and format stay on this initialized worker thread.
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                0,
                0,
                &raw const format,
                None,
            )
            .map_err(|error| format!("could not initialize process loopback: {error}"))?;
        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|error| format!("could not acquire WASAPI capture service: {error}"))?;
        let event = render::BufferReadyEvent::new(&client)?;
        // Declare after the event: stop the stream on every exit before closing
        // the event handle or releasing interfaces, including packet errors.
        let _started = StartedCapture(&client);
        client
            .Start()
            .map_err(|error| format!("could not start process loopback: {error}"))?;
        if config.stop.load(Ordering::Acquire) || !identity.alive() {
            return Ok(());
        }
        connected();
        let mut identity_check = Instant::now();
        while !config.stop.load(Ordering::Acquire) {
            if !identity.alive() {
                return Err("application exited".into());
            }
            if identity_check.elapsed() >= Duration::from_millis(250) {
                config.target.verify(identity).map_err(|state| {
                    config.state.store(state, Ordering::Release);
                    "application selection changed".to_owned()
                })?;
                identity_check = Instant::now();
            }
            if event.wait()? && !config.stop.load(Ordering::Acquire) {
                drain_selected_capture_packets(
                    &capture,
                    usize::from(config.channels),
                    &config.ring,
                    &mut bridge,
                    &config.epoch,
                )?;
            }
        }
    }
    Ok(())
}

struct StartedCapture<'a>(&'a IAudioClient);
impl Drop for StartedCapture<'_> {
    fn drop(&mut self) {
        // This guard never outlives the worker's COM apartment/client.
        let _ = unsafe { self.0.Stop() };
    }
}

struct CapturePacket<'a> {
    client: &'a IAudioCaptureClient,
    frames: u32,
    released: bool,
}
impl CapturePacket<'_> {
    fn release(mut self) -> Result<(), String> {
        self.released = true;
        unsafe { self.client.ReleaseBuffer(self.frames) }
            .map_err(|error| format!("could not release WASAPI packet: {error}"))
    }
}
impl Drop for CapturePacket<'_> {
    fn drop(&mut self) {
        if !self.released {
            // A size/null-pointer validation failure still returns the packet.
            let _ = unsafe { self.client.ReleaseBuffer(self.frames) };
        }
    }
}

#[expect(
    clippy::cast_ptr_alignment,
    reason = "WASAPI aligns negotiated float buffers"
)]
unsafe fn drain_selected_capture_packets(
    capture: &IAudioCaptureClient,
    channels: usize,
    ring: &SpscSampleRing,
    bridge: &mut clock_bridge::CaptureBridge,
    epoch: &AtomicU64,
) -> Result<(), String> {
    loop {
        let packet_frames = unsafe { capture.GetNextPacketSize() }
            .map_err(|error| format!("could not inspect WASAPI packet: {error}"))?;
        if packet_frames == 0 {
            return Ok(());
        }
        let mut data = ptr::null_mut();
        let mut frames = 0;
        let mut flags = 0;
        let mut position = 0;
        unsafe {
            capture
                .GetBuffer(
                    &raw mut data,
                    &raw mut frames,
                    &raw mut flags,
                    Some(&raw mut position),
                    None,
                )
                .map_err(|error| format!("could not read WASAPI packet: {error}"))?;
        }
        #[cfg(test)]
        ring.capture_evidence.packet(position, frames, flags);
        let packet = CapturePacket {
            client: capture,
            frames,
            released: false,
        };
        let samples = usize::try_from(frames)
            .ok()
            .and_then(|frames| frames.checked_mul(channels))
            .ok_or_else(|| "WASAPI packet size overflowed".to_owned())?;
        if flags & u32::try_from(AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0).unwrap_or_default() != 0
        {
            bridge.reset(ring);
            // The graph consumer alone flushes its ring on this epoch. Never
            // write the consumer cursor from a capture producer thread.
            epoch.fetch_add(1, Ordering::Release);
        }
        if flags & u32::try_from(AUDCLNT_BUFFERFLAGS_SILENT.0).unwrap_or_default() != 0 {
            bridge.push_packet(None, frames as usize, channels, ring)?;
        } else if samples != 0 {
            if data.is_null() {
                return Err("WASAPI returned a null non-silent capture packet".into());
            }
            // SAFETY: WASAPI owns a readable float buffer until `ReleaseBuffer` below.
            bridge.push_packet(
                Some(unsafe { std::slice::from_raw_parts(data.cast::<f32>(), samples) }),
                frames as usize,
                channels,
                ring,
            )?;
        }
        packet.release()?;
    }
}

fn activate_process_audio_client(
    process_id: u32,
    include_descendants: bool,
) -> Result<IAudioClient, String> {
    if !include_descendants {
        // EXCLUDE_TARGET_PROCESS_TREE means capture OTHER applications, not
        // capture this PID without children. Never widen a selector silently.
        return Err("WASAPI process capture requires include_descendants=true; exact-PID capture without children is unsupported".into());
    }
    let state = Arc::new(ActivationState::default());
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler {
        state: Arc::clone(&state),
    }
    .into();
    let mut parameters = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: process_id,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let property = BorrowedActivationBlob::new(&mut parameters);
    // SAFETY: the tagged PROPVARIANT blob points at stack data that remains alive until the API
    // returns. The asynchronous operation copies the activation parameters during this call.
    unsafe {
        let client_iid = IAudioClient::IID;
        let _operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &raw const client_iid,
            Some(&raw const *property.value),
            &handler,
        )
        .map_err(|error| format!("could not request process loopback activation: {error}"))?;
        state.wait(ACTIVATION_TIMEOUT)
    }
}

/// PROPVARIANT normally owns its blob (windows-rs Drop calls `PropVariantClear`).
/// This one only borrows activation parameters. Suppress the owning destructor
/// and tie the blob pointer to its actual Rust lifetime; nothing is heap-leaked.
struct BorrowedActivationBlob<'a> {
    value: ManuallyDrop<PROPVARIANT>,
    _parameters: std::marker::PhantomData<&'a mut AUDIOCLIENT_ACTIVATION_PARAMS>,
}
impl<'a> BorrowedActivationBlob<'a> {
    fn new(parameters: &'a mut AUDIOCLIENT_ACTIVATION_PARAMS) -> Self {
        let mut value = ManuallyDrop::new(PROPVARIANT::default());
        unsafe {
            let inner = (&raw mut value.Anonymous.Anonymous).cast::<PROPVARIANT_0_0>();
            (*inner).vt = VT_BLOB;
            (*inner).Anonymous.blob = BLOB {
                cbSize: u32::try_from(size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>())
                    .expect("activation parameter size fits u32"),
                pBlobData: std::ptr::from_mut(parameters).cast::<u8>(),
            };
        }
        Self {
            value,
            _parameters: std::marker::PhantomData,
        }
    }
}

#[derive(Default)]
struct ActivationState {
    result: Mutex<Option<Result<usize, String>>>,
    ready: Condvar,
}

impl ActivationState {
    fn wait(&self, timeout: Duration) -> Result<IAudioClient, String> {
        let result = self
            .result
            .lock()
            .map_err(|_| "WASAPI activation state is poisoned".to_owned())?;
        let (mut result, timeout_result) = self
            .ready
            .wait_timeout_while(result, timeout, |value| value.is_none())
            .map_err(|_| "WASAPI activation state is poisoned".to_owned())?;
        if timeout_result.timed_out() && result.is_none() {
            return Err("WASAPI process-loopback activation timed out".to_owned());
        }
        let raw = result
            .take()
            .ok_or_else(|| "WASAPI activation returned no result".to_owned())??;
        // SAFETY: the callback transferred one owned COM reference. Both callback and capture
        // threads run in the process MTA, and this reconstructs that reference exactly once.
        Ok(unsafe { IAudioClient::from_raw(raw as *mut c_void) })
    }
}

impl Drop for ActivationState {
    fn drop(&mut self) {
        let Ok(slot) = self.result.get_mut() else {
            return;
        };
        let Some(Ok(raw)) = slot.take() else {
            return;
        };
        // SAFETY: no waiter consumed the callback's owned COM reference. Reconstructing and
        // dropping it here releases exactly that one reference, including after a timeout.
        drop(unsafe { IAudioClient::from_raw(raw as *mut c_void) });
    }
}

#[allow(clippy::inline_always, clippy::ref_as_ptr)]
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivationHandler {
    state: Arc<ActivationState>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(
        &self,
        activateoperation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let result = (|| unsafe {
            let operation = activateoperation.as_ref().ok_or_else(|| {
                windows::core::Error::from_hresult(HRESULT(0x8000_4003_u32.cast_signed()))
            })?;
            let mut activation_result = HRESULT(0);
            let mut interface: Option<IUnknown> = None;
            operation.GetActivateResult(&raw mut activation_result, &raw mut interface)?;
            activation_result.ok()?;
            interface
                .ok_or_else(|| {
                    windows::core::Error::from_hresult(HRESULT(0x8000_4005_u32.cast_signed()))
                })?
                .cast::<IAudioClient>()
                .map(|client| client.into_raw() as usize)
        })()
        .map_err(|error| format!("WASAPI process-loopback activation failed: {error}"));
        if let Ok(mut slot) = self.state.result.lock() {
            *slot = Some(result);
            self.state.ready.notify_one();
        }
        Ok(())
    }
}

/// Single-producer/single-consumer sample ring. Monotonic counters keep ownership separate:
/// the capture thread writes `write`, while the audio thread writes `read`.
struct SpscSampleRing {
    samples: Box<[AtomicU32]>,
    read: AtomicU64,
    write: AtomicU64,
    // Recovery discard advances read, but is not actual consumer-clock work.
    consumed: AtomicU64,
    #[cfg(test)]
    capture_evidence: application_signal_tests::CaptureEvidence,
}

impl SpscSampleRing {
    fn available_frames(&self, channels: usize) -> usize {
        if channels == 0 {
            return 0;
        }
        usize::try_from(
            self.write
                .load(Ordering::Acquire)
                .saturating_sub(self.read.load(Ordering::Acquire)),
        )
        .unwrap_or(self.samples.len())
        .min(self.samples.len())
            / channels
    }

    #[cfg(test)]
    fn push_selected(&self, source: &[f32], native: usize, selected: usize) -> usize {
        if selected == 0 || selected > native {
            return 0;
        }
        self.push_bits(
            source
                .chunks_exact(native)
                .flat_map(|frame| frame[..selected].iter().map(|sample| sample.to_bits())),
            source.len() / native * selected,
            selected,
        )
    }
    /// Consumer-only: retire queued PCM before a recovered stream is activated.
    fn discard(&self) {
        self.read
            .store(self.write.load(Ordering::Acquire), Ordering::Release);
    }
    fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity.max(1))
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            read: AtomicU64::new(0),
            write: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            #[cfg(test)]
            capture_evidence: application_signal_tests::CaptureEvidence::default(),
        }
    }

    fn push(&self, source: &[f32], channels: usize) -> usize {
        self.push_bits(
            source.iter().map(|sample| sample.to_bits()),
            source.len(),
            channels,
        )
    }

    fn push_bits(
        &self,
        source: impl Iterator<Item = u32>,
        source_len: usize,
        channels: usize,
    ) -> usize {
        if channels == 0 {
            return 0;
        }
        let read = self.read.load(Ordering::Acquire);
        let write = self.write.load(Ordering::Relaxed);
        let used = usize::try_from(write.saturating_sub(read)).unwrap_or(self.samples.len());
        let free_frames = self.samples.len().saturating_sub(used) / channels;
        let accepted = (source_len / channels).min(free_frames) * channels;
        for (offset, bits) in source.take(accepted).enumerate() {
            let index = (usize::try_from(write).unwrap_or(0) + offset) % self.samples.len();
            self.samples[index].store(bits, Ordering::Relaxed);
        }
        self.write.store(
            write + u64::try_from(accepted).unwrap_or(0),
            Ordering::Release,
        );
        accepted
    }

    fn pop(&self, destination: &mut [f32], channels: usize) -> usize {
        self.pop_selected(destination, channels, channels)
    }

    /// Preserve channel indices and silence unaddressed native channels; this
    /// is an explicit prefix projection, never a stereo/mono remix.
    fn pop_selected(&self, destination: &mut [f32], native: usize, channels: usize) -> usize {
        destination.fill(0.0);
        if channels == 0 || channels > native {
            destination.fill(0.0);
            return 0;
        }
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let available = usize::try_from(write.saturating_sub(read)).unwrap_or(0);
        let consumed = (destination.len() / native).min(available / channels) * channels;
        for offset in 0..consumed {
            let index = (usize::try_from(read).unwrap_or(0) + offset) % self.samples.len();
            destination[offset / channels * native + offset % channels] =
                f32::from_bits(self.samples[index].load(Ordering::Relaxed));
        }
        self.read.store(
            read + u64::try_from(consumed).unwrap_or(0),
            Ordering::Release,
        );
        self.consumed
            .fetch_add(u64::try_from(consumed).unwrap_or(0), Ordering::Release);
        consumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "channel projection must preserve PCM bit-exactly, without DSP arithmetic"
    )]
    fn device_channel_projection_preserves_indices_and_silences_unused_outputs() {
        for native in [1, 2, 8, 16, 32, 64] {
            for selected in 1..=native {
                let ring = SpscSampleRing::new(selected * 16);
                let pcm = (0..native * 8)
                    .map(|i| f32::from(u16::try_from(i % native + 1).unwrap()))
                    .collect::<Vec<_>>();
                assert_eq!(ring.push_selected(&pcm, native, selected), selected * 8);
                let mut result = vec![f32::NAN; native * 8];
                assert_eq!(
                    ring.pop_selected(&mut result, native, selected),
                    selected * 8
                );
                for frame in result.chunks_exact(native) {
                    for (channel, sample) in frame.iter().enumerate() {
                        assert_eq!(
                            *sample,
                            if channel < selected {
                                f32::from(u16::try_from(channel + 1).unwrap())
                            } else {
                                0.0
                            }
                        );
                    }
                }
                assert_eq!(ring.push_selected(&pcm, native, selected), selected * 8);
                ring.discard();
                assert_eq!(ring.pop_selected(&mut result, native, selected), 0);
                assert!(result.iter().all(|sample| *sample == 0.0));
            }
        }
    }

    /// Runs the actual production graph worker through native-output attachment
    /// and removal, including returning to a valid empty/control-only graph.
    #[test]
    #[ignore = "requires an interactive Windows lab audio session"]
    fn native_wasapi_graph_activation_and_removal() {
        assert_eq!(std::env::var("MIXRELAY_LAB_AUDIO_TEST").as_deref(), Ok("1"));
        let meters = default_peak_meter_bank();
        let processor = create_initial_processor(Arc::clone(&meters)).unwrap();
        let exchange = Arc::new(ProcessorExchange::new(1));
        let resources = Arc::new(WindowsGraphResources::default());
        let (worker_exchange, worker_resources) = (exchange.clone(), resources.clone());
        let worker =
            thread::spawn(move || run_audio_worker(processor, worker_exchange, worker_resources));
        let guard = mixrelay_engine_service::session::AudioWorker::new(
            resources.runtime_status.clone(),
            worker,
        );
        for revision in 2..10 {
            let attached = revision % 2 == 0;
            let document = native_output_test_document(revision, attached);
            let execution = GraphCompiler::default()
                .compile(
                    &document,
                    CompileContext {
                        format: render_format(),
                        limits: CompileLimits::default(),
                    },
                )
                .unwrap();
            let render = RenderPlan::prepare(&execution).unwrap();
            resources.reconcile(&document).unwrap();
            exchange
                .submit(Box::new(BlockProcessor::with_peak_meters(
                    Arc::new(render),
                    meters.clone(),
                )))
                .unwrap();
            let start = Instant::now();
            loop {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "graph activation/processing stalled"
                );
                let processed = !attached
                    || resources
                        .render_cache
                        .lock()
                        .unwrap()
                        .get("probe")
                        .and_then(|(_, stream)| stream.upgrade())
                        .is_some_and(|stream| stream.written_frames() >= 480);
                if exchange.revisions().active == revision
                    && processed
                    && resources.device_clock_active.load(Ordering::Acquire) == attached
                {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            exchange.reclaim_retired();
            resources.reclaim_retired();
            eprintln!("graph revision={revision} native_output={attached} activated and processed");
        }
        drop(guard);
        assert!(resources.runtime_status.stop_requested());
    }

    fn native_output_test_document(
        revision: u64,
        attached: bool,
    ) -> mixrelay_audio_graph::GraphDocument {
        use mixrelay_audio_graph::{
            AudioDeviceOutputConfig, AudioEndpoint, ChannelMapping, DeviceSelector, EdgeSpec,
            GainConfig, GraphDocument, NodeSpec,
        };
        if !attached {
            return GraphDocument::new(revision, vec![], vec![]);
        }
        // An unconnected hardware sink is deliberately absent from external DSP
        // bindings. Feed deterministic silence through a processor so this test
        // verifies real graph -> ExternalIo -> native ring writes, without
        // routing the test process's own captured output back into itself.
        GraphDocument::new(
            revision,
            vec![
                NodeSpec {
                    control: mixrelay_audio_graph::NodeControl::default(),
                    id: "silence".into(),
                    label: None,
                    config: NodeConfig::Gain(GainConfig {
                        channels: 2,
                        gain_db: 0.0,
                    }),
                },
                NodeSpec {
                    control: mixrelay_audio_graph::NodeControl::default(),
                    id: "probe".into(),
                    label: None,
                    config: NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                        selector: DeviceSelector::DefaultOutput,
                        input_channels: 2,
                    }),
                },
            ],
            vec![EdgeSpec {
                id: "route".into(),
                source: AudioEndpoint {
                    node_id: "silence".into(),
                    port_id: AUDIO_PORT_ID.into(),
                },
                destination: AudioEndpoint {
                    node_id: "probe".into(),
                    port_id: AUDIO_PORT_ID.into(),
                },
                channel_mapping: ChannelMapping::Auto,
            }],
        )
    }

    /// Opt-in native boundary test: captures only this test process, never other
    /// applications. Emits a quiet (-48 dBFS) tone to the guest default output.
    #[test]
    #[ignore = "requires an interactive Windows lab audio session; emits a quiet test tone"]
    fn native_wasapi_self_capture_and_teardown() {
        use mixrelay_audio_graph::{AudioDeviceOutputConfig, DeviceSelector};
        use windows::Win32::System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{GetCurrentProcess, GetProcessHandleCount},
        };
        assert_eq!(std::env::var("MIXRELAY_LAB_AUDIO_TEST").as_deref(), Ok("1"));
        let handles = || {
            let mut count = 0;
            unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) }.unwrap();
            count
        };
        let private_bytes = || {
            let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
            unsafe {
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    (&raw mut memory).cast(),
                    u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX>()).unwrap(),
                )
            }
            .unwrap();
            memory.PrivateUsage
        };
        let iterations = std::env::var("MIXRELAY_LAB_AUDIO_CYCLES").map_or(13, |value| {
            value.parse::<usize>().expect("integer cycle count")
        });
        assert!(
            (13..=2000).contains(&iterations),
            "bounded lab cycle count required"
        );
        let counters = Arc::new(mixrelay_audio_engine::RuntimeCounters::default());
        // The first complete capture AND render cycle warms process-global COM
        // and audio-service caches. Capture alone does not warm render caches.
        let cold_handles = handles();
        let mut handle_samples = Vec::with_capacity(iterations);
        let mut memory_samples = Vec::with_capacity(iterations);
        for iteration in 0..iterations {
            let capture = ProcessLoopbackCapture::open(std::process::id(), true, 2).unwrap();
            let mut cache = render::RenderCache::new();
            let outputs = render::prepare_outputs(
                &[(
                    "probe".into(),
                    AudioDeviceOutputConfig {
                        selector: DeviceSelector::DefaultOutput,
                        input_channels: 2,
                    },
                )],
                &mut cache,
                &counters,
            )
            .unwrap();
            let output = &outputs["probe"];
            probe_native_demand(iteration, output, &capture);
            drop(outputs);
            drop(capture);
            handle_samples.push(handles());
            memory_samples.push(private_bytes());
            eprintln!(
                "post-teardown iteration={iteration} handles={} private_bytes={}",
                handle_samples[iteration], memory_samples[iteration]
            );
        }
        eprintln!("native stream teardown handles: cold={cold_handles} cycles={handle_samples:?}");
        eprintln!("native stream teardown private_bytes={memory_samples:?}");
        let baseline = handle_samples[0];
        let maximum = *handle_samples.iter().max().unwrap();
        // Preserve every sample so delayed one-time initialization is not
        // confused with monotonically accumulating per-stream handles.
        assert!(
            maximum <= baseline + 4,
            "native handles accumulated after full warmup"
        );
    }

    fn probe_native_demand(
        iteration: usize,
        output: &render::DeviceRender,
        capture: &ProcessLoopbackCapture,
    ) {
        let mut received = vec![0.0; 8192];
        let mut peak = [0.0_f32; 2];
        let mut captured_frames = 0;
        let mut tone = vec![0.0; 4096 * 2];
        let mut demand = mixrelay_engine_service::render_demand::RenderDemandCursor::default();
        let mut processed_frames = 0;
        let mut tracker = mixrelay_audio_io::process::CycleTracker::default();
        let started = Instant::now();
        while processed_frames < 33_600 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "native demand stalled"
            );
            let Some(cycle) = output.next_cycle(&mut demand) else {
                thread::park_timeout(Duration::from_millis(50));
                continue;
            };
            for block in CycleBlocks::new(cycle, CURRENT_TIMER_FORMAT).unwrap() {
                tracker.accept(block, CURRENT_TIMER_FORMAT).unwrap();
                let samples = block.frames as usize * 2;
                for (offset, frame) in tone[..samples].chunks_exact_mut(2).enumerate() {
                    let phase = u16::try_from((block.first_frame + offset as u64) % 48).unwrap();
                    let sample = (f32::from(phase) * std::f32::consts::TAU / 48.0).sin() * 0.004;
                    frame.copy_from_slice(&[sample, sample * 0.5]);
                }
                assert_eq!(
                    output.write(&tone[..samples]),
                    block.frames,
                    "native-paced overflow"
                );
                processed_frames += block.frames;
            }
            let count = capture.ring.pop(&mut received, 2);
            captured_frames += count / 2;
            for frame in received[..count].chunks_exact(2) {
                for channel in 0..2 {
                    peak[channel] = peak[channel].max(frame[channel].abs());
                }
            }
        }
        assert!(!output.failed(), "native render worker failed");
        eprintln!(
            "iteration={iteration} native_paced_frames={processed_frames} captured_frames={captured_frames} peak={peak:?}"
        );
        assert!(captured_frames > 16_000, "no sustained native capture");
        assert!(
            peak[0] > 0.001 && peak[0] < 0.006,
            "missing or corrupted left tone"
        );
        assert!(
            (peak[1] / peak[0] - 0.5).abs() < 0.05,
            "channel mapping changed"
        );
    }

    #[test]
    fn borrowed_activation_blob_never_frees_stack_storage() {
        let mut parameters = AUDIOCLIENT_ACTIVATION_PARAMS::default();
        let address = std::ptr::from_mut(&mut parameters).cast::<u8>();
        {
            let blob = BorrowedActivationBlob::new(&mut parameters);
            unsafe {
                let inner = &blob.value.Anonymous.Anonymous;
                assert_eq!(inner.vt, VT_BLOB);
                assert_eq!(inner.Anonymous.blob.pBlobData, address);
                assert_eq!(
                    inner.Anonymous.blob.cbSize as usize,
                    size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>()
                );
            }
        }
        parameters.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
        assert_eq!(
            parameters.ActivationType,
            AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
        );
    }

    #[test]
    fn exact_pid_selector_never_becomes_capture_of_other_applications() {
        let error = activate_process_audio_client(123, false).unwrap_err();
        assert!(error.contains("exact-PID capture"));
    }

    #[test]
    fn ring_rejects_incomplete_source_and_destination_frames() {
        for channels in [2, 8, 16, 32, 64] {
            let ring = SpscSampleRing::new(channels * 4);
            let source = vec![0.25; channels * 2 + 1];
            assert_eq!(ring.push(&source, channels), channels * 2);
            let mut partial = vec![9.0; channels + 1];
            assert_eq!(ring.pop(&mut partial, channels), channels);
            assert_eq!(partial[channels].to_bits(), 0.0_f32.to_bits());
            let mut rest = vec![0.0; channels];
            assert_eq!(ring.pop(&mut rest, channels), channels);
            assert!(rest.iter().all(|x| x.to_bits() == 0.25_f32.to_bits()));
        }
        let ring = SpscSampleRing::new(4);
        assert_eq!(ring.push(&[1.0], 0), 0);
        let mut invalid = [9.0];
        assert_eq!(ring.pop(&mut invalid, 0), 0);
        assert_eq!(invalid[0].to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn ring_overflow_preserves_unread_frames_across_wrap() {
        let ring = SpscSampleRing::new(5);
        for _ in 0..1000 {
            assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0], 2), 4);
            assert_eq!(ring.push(&[5.0, 6.0], 2), 0);
            let mut frame = [0.0; 2];
            assert_eq!(ring.pop(&mut frame, 2), 2);
            assert_eq!(frame.map(f32::to_bits), [1.0_f32, 2.0].map(f32::to_bits));
            assert_eq!(ring.push(&[5.0, 6.0], 2), 2);
            let mut remaining = [0.0; 4];
            assert_eq!(ring.pop(&mut remaining, 2), 4);
            assert_eq!(
                remaining.map(f32::to_bits),
                [3.0_f32, 4.0, 5.0, 6.0].map(f32::to_bits)
            );
        }
    }

    #[test]
    fn ring_concurrent_frames_keep_order() {
        let ring = Arc::new(SpscSampleRing::new(64 * 8));
        let writer = ring.clone();
        let thread = thread::spawn(move || {
            for sequence in 1..=10_000_u32 {
                let frame = [f32::from_bits(sequence); 64];
                while writer.push(&frame, 64) == 0 {
                    thread::yield_now();
                }
            }
        });
        for sequence in 1..=10_000_u32 {
            let mut frame = [0.0; 64];
            while ring.pop(&mut frame, 64) == 0 {
                thread::yield_now();
            }
            assert!(frame.iter().all(|sample| sample.to_bits() == sequence));
        }
        thread.join().unwrap();
    }

    #[test]
    fn sample_ring_preserves_frames_and_zero_fills_underflow() {
        let ring = SpscSampleRing::new(8);
        ring.push(&[1.0, 2.0, 3.0, 4.0], 2);
        let mut destination = [9.0; 6];
        ring.pop(&mut destination, 2);
        assert_eq!(
            destination.map(f32::to_bits),
            [1.0_f32, 2.0, 3.0, 4.0, 0.0, 0.0].map(f32::to_bits)
        );
    }

    #[test]
    fn sample_ring_never_accepts_half_an_interleaved_frame() {
        let ring = SpscSampleRing::new(5);
        ring.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2);
        let mut destination = [0.0; 6];
        ring.pop(&mut destination, 2);
        assert_eq!(
            destination.map(f32::to_bits),
            [1.0_f32, 2.0, 3.0, 4.0, 0.0, 0.0].map(f32::to_bits)
        );
    }
}
