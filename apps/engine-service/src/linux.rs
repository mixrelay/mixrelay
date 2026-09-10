use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, ExternalIo, PeakMeterBank, ProcessorExchange, RenderPlan,
};
use mixrelay_audio_graph::{
    ApplicationCaptureRequest, CompileContext, CompileLimits, FROM_OS_PORT_ID, GraphCompiler,
    NodeConfig, RenderFormat, TO_OS_PORT_ID,
};
use mixrelay_audio_io::process::{CycleBlocks, ProcessCycle, ProcessReport, TransferOutcome};
use mixrelay_audio_io::session::{AudioBackendKind, CURRENT_TIMER_FORMAT, ClockAuthority};
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities, VirtualEndpointSpec,
};
use mixrelay_control_server::router_with_backend;
use mixrelay_engine_service::{
    EngineGraphController, EngineGraphControllerConfig, GraphResourceBackend,
    default_duplex_document, default_peak_meter_bank, session::AdapterRuntimeStatus,
};
use mixrelay_processor_host::ProcessorCatalog;
use std::{
    ptr,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[path = "linux_applications.rs"]
mod applications;
#[path = "linux_clock.rs"]
mod clock;
#[path = "linux_devices.rs"]
mod devices;
#[path = "linux_format.rs"]
mod format;
#[path = "linux_native.rs"]
mod native;
use applications::{CaptureSet as PipeWireCaptureSet, CaptureWatch};
use devices::{DeviceStreams, DeviceWatch};
use mixrelay_audio_io::recovery::{RecoveryConsumer, RecoveryMailbox};
use mixrelay_engine_service::resource_requirements::{DeviceRequirements, ResourceRequirements};
use native::{NativeCycle, NativeDriver, NativeStream};
const ENGINE_BLOCK_FRAMES: u32 = CURRENT_TIMER_FORMAT.block_frames;
fn adapter_profile() -> mixrelay_engine_service::session::AdapterProfile {
    mixrelay_engine_service::session::AdapterProfile {
        initial_format: mixrelay_audio_io::session::CURRENT_TIMER_FORMAT,
        capabilities: mixrelay_audio_io::session::AudioSessionCapabilities {
            backend: AudioBackendKind::PipeWire,
            supported_sample_rates: format::RATES.to_vec(),
            supported_block_frames: format::BLOCKS.to_vec(),
            can_reconfigure_live: true,
            hardware_input: true,
            hardware_output: true,
            virtual_endpoint: true,
            application_capture: true,
            sample_rate_conversion: true,
            drift_correction: false,
        },
        application_capture_rates: format::RATES.to_vec(),
        clock_authority: mixrelay_audio_io::session::ClockAuthority::AudioServer,
        transport_latency_frames: None,
        requires_observed_device_rate: false,
    }
}
const ENGINE_SAMPLE_RATE: u32 = CURRENT_TIMER_FORMAT.sample_rate;
const MAX_ENDPOINT_CHANNELS: u16 = 64;
const CONTROL_SERVER_PORT: u16 = 19_341;

pub(super) fn run() -> Result<(), String> {
    let mut initial_spec = default_duplex_document(1, None)
        .nodes
        .into_iter()
        .find_map(|node| match node.config {
            mixrelay_audio_graph::NodeConfig::VirtualEndpoint(spec) => Some(EndpointBinding {
                node_id: node.id.as_str().to_owned(),
                spec: spec.spec,
            }),
            _ => None,
        })
        .ok_or_else(|| "default graph has no virtual endpoint".to_owned())?;
    let mut runtime_status = AdapterRuntimeStatus::new(adapter_profile())
        .with_graph_format_control()
        .with_observed_rate_following(format::observed_endpoint_rate);
    let initial_format = runtime_status.restore_preferences();
    initial_spec.spec.default_sample_rate = initial_format.sample_rate;
    initial_spec.spec.supported_sample_rates = vec![initial_format.sample_rate];
    let endpoint = PipeWireEndpoint::open_format(&initial_spec, initial_format)?;
    let endpoint_backend = LinuxVirtualEndpointBackend::new(initial_spec);
    endpoint_backend
        .active_format
        .store(format::pack(initial_format), Ordering::Release);
    let runtime_status = Arc::new(runtime_status);
    let meters = default_peak_meter_bank();
    let processor = create_initial_processor(Arc::clone(&meters), initial_format)?;
    let exchange = Arc::new(ProcessorExchange::new(processor.plan().revision()));
    let controller = Arc::new(
        EngineGraphController::new_with_configuration(
            Arc::clone(&exchange),
            EngineGraphControllerConfig {
                format: initial_format,
                meters,
                initial_bindings: processor.plan().meter_bindings().to_vec(),
                endpoint_backend: endpoint_backend.clone(),
                required_endpoint_node_id: None,
                processor_catalog: ProcessorCatalog::discover(),
                graph_resources: endpoint_backend.clone(),
                runtime_status: runtime_status.clone(),
            },
        )
        .with_graph_catalog(),
    );

    spawn_reclaimers(&exchange, &endpoint_backend)?;
    let _recovery = spawn_device_recovery(&endpoint_backend)?;
    let worker_status = runtime_status.clone();
    let audio_thread = thread::Builder::new()
        .name("mixrelay-pipewire-audio".to_owned())
        .spawn(move || {
            run_audio_worker(
                processor,
                exchange,
                endpoint_backend,
                endpoint,
                &worker_status,
            );
        })
        .map_err(|error| format!("could not start PipeWire audio worker: {error}"))?;
    let _worker = mixrelay_engine_service::session::AudioWorker::new(runtime_status, audio_thread);
    let _format_monitor = format::Monitor::start(&controller)?;
    let maintenance = controller.start_control_maintenance()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not initialize Linux async runtime: {error}"))?;
    runtime.block_on(async move {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], CONTROL_SERVER_PORT));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| format!("could not bind local control server: {error}"))?;
        let result = axum::serve(listener, router_with_backend(controller))
            .with_graceful_shutdown(async {
                let _ = mixrelay_engine_service::session::shutdown_signal().await;
            })
            .await
            .map_err(|error| format!("local control server failed: {error}"));
        let saved = maintenance.finish();
        result.and(saved)
    })
}

fn create_initial_processor(
    meters: Arc<PeakMeterBank>,
    format: RenderFormat,
) -> Result<Box<BlockProcessor>, String> {
    let execution = GraphCompiler::default()
        .compile(
            &mixrelay_engine_service::default_session_document(1, format),
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

const fn render_format() -> RenderFormat {
    RenderFormat {
        sample_rate: ENGINE_SAMPLE_RATE,
        block_frames: ENGINE_BLOCK_FRAMES,
    }
}

fn spawn_reclaimers(
    exchange: &Arc<ProcessorExchange>,
    endpoints: &Arc<LinuxVirtualEndpointBackend>,
) -> Result<(), String> {
    let weak_exchange = Arc::downgrade(exchange);
    let weak_endpoints = Arc::downgrade(endpoints);
    thread::Builder::new()
        .name("mixrelay-linux-reclaimer".to_owned())
        .spawn(move || {
            loop {
                let Some(exchange) = weak_exchange.upgrade() else {
                    break;
                };
                exchange.reclaim_retired();
                drop(exchange);
                let Some(endpoints) = weak_endpoints.upgrade() else {
                    break;
                };
                endpoints.reclaim_retired();
                drop(endpoints);
                thread::sleep(Duration::from_millis(100));
            }
        })
        .map(|_| ())
        .map_err(|error| format!("could not start Linux resource reclaimer: {error}"))
}

struct DeviceRecoveryWorker {
    stopping: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for DeviceRecoveryWorker {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn spawn_device_recovery(
    backend: &Arc<LinuxVirtualEndpointBackend>,
) -> Result<DeviceRecoveryWorker, String> {
    let weak = Arc::downgrade(backend);
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let worker = thread::Builder::new()
        .name("mixrelay-device-recovery".into())
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let Some(backend) = weak.upgrade() else {
                    break;
                };
                backend.device_recovery.reclaim();
                backend.recover_applications(&stop);
                if backend.device_recovery.idle() {
                    // Do not hold the configuration mutex across discovery/open.
                    let watch = backend
                        .device_watch
                        .lock()
                        .ok()
                        .and_then(|watch| watch.clone());
                    if let Some(watch) = watch {
                        let failed = watch.disconnected();
                        if !failed.is_empty()
                            && let Ok(prepared) = DeviceStreams::recover(
                                &failed,
                                watch.generation,
                                &stop,
                                watch.format,
                            )
                            && !prepared.is_empty()
                            && !stop.load(Ordering::Acquire)
                        {
                            // A concurrent edit may have replaced/deleted these nodes.
                            // The audio generation check rejects this result in that case.
                            let _ = backend.device_recovery.publish(watch.generation, prepared);
                        }
                    }
                }
                drop(backend);
                thread::sleep(Duration::from_millis(500));
            }
        })
        .map_err(|error| format!("could not start device recovery: {error}"))?;
    Ok(DeviceRecoveryWorker {
        stopping,
        worker: Some(worker),
    })
}

struct LinuxProcessState<'a> {
    processor: Box<BlockProcessor>,
    exchange: Arc<ProcessorExchange>,
    endpoint_backend: &'a LinuxVirtualEndpointBackend,
    application_recovery: RecoveryConsumer<'a, PipeWireCaptureSet>,
    device_recovery: RecoveryConsumer<'a, DeviceStreams>,
    active_endpoint: Option<Box<PipeWireEndpoint>>,
    captures: Box<PipeWireCaptureSet>,
    devices: Box<DeviceStreams>,
    render: Box<[f32]>,
    capture: Box<[f32]>,
    status: Arc<AdapterRuntimeStatus>,
    timeline: clock::GraphTimeline,
}

fn run_audio_worker(
    processor: Box<BlockProcessor>,
    exchange: Arc<ProcessorExchange>,
    endpoint_backend: Arc<LinuxVirtualEndpointBackend>,
    endpoint: PipeWireEndpoint,
    status: &Arc<AdapterRuntimeStatus>,
) {
    let Ok(_lease) = status.begin_processing() else {
        return;
    };
    let Some(application_recovery) = endpoint_backend.application_recovery.try_consumer() else {
        tracing::error!("application recovery already has an audio consumer");
        return;
    };
    let Some(device_recovery) = endpoint_backend.device_recovery.try_consumer() else {
        tracing::error!("device recovery already has an audio consumer");
        return;
    };
    let initial_rate = processor.plan().format().sample_rate;
    let mut context = Box::new(LinuxProcessState {
        processor,
        exchange,
        endpoint_backend: &endpoint_backend,
        application_recovery,
        device_recovery,
        active_endpoint: Some(Box::new(endpoint)),
        captures: Box::default(),
        devices: Box::default(),
        render: vec![0.0; usize::from(MAX_ENDPOINT_CHANNELS) * format::MAX_BLOCK]
            .into_boxed_slice(),
        capture: vec![0.0; usize::from(MAX_ENDPOINT_CHANNELS) * format::MAX_BLOCK]
            .into_boxed_slice(),
        status: status.clone(),
        timeline: clock::GraphTimeline::new(initial_rate),
    });
    // SAFETY: Box pins context; only the serial native RT callback accesses it
    // until Driver Drop disconnects/drains. No timer below ever processes audio.
    let driver =
        unsafe { NativeDriver::open(native_process, ptr::from_mut(context.as_mut()).cast()) };
    let Ok(driver) = driver else {
        return;
    };
    let started = Instant::now();
    while !driver.ready() && !status.stop_requested() {
        if started.elapsed() > Duration::from_secs(5) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    while !status.stop_requested() && driver.ready() {
        // Control-plane health/shutdown wait, NOT an audio scheduling clock.
        thread::sleep(Duration::from_millis(20));
    }
    drop(driver); // Drain before context (and its active resources) can be dropped.
}

unsafe extern "C" fn native_process(data: *mut std::ffi::c_void, native: *const NativeCycle) {
    // SAFETY: installed by run_audio_worker with pinned data and a native cycle
    // valid for this invocation. PipeWire serializes this filter's process calls.
    let (context, native) = unsafe { (&mut *data.cast::<LinuxProcessState<'_>>(), &*native) };
    if context.status.stop_requested() {
        return;
    }
    if context.status.checkpoint() {
        return;
    }
    if context.process(native).is_err() {
        context.status.request_stop();
    }
}

impl LinuxProcessState<'_> {
    fn process(&mut self, native: &NativeCycle) -> Result<(), ()> {
        let previous_format = self.processor.plan().format();
        let activated = self
            .exchange
            .activate_pending_configured(&mut self.processor, |revision| {
                self.endpoint_backend.activate_pending(
                    revision,
                    &mut self.active_endpoint,
                    &mut self.captures,
                    &mut self.devices,
                )
            })
            .unwrap_or(false);
        let format = self.processor.plan().format();
        if activated && previous_format != format {
            self.timeline = clock::GraphTimeline::new(format.sample_rate);
            self.status.activate_format(format);
        }
        format::observe_endpoint(self.active_endpoint.as_deref());
        let cycle = self.timeline.map(native)?;
        self.status
            .observe_clock(ClockAuthority::AudioServer, false);
        if cycle.frames == 0 {
            return Ok(());
        }
        let blocks = CycleBlocks::new(
            cycle,
            mixrelay_audio_io::session::SessionFormat {
                sample_rate: format.sample_rate,
                block_frames: format.block_frames,
            },
        )
        .map_err(|_| ())?;
        if cycle.discontinuity && cycle.clock.epoch > 1 {
            self.status.record_process(ProcessReport {
                discontinuities: 1,
                ..ProcessReport::default()
            });
        }
        for block in blocks {
            self.process_block(block)?;
        }
        // A node.group preference is not proof of a shared driver. Report only
        // actual native callback IDs after every currently active owner agrees.
        let synchronized = self.active_endpoint.as_ref().is_none_or(|endpoint| {
            endpoint
                .render
                .iter()
                .chain(endpoint.capture.iter())
                .all(|stream| stream.shares_clock(native.clock_id, native.rate))
        }) && self.captures.shares_clock(native.clock_id, native.rate)
            && self.devices.shares_clock(native.clock_id, native.rate);
        self.status
            .observe_clock(ClockAuthority::AudioServer, synchronized);
        Ok(())
    }

    fn process_block(&mut self, cycle: ProcessCycle) -> Result<(), ()> {
        self.application_recovery
            .activate(self.captures.generation, |next| {
                self.captures.replace(next);
            });
        self.device_recovery
            .activate(self.devices.generation, |next| {
                self.devices.replace_disconnected(next);
            });
        self.status
            .observe_endpoint(self.active_endpoint.as_ref().is_some_and(|e| e.ready()));
        let frames = cycle.frames as usize;
        let render_samples = self
            .active_endpoint
            .as_ref()
            .map_or(0, |e| e.render_channels * frames);
        let capture_samples = self
            .active_endpoint
            .as_ref()
            .map_or(0, |e| e.capture_channels * frames);
        let render = &mut self.render[..render_samples];
        let capture = &mut self.capture[..capture_samples];
        let render_frames = self
            .active_endpoint
            .as_mut()
            .map_or(0, |e| e.read_render(render));
        capture.fill(0.0);
        let report = self
            .processor
            .process_external_cycle(
                cycle,
                &mut LinuxExternalIo {
                    endpoint_id: self.active_endpoint.as_ref().map(|e| e.id.as_str()),
                    render,
                    render_frames,
                    capture,
                    application_captures: &mut self.captures,
                    devices: &mut self.devices,
                },
            )
            .map_err(|_| ())?;
        self.status.record_process(report);
        self.status.record_process(self.devices.take_report());
        if let Some(endpoint) = self.active_endpoint.as_mut()
            && !endpoint.write_capture(capture)
        {
            self.status.record_process(ProcessReport {
                output_overflows: 1,
                ..ProcessReport::default()
            });
        }
        if let Some(endpoint) = &mut self.active_endpoint {
            for stream in endpoint
                .render
                .iter_mut()
                .chain(endpoint.capture.iter_mut())
            {
                self.status.record_process(stream.take_report());
            }
        }
        self.status.record_process(self.captures.take_report());
        self.status.observe_application_state(self.captures.state());
        Ok(())
    }
}
struct LinuxExternalIo<'a> {
    endpoint_id: Option<&'a str>,
    render: &'a [f32],
    render_frames: u32,
    capture: &'a mut [f32],
    application_captures: &'a mut PipeWireCaptureSet,
    devices: &'a mut DeviceStreams,
}

impl ExternalIo for LinuxExternalIo<'_> {
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if Some(binding.node_id().as_str()) == self.endpoint_id
            && binding.port_id().as_str() == FROM_OS_PORT_ID
        {
            destination.copy_from_slice(self.render);
            return TransferOutcome::complete(self.render_frames);
        } else if binding.port_id().as_str() == mixrelay_audio_graph::AUDIO_PORT_ID {
            if !self
                .application_captures
                .contains(binding.node_id().as_str())
            {
                return self.devices.read(binding.node_id().as_str(), destination);
            }
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
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if Some(binding.node_id().as_str()) == self.endpoint_id
            && binding.port_id().as_str() == TO_OS_PORT_ID
        {
            self.capture.copy_from_slice(source);
            return TransferOutcome::complete(cycle.frames);
        }
        if binding.port_id().as_str() == mixrelay_audio_graph::AUDIO_PORT_ID {
            return self.devices.write(binding.node_id().as_str(), source);
        }
        TransferOutcome::complete(0)
    }
}

struct LinuxVirtualEndpointBackend {
    self_ref: Weak<Self>,
    active_format: AtomicU64,
    format_reserved: AtomicBool,
    capabilities: VirtualEndpointCapabilities,
    active_spec: Mutex<LinuxResourceSpec>,
    pending: AtomicPtr<LinuxResourceUpdate>,
    retired: AtomicPtr<LinuxResourceUpdate>,
    device_generation: AtomicU64,
    device_watch: Mutex<Option<DeviceWatch>>,
    device_recovery: RecoveryMailbox<DeviceStreams>,
    application_generation: AtomicU64,
    application_watch: Mutex<Option<CaptureWatch>>,
    application_recovery: RecoveryMailbox<PipeWireCaptureSet>,
}

impl LinuxVirtualEndpointBackend {
    fn new(initial_spec: EndpointBinding) -> Arc<Self> {
        Arc::new_cyclic(|self_ref| Self {
            self_ref: self_ref.clone(),
            active_format: AtomicU64::new(format::pack(render_format())),
            format_reserved: AtomicBool::new(false),
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: MAX_ENDPOINT_CHANNELS,
                max_capture_channels: MAX_ENDPOINT_CHANNELS,
                supported_sample_rates: format::RATES.to_vec(),
                supports_duplex: true,
                supports_dynamic_creation: false,
            },
            active_spec: Mutex::new(LinuxResourceSpec {
                endpoint: Some(initial_spec),
                captures: Vec::new(),
                devices: DeviceRequirements::default(),
            }),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
            device_generation: AtomicU64::new(0),
            device_watch: Mutex::new(None),
            device_recovery: RecoveryMailbox::default(),
            application_generation: AtomicU64::new(0),
            application_watch: Mutex::new(None),
            application_recovery: RecoveryMailbox::default(),
        })
    }

    fn recover_applications(&self, stopping: &AtomicBool) {
        self.application_recovery.reclaim();
        if !self.application_recovery.idle() {
            return;
        }
        let watch = self
            .application_watch
            .lock()
            .ok()
            .and_then(|watch| watch.clone());
        let Some(watch) = watch else {
            return;
        };
        let Ok(Some((prepared, next_watch))) = watch.recover(stopping) else {
            return;
        };
        if stopping.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut registered) = self.application_watch.lock()
            && registered
                .as_ref()
                .is_some_and(|current| current.generation == watch.generation)
            && self
                .application_recovery
                .publish(watch.generation, prepared)
                .is_ok()
        {
            *registered = Some(next_watch);
        }
    }

    fn activate_pending(
        &self,
        revision: u64,
        active: &mut Option<Box<PipeWireEndpoint>>,
        captures: &mut Box<PipeWireCaptureSet>,
        devices: &mut Box<DeviceStreams>,
    ) -> Result<(), &'static str> {
        if !self.retired.load(Ordering::Acquire).is_null() {
            return Err("the previous PipeWire endpoint is still being reclaimed");
        }
        let pending = self.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        if pending.is_null() {
            return Ok(());
        }
        // SAFETY: `reconcile` creates this pointer and the audio thread atomically removed the
        // only stored owner. Destruction is deferred because it may wait for `pw-cli`.
        let mut update = unsafe { Box::from_raw(pending) };
        if update.revision != revision {
            self.pending.store(Box::into_raw(update), Ordering::Release);
            return Err("PipeWire resources do not match pending graph revision");
        }
        if let EndpointUpdate::Replace(next) = &mut update.endpoint {
            std::mem::swap(active, next);
            if let Some(endpoint) = active
                && let Some(stream) = &mut endpoint.render
            {
                stream.discard_input();
            }
        }
        if let Some(next) = &mut update.captures {
            std::mem::swap(captures, next);
            captures.discard_inputs();
        }
        if let Some(next) = &mut update.devices {
            std::mem::swap(devices, next);
            devices.discard_inputs();
        }
        // Reuse the prepared envelope to retire old resources. No Box allocation or
        // deallocation, process wait, or FIFO teardown occurs on this audio boundary.
        self.retired.store(Box::into_raw(update), Ordering::Release);
        Ok(())
    }

    fn reclaim_retired(&self) {
        let retired = self.retired.swap(ptr::null_mut(), Ordering::AcqRel);
        if !retired.is_null() {
            // SAFETY: the reclaimer removed the only pointer published by the audio thread.
            drop(unsafe { Box::from_raw(retired) });
        }
    }

    fn publish(&self, update: LinuxResourceUpdate) -> Result<(), HostAudioBackendError> {
        if self.format_reserved.load(Ordering::Acquire) {
            return Err(HostAudioBackendError::BackendFailure {
                message: "PipeWire format update reserved".into(),
            });
        }
        let pointer = Box::into_raw(Box::new(update));
        if self
            .pending
            .compare_exchange(
                ptr::null_mut(),
                pointer,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            // SAFETY: publication failed, so this thread retains the allocation.
            drop(unsafe { Box::from_raw(pointer) });
            return Err(HostAudioBackendError::BackendFailure {
                message: "a PipeWire endpoint update is already pending".to_owned(),
            });
        }
        Ok(())
    }
}

impl VirtualEndpointBackend for LinuxVirtualEndpointBackend {
    fn settings(&self) -> Option<mixrelay_audio_io::VirtualEndpointSettings> {
        Some(mixrelay_audio_io::VirtualEndpointSettings::configurable(
            &self.capabilities,
        ))
    }

    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }

    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        if desired.len() > 1 {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "the Linux backend supports one duplex virtual endpoint".to_owned(),
            });
        }
        Ok(())
    }
}

impl GraphResourceBackend for LinuxVirtualEndpointBackend {
    fn device_inventory(
        &self,
    ) -> Result<Vec<mixrelay_audio_io::AudioDeviceDescriptor>, HostAudioBackendError> {
        mixrelay_audio_io::linux::devices()
    }

    fn prepare_format_change(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
        _current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<Box<dyn mixrelay_engine_service::PreparedGraphFormat>, HostAudioBackendError> {
        format::prepare(self, document, requested)
    }
    fn reconcile(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
    ) -> Result<(), HostAudioBackendError> {
        let next = LinuxResourceSpec::from_document(document)?;
        let format = format::unpack(self.active_format.load(Ordering::Acquire));
        let mut active =
            self.active_spec
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "PipeWire endpoint state is poisoned".to_owned(),
                })?;
        if *active == next {
            return Ok(());
        }
        let endpoint = if active.endpoint == next.endpoint {
            EndpointUpdate::Retain
        } else {
            EndpointUpdate::Replace(match &next.endpoint {
                Some(spec) => Some(Box::new(
                    PipeWireEndpoint::open_format(spec, format)
                        .map_err(|message| HostAudioBackendError::BackendFailure { message })?,
                )),
                None => None,
            })
        };
        let (captures, application_watch) = if active.captures == next.captures {
            (None, None)
        } else {
            let generation = self.application_generation.fetch_add(1, Ordering::Relaxed) + 1;
            let (captures, watch) =
                PipeWireCaptureSet::open_format(&next.captures, generation, format)
                    .map_err(|message| HostAudioBackendError::BackendFailure { message })?;
            (Some(Box::new(captures)), Some(watch))
        };
        let devices = if active.devices == next.devices {
            None
        } else {
            let generation = self.device_generation.fetch_add(1, Ordering::Relaxed) + 1;
            Some(Box::new(
                DeviceStreams::open_format(&next.devices, generation, format)
                    .map_err(|message| HostAudioBackendError::BackendFailure { message })?,
            ))
        };
        let watch = devices
            .as_ref()
            .map(|streams| streams.watch(next.devices.clone()));
        // Lock before publication so poisoning cannot leave a staged edit with
        // an unregistered watcher. Recovery never holds this lock during I/O.
        let mut registered =
            self.device_watch
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "device recovery state is poisoned".into(),
                })?;
        let mut registered_applications =
            self.application_watch
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "application recovery state is poisoned".into(),
                })?;
        self.publish(LinuxResourceUpdate {
            revision: document.revision,
            endpoint,
            captures,
            devices,
        })?;
        if let Some(watch) = watch {
            *registered = Some(watch);
        }
        if let Some(watch) = application_watch {
            *registered_applications = Some(watch);
        }
        *active = next;
        Ok(())
    }
}

impl Drop for LinuxVirtualEndpointBackend {
    fn drop(&mut self) {
        let pending = *self.pending.get_mut();
        if !pending.is_null() {
            // SAFETY: dropping the backend is exclusive; no shared owner can reach the update.
            drop(unsafe { Box::from_raw(pending) });
        }
        let retired = *self.retired.get_mut();
        if !retired.is_null() {
            // SAFETY: dropping the backend is exclusive; no shared owner can reach the endpoint.
            drop(unsafe { Box::from_raw(retired) });
        }
    }
}

#[derive(Clone, PartialEq)]
struct LinuxResourceSpec {
    endpoint: Option<EndpointBinding>,
    captures: Vec<(String, ApplicationCaptureRequest)>,
    devices: DeviceRequirements,
}

impl LinuxResourceSpec {
    fn from_document(
        document: &mixrelay_audio_graph::GraphDocument,
    ) -> Result<Self, HostAudioBackendError> {
        let requirements = ResourceRequirements::from_document(document);
        let endpoint = match requirements.endpoints.as_slice() {
            [] => None,
            [(node_id, spec)] => Some(EndpointBinding {
                node_id: node_id.clone(),
                spec: spec.clone(),
            }),
            _ => {
                return Err(HostAudioBackendError::CapabilityUnavailable {
                    capability: "the Linux backend supports one duplex virtual endpoint".to_owned(),
                });
            }
        };
        Ok(Self {
            endpoint,
            captures: requirements.captures,
            devices: requirements.devices,
        })
    }
}

// Keep graph binding identity explicit instead of deriving it from a native
// device name. The control protocol currently requires these IDs to match.
#[derive(Clone, PartialEq)]
struct EndpointBinding {
    node_id: String,
    spec: VirtualEndpointSpec,
}

struct LinuxResourceUpdate {
    revision: u64,
    endpoint: EndpointUpdate,
    captures: Option<Box<PipeWireCaptureSet>>,
    devices: Option<Box<DeviceStreams>>,
}

enum EndpointUpdate {
    Retain,
    // None removes the endpoint; the same slot retires the previous owner after swapping.
    Replace(Option<Box<PipeWireEndpoint>>),
}

struct PipeWireEndpoint {
    id: String,
    render_channels: usize,
    capture_channels: usize,
    render: Option<NativeStream>,
    capture: Option<NativeStream>,
}
impl PipeWireEndpoint {
    fn open_format(binding: &EndpointBinding, format: RenderFormat) -> Result<Self, String> {
        let spec = &binding.spec;
        let name = sanitize_node_name(spec.id.as_str());
        let render = spec
            .render
            .map(|stream| {
                NativeStream::open_format(
                    &format!("{name}.render"),
                    &spec.display_name,
                    None,
                    0,
                    stream.channels,
                    format,
                )
            })
            .transpose()?;
        let capture = spec
            .capture
            .map(|stream| {
                NativeStream::open_format(
                    &format!("{name}.capture"),
                    &spec.display_name,
                    None,
                    1,
                    stream.channels,
                    format,
                )
            })
            .transpose()?;
        Ok(Self {
            id: binding.node_id.clone(),
            render_channels: usize::from(spec.render.map_or(0, |s| s.channels)),
            capture_channels: usize::from(spec.capture.map_or(0, |s| s.channels)),
            render,
            capture,
        })
    }
    fn ready(&self) -> bool {
        self.render.as_ref().is_none_or(NativeStream::ready)
            && self.capture.as_ref().is_none_or(NativeStream::ready)
    }
    fn read_render(&mut self, destination: &mut [f32]) -> u32 {
        self.render.as_mut().map_or(0, |s| s.read(destination))
    }
    fn write_capture(&mut self, source: &[f32]) -> bool {
        self.capture
            .as_mut()
            .is_none_or(|s| s.write(source) as usize == source.len() / self.capture_channels)
    }
}
fn sanitize_node_name(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("io.mixrelay.{value}")
}
