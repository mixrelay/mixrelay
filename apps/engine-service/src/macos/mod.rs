mod callback_source;
mod callback_transport;
mod capture_schedule;
mod direct;
mod endpoint_backend;
mod endpoint_boundaries;
mod format;
mod maintenance;
mod native_transport;
mod worker_io;
use super::macos_pacing::{
    ApplicationClockSnapshot, ApplicationPacer, HardwarePacer, NativeClockSnapshot, NativePacer,
    Pace,
};
use mixrelay_audio_io::formats::MAX_BUFFER_FRAMES;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, ExternalIo, PeakMeterBank, ProcessorExchange, RenderPlan,
};
use mixrelay_audio_graph::{CompileContext, CompileLimits, GraphCompiler, RenderFormat};
use mixrelay_audio_io::process::{ProcessCycle, ProcessReport, TransferOutcome};
use mixrelay_control_server::router_with_backend;
use mixrelay_engine_service::{
    EngineGraphController, EngineGraphControllerConfig, default_peak_meter_bank,
    session::AdapterRuntimeStatus,
};
use mixrelay_processor_host::ProcessorCatalog;

#[cfg(test)]
pub(super) fn adapter_profile() -> mixrelay_engine_service::session::AdapterProfile {
    format::profile(mixrelay_audio_io::formats::SAMPLE_RATES.to_vec())
}
const CONTROL_SERVER_PORT: u16 = 19_341;

use endpoint_backend::MacVirtualEndpointBackend;

use mixrelay_engine_service::endpoint_service::AudioConfig as NativeAudioConfig;

unsafe extern "C" {
    fn mr_engine_service_source_clock(token: u64, snapshot: *mut ApplicationClockSnapshot) -> bool;
    fn mr_engine_service_set_source_clock_master(token: u64, master: bool) -> bool;
    fn mr_engine_service_read_source_clocked(
        source_token: u64,
        interleaved_samples: *mut f32,
        frame_count: u32,
        channels: u32,
        sample_rate: u32,
    ) -> u32;
}

struct MacEngineExternalIo<'a> {
    devices: &'a super::macos_devices::DeviceSet,
    render_samples: &'a [f32],
    render_transfer: TransferOutcome,
    capture_samples: &'a mut [f32],
    primary: Option<&'a mixrelay_audio_io::VirtualEndpointId>,
    independent: &'a mut endpoint_boundaries::EndpointBoundaries<callback_transport::NativeCapture>,
}

impl ExternalIo for MacEngineExternalIo<'_> {
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        let frames = cycle.frames;
        if let Some(outcome) = self.devices.read(binding.node_id().as_str(), destination) {
            return outcome;
        }
        if binding.virtual_endpoint_id().is_some() && binding.virtual_endpoint_id() == self.primary
        {
            destination.copy_from_slice(self.render_samples);
            return self.render_transfer;
        }
        if let Some(outcome) = self.independent.read(binding, destination) {
            return outcome;
        }
        // SAFETY: the destination is writable for `frames * binding.channels()` samples.
        let available = unsafe {
            mr_engine_service_read_source_clocked(
                source_token(binding.node_id().as_str()),
                destination.as_mut_ptr(),
                frames,
                u32::from(binding.channels()),
                cycle.sample_rate,
            )
        };
        TransferOutcome::complete(available)
    }

    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if let Some(outcome) = self.devices.write(binding.node_id().as_str(), source) {
            return outcome;
        }
        if binding.virtual_endpoint_id().is_some() && binding.virtual_endpoint_id() == self.primary
        {
            for (destination, sample) in self.capture_samples.iter_mut().zip(source) {
                *destination += sample;
            }
            return TransferOutcome::complete(cycle.frames);
        }
        if let Some(outcome) = self.independent.write(binding, source) {
            return outcome;
        }
        TransferOutcome::complete(0)
    }
}

fn source_token(source_id: &str) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in source_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    if hash == 0 { 1 } else { hash }
}

fn prepare_runtime() -> AdapterRuntimeStatus {
    // The processing domain is independent of each endpoint's native rate.
    // Endpoint format changes are ID-directed through VirtualEndpointBackend.
    AdapterRuntimeStatus::new(format::profile(
        mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
    ))
    .with_graph_format_control()
}

pub(super) async fn run() -> Result<(), String> {
    use mixrelay_macos_transport::app_lifecycle::{APP_LEASE, ENGINE_LEASE, ProcessLease};
    let Some(engine_lease) = ProcessLease::acquire_for_app(ENGINE_LEASE, APP_LEASE)
        .map_err(|error| format!("could not claim engine lifecycle: {error}"))?
    else {
        tracing::info!("desktop is absent; refusing demand-started audio execution");
        return Ok(());
    };
    engine_lease
        .record("running")
        .map_err(|error| format!("engine lifecycle record failed: {error}"))?;
    let result = run_owned().await;
    let outcome = result
        .as_ref()
        .map_or_else(|error| format!("failed\n{error}"), |()| "stopped".into());
    engine_lease
        .record(&outcome)
        .map_err(|error| format!("engine exit record failed: {error}"))?;
    result
}

async fn run_owned() -> Result<(), String> {
    // Reserve the control endpoint before native publication or audio ownership.
    // A second service must fail without preparing devices beside the running one.
    let listener = bind_control(SocketAddr::from(([127, 0, 0, 1], CONTROL_SERVER_PORT))).await?;
    initialize_native_transport()?;
    let mut runtime_status = prepare_runtime();
    let format = runtime_status.restore_preferences();
    let runtime_status = Arc::new(runtime_status);
    let meters = default_peak_meter_bank();
    let initial_document = mixrelay_audio_graph::GraphDocument::new(1, Vec::new(), Vec::new());
    let processor = create_initial_processor(&initial_document, format, Arc::clone(&meters))?;
    let exchange = Arc::new(ProcessorExchange::new(processor.plan().revision()));
    let initial_bindings = processor.plan().meter_bindings().to_vec();
    let endpoint_backend = Arc::new(MacVirtualEndpointBackend::new(
        mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
    ));
    let devices = Arc::new(super::macos_devices::MacDevices::new(
        runtime_status.clone(),
        format.sample_rate,
        format.block_frames,
    ));
    let processor_catalog = ProcessorCatalog::discover();
    let controller = Arc::new(
        EngineGraphController::new_with_initial_document(
            Arc::clone(&exchange),
            EngineGraphControllerConfig {
                format,
                meters,
                initial_bindings,
                endpoint_backend: endpoint_backend.clone(),
                required_endpoint_node_id: None,
                processor_catalog,
                graph_resources: devices.clone(),
                runtime_status: devices.clone(),
            },
            initial_document,
        )
        .with_graph_catalog(),
    );
    // Declared before AudioWorker so reclamation remains available until the
    // audio owner has stopped and joined, including partial-startup failures.
    let _reclaimer =
        maintenance::start_reclaimer(Arc::downgrade(&exchange), Arc::downgrade(&devices))?;
    let worker_status = runtime_status.clone();
    let monitor_devices = devices.clone();
    let audio_thread = std::thread::Builder::new()
        .name("mixrelay-audio-worker".into())
        .spawn(move || {
            if let Err(error) = run_audio_worker(
                processor,
                AudioContext {
                    exchange: &exchange,
                    endpoint_backend: &endpoint_backend,
                    runtime_status: &worker_status,
                    devices: &devices,
                },
            ) {
                tracing::error!(%error, "MixRelay audio worker failed");
            }
        })
        .map_err(|error| format!("could not start audio worker: {error}"))?;
    let _worker =
        mixrelay_engine_service::session::AudioWorker::new(runtime_status.clone(), audio_thread);
    let observers =
        maintenance::ControlObservers::start(&runtime_status, &controller, &monitor_devices)?;
    let result = serve_control(listener, controller).await;
    let saved = observers.finish();
    result.and(saved)
}

async fn bind_control(address: SocketAddr) -> Result<tokio::net::TcpListener, String> {
    tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("could not bind local control server at {address}: {error}"))
}

async fn serve_control(
    listener: tokio::net::TcpListener,
    controller: Arc<EngineGraphController>,
) -> Result<(), String> {
    axum::serve(
        mixrelay_control_server::bounded_listener(listener),
        router_with_backend(controller),
    )
    .with_graceful_shutdown(async {
        tokio::select! {
            result = mixrelay_engine_service::session::shutdown_signal() => {
                if let Err(error) = result {
                    tracing::error!(%error, "engine shutdown signal failed");
                }
            }
            () = desktop_released() => {}
        }
    })
    .await
    .map_err(|error| format!("local control server failed: {error}"))
}

async fn desktop_released() {
    use mixrelay_macos_transport::app_lifecycle::{APP_LEASE, is_owned};
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
        interval.tick().await;
        match is_owned(APP_LEASE) {
            Ok(true) => continue,
            Ok(false) => tracing::info!("desktop released ownership; finalizing audio server"),
            Err(error) => {
                tracing::error!(%error, "desktop lease probe failed; stopping audio server");
            }
        }
        break;
    }
}

#[cfg(test)]
mod startup_tests;

fn initialize_native_transport() -> Result<(), String> {
    let exit_code = mixrelay_engine_service::endpoint_service::mr_engine_service_start();
    if exit_code != 0 {
        return Err(format!(
            "native engine service initialization failed with code {exit_code}"
        ));
    }
    Ok(())
}

fn create_initial_processor(
    document: &mixrelay_audio_graph::GraphDocument,
    format: RenderFormat,
    meters: Arc<PeakMeterBank>,
) -> Result<Box<BlockProcessor>, String> {
    let execution = GraphCompiler::default()
        .compile(
            document,
            CompileContext {
                format,
                limits: CompileLimits::default(),
            },
        )
        .map_err(|error| error.to_string())?;
    let render_plan = RenderPlan::prepare(&execution).map_err(|error| error.to_string())?;
    Ok(Box::new(BlockProcessor::with_peak_meters(
        Arc::new(render_plan),
        meters,
    )))
}

#[derive(Clone, Copy)]
struct AudioContext<'a> {
    exchange: &'a ProcessorExchange,
    endpoint_backend: &'a MacVirtualEndpointBackend,
    runtime_status: &'a AdapterRuntimeStatus,
    devices: &'a super::macos_devices::MacDevices,
}
impl AudioContext<'_> {
    fn activate(
        &self,
        processor: &mut Box<BlockProcessor>,
        active_devices: &mut super::macos_devices::DeviceSet,
        activation: &mut super::macos_devices::DeviceActivation<'_>,
    ) -> Result<bool, String> {
        let mut activated = MacVirtualEndpointBackend::activate_recovery();
        // Same-spec native recovery must not wait for an unrelated graph edit.
        activation.activate_recovered(active_devices);
        activated |= self.exchange.activate_pending_configured(processor, |_| {
            self.endpoint_backend.activate_pending()?;
            activation.activate(active_devices);
            Ok::<(), String>(())
        })?;
        Ok(activated)
    }
}
fn run_audio_worker(
    mut processor: Box<BlockProcessor>,
    context: AudioContext<'_>,
) -> Result<(), String> {
    let runtime_status = context.runtime_status;
    let _lease = runtime_status.begin_processing()?;
    let mut activation = context.devices.activation()?;
    let mut active_devices = super::macos_devices::DeviceSet::default();
    let mut io = worker_io::WorkerIo::prepare(processor.plan())?;
    let mut timeline = MacTimeline::new();
    let mut pacers = NativePacers::default();
    let mut boundary_check = Instant::now();
    let mut awaiting_format = false;

    while !runtime_status.stop_requested() {
        if runtime_status.checkpoint() {
            std::thread::sleep(Duration::from_millis(1));
            timeline.deadline = Instant::now();
            continue;
        }
        let previous_format = processor.plan().format();
        if context.activate(&mut processor, &mut active_devices, &mut activation)?
            && io.reconcile(
                processor.plan(),
                processor.plan().format().sample_rate != previous_format.sample_rate,
            )?
        {
            timeline.reset();
            pacers.hal.reset();
        }
        let format = processor.plan().format();
        if !active_devices.processing_ready(format.sample_rate) {
            if !awaiting_format {
                tracing::info!(
                    processing_rate = format.sample_rate,
                    "CoreAudio rendering gated until device format is ready"
                );
                awaiting_format = true;
            }
            runtime_status.observe_clock_idle();
            timeline.reset();
            pacers = NativePacers::default();
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        if std::mem::take(&mut awaiting_format) {
            tracing::info!(
                processing_rate = format.sample_rate,
                "CoreAudio device format ready; rendering admitted"
            );
        }
        runtime_status.activate_format(format);
        if let Some(plan) =
            super::macos_devices::direct::DirectPlan::prepare(&active_devices, processor.plan())
        {
            io.retire();
            direct::run(&plan, &mut processor, context)?;
            io = worker_io::WorkerIo::prepare(processor.plan())?;
            timeline.reset();
            pacers = NativePacers::default();
            continue;
        }
        if boundary_check.elapsed() >= Duration::from_millis(100) && io.pending(format.sample_rate)
        {
            if io.reconcile(processor.plan(), false)? {
                timeline.reset();
                pacers.hal.reset();
            }
            boundary_check = Instant::now();
        }
        process_worker_block(
            &mut io,
            &mut timeline,
            &mut pacers,
            &mut WorkerBlock {
                processor: &mut processor,
                devices: &active_devices,
                status: runtime_status,
            },
        )?;
    }
    io.retire();
    Ok(())
}

struct WorkerBlock<'a> {
    processor: &'a mut BlockProcessor,
    devices: &'a super::macos_devices::DeviceSet,
    status: &'a AdapterRuntimeStatus,
}

fn process_worker_block(
    io: &mut worker_io::WorkerIo<worker_io::NativeTransport>,
    timeline: &mut MacTimeline,
    pacers: &mut NativePacers,
    block: &mut WorkerBlock<'_>,
) -> Result<(), String> {
    let route = RouteClock::observe(block.processor, block.devices);
    let endpoint = io.snapshot();
    let (pace, clock_id) = next_native_pace(
        pacers,
        block.devices,
        block.processor.plan(),
        io.capacity(),
        endpoint.as_ref(),
    );
    let Some((frames, discontinuity)) =
        timeline.accept((pace, clock_id), &route, block.status, endpoint.as_ref())
    else {
        // Waking this thread creates no credit; only the selected native clock
        // advances the processor's device-owned frame coordinates.
        std::thread::sleep(Duration::from_micros(250));
        return Ok(());
    };
    let period = Duration::from_secs_f64(
        f64::from(frames) / f64::from(block.processor.plan().format().sample_rate),
    );
    let processed = io.process(
        block.processor,
        WorkerCycle {
            timeline,
            frames,
            discontinuity: discontinuity || timeline.discontinuity_pending,
            write_hal: timeline.native && clock_id == 1 || pace == Pace::Unknown && route.has_hal,
            devices: block.devices,
            status: block.status,
            endpoint,
        },
    )?;
    timeline.discontinuity_pending = !processed;
    mixrelay_engine_service::endpoint_service::touch_audio();
    timeline.frame += u64::from(frames);
    if !timeline.native {
        wait_for_block(&mut timeline.deadline, period);
    }
    Ok(())
}

struct RouteClock {
    maximum_frames: u32,
    has_hal: bool,
    synchronized: bool,
    independent_idle: bool,
}
impl RouteClock {
    fn observe(processor: &BlockProcessor, devices: &super::macos_devices::DeviceSet) -> Self {
        let mut result = Self {
            maximum_frames: processor.plan().format().block_frames,
            has_hal: false,
            synchronized: true,
            independent_idle: true,
        };
        for binding in processor
            .plan()
            .source_bindings()
            .iter()
            .chain(processor.plan().sink_bindings())
        {
            let id = binding.node_id().as_str();
            if devices.is_inactive(id) {
                continue;
            }
            if binding.virtual_endpoint_id().is_some() {
                result.has_hal = true;
            } else {
                // Missing bridge evidence must not be upgraded to synchronized.
                if let Some(ready) = devices.clock_synchronized(id) {
                    result.synchronized &= ready;
                    result.independent_idle = false;
                } else if let Some(clock) = application_clock(id) {
                    result.synchronized &= clock.synchronized != 0
                        && clock.graph_rate == processor.plan().format().sample_rate;
                    result.independent_idle &= clock.running == 0;
                } else {
                    result.synchronized = false;
                    result.independent_idle = false;
                }
            }
        }
        result
    }
}

#[derive(Default)]
struct NativePacers {
    hal: NativePacer,
    hardware: HardwarePacer,
    application: ApplicationPacer,
    selected_application: Option<u64>,
}
impl NativePacers {
    fn select_application_master(&mut self, token: Option<u64>) {
        if token != self.selected_application
            && let Some(previous) = self.selected_application
        {
            // SAFETY: bounded registry lookup plus atomic publication only.
            unsafe {
                mr_engine_service_set_source_clock_master(previous, false);
            }
        }
        self.selected_application = token;
        if let Some(token) = token {
            // Idempotent reassertion also covers re-created producer slots.
            unsafe {
                mr_engine_service_set_source_clock_master(token, true);
            }
        }
    }
}
impl Drop for NativePacers {
    fn drop(&mut self) {
        self.select_application_master(None);
    }
}

fn application_clock(id: &str) -> Option<ApplicationClockSnapshot> {
    let mut clock = ApplicationClockSnapshot::default();
    // SAFETY: caller storage is fixed-size; registry reads bounded atomics.
    unsafe { mr_engine_service_source_clock(source_token(id), &raw mut clock) }.then_some(clock)
}

fn next_native_pace(
    pacers: &mut NativePacers,
    devices: &super::macos_devices::DeviceSet,
    plan: &RenderPlan,
    capacity: u32,
    endpoint: Option<&native_transport::EndpointSnapshot>,
) -> (Pace, u64) {
    let format = plan.format();
    let pace = capture_schedule::pace(
        &mut pacers.hal,
        endpoint,
        format.sample_rate,
        format.block_frames,
        capacity,
    );
    if matches!(pace, Pace::Idle | Pace::Unknown)
        && let Some(hardware) = devices.hardware_clock_progress()
    {
        devices.set_clock_master(Some(hardware.owner_id));
        pacers.select_application_master(None);
        return (
            pacers.hardware.next(
                (hardware.epoch, hardware.native_frames, hardware.native_rate),
                format.sample_rate,
                format.block_frames,
                capacity,
            ),
            2,
        );
    }
    devices.set_clock_master(None);
    if matches!(pace, Pace::Idle | Pace::Unknown) {
        for binding in plan.source_bindings() {
            let id = binding.node_id().as_str();
            if binding.virtual_endpoint_id().is_none()
                && let Some(clock) = application_clock(id)
                && clock.running != 0
            {
                pacers.select_application_master(Some(source_token(id)));
                return (
                    pacers.application.next(
                        (source_token(id), clock),
                        (format.sample_rate, format.block_frames, capacity),
                    ),
                    3,
                );
            }
        }
    }
    pacers.select_application_master(None);
    (pace, 1)
}

#[derive(Clone, Copy)]
struct WorkerCycle<'a> {
    timeline: &'a MacTimeline,
    frames: u32,
    discontinuity: bool,
    write_hal: bool,
    devices: &'a super::macos_devices::DeviceSet,
    status: &'a AdapterRuntimeStatus,
    endpoint: Option<native_transport::EndpointSnapshot>,
}

struct MacTimeline {
    frame: u64,
    epoch: u64,
    deadline: Instant,
    native: bool,
    clock_id: u64,
    worker_stream_identity: Option<([u8; 16], u64)>,
    discontinuity_pending: bool,
}

impl MacTimeline {
    fn new() -> Self {
        Self {
            frame: 0,
            epoch: 0,
            deadline: Instant::now(),
            native: false,
            clock_id: 0,
            worker_stream_identity: None,
            discontinuity_pending: false,
        }
    }
    fn reset(&mut self) {
        self.frame = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.deadline = Instant::now();
    }

    fn accept(
        &mut self,
        pacing: (Pace, u64),
        route: &RouteClock,
        status: &AdapterRuntimeStatus,
        endpoint: Option<&native_transport::EndpointSnapshot>,
    ) -> Option<(u32, bool)> {
        let (pace, clock_id) = pacing;
        match pace {
            Pace::Block {
                first_frame,
                frames,
                epoch,
                discontinuity,
                overrun,
            } => {
                let switched = !self.native || self.clock_id != clock_id;
                self.native = true;
                self.clock_id = clock_id;
                self.frame = first_frame;
                self.epoch = epoch;
                if overrun {
                    status.record_process(ProcessReport {
                        output_overflows: 1,
                        discontinuities: 1,
                        ..ProcessReport::default()
                    });
                }
                status.observe_clock(
                    mixrelay_audio_io::session::ClockAuthority::Device,
                    route.synchronized,
                );
                Some((frames, discontinuity || switched))
            }
            Pace::Wait | Pace::RateMismatch => None,
            Pace::Idle | Pace::Unknown => {
                let switched = self.native;
                if switched {
                    self.reset();
                }
                self.native = false;
                self.clock_id = 0;
                if pace == Pace::Unknown && route.has_hal {
                    let epoch =
                        endpoint.map(|snapshot| (snapshot.incarnation, snapshot.render_epoch));
                    if epoch != self.worker_stream_identity {
                        self.worker_stream_identity = epoch;
                        self.reset();
                    }
                }
                if route.independent_idle && (pace == Pace::Idle || !route.has_hal) {
                    status.observe_clock_idle();
                } else {
                    status.observe_clock(
                        mixrelay_audio_io::session::ClockAuthority::EngineTimer,
                        false,
                    );
                }
                Some((route.maximum_frames, switched || self.frame == 0))
            }
        }
    }
}

fn wait_for_block(deadline: &mut Instant, period: Duration) {
    *deadline += period;
    let now = Instant::now();
    if *deadline > now {
        std::thread::sleep(deadline.duration_since(now));
    } else if now.duration_since(*deadline) > period.saturating_mul(4) {
        *deadline = now;
    }
}

fn process_native_cycle(
    processor: &mut BlockProcessor,
    io: &mut MacEngineExternalIo<'_>,
    cycle: WorkerCycle<'_>,
) -> Result<ProcessReport, String> {
    let format = processor.plan().format();
    let WorkerCycle {
        timeline,
        frames,
        discontinuity,
        ..
    } = cycle;
    let clock_id = timeline.clock_id;
    processor
        .process_external_cycle(
            ProcessCycle {
                clock: mixrelay_audio_io::process::ClockDomain {
                    authority: if clock_id != 0 {
                        mixrelay_audio_io::session::ClockAuthority::Device
                    } else {
                        mixrelay_audio_io::session::ClockAuthority::EngineTimer
                    },
                    id: clock_id,
                    epoch: timeline.epoch,
                },
                sample_rate: format.sample_rate,
                frames,
                first_frame: timeline.frame,
                monotonic_ns: None,
                discontinuity,
            },
            io,
        )
        .map_err(|error| error.to_string())
}

fn block_sample_count(channels: u32, frames: u32) -> Result<usize, String> {
    // A one-way endpoint has no storage on its inactive side. Still validate
    // frame bounds; an absent stream must not hide an oversized callback.
    mixrelay_audio_io::format_contract::block_sample_count(
        channels.max(1),
        frames,
        u32::from(endpoint_backend::MAX_ENDPOINT_CHANNELS),
        u32::try_from(MAX_BUFFER_FRAMES).expect("product buffer limit fits u32"),
    )
    .map(|samples| if channels == 0 { 0 } else { samples })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod storage_tests {
    use super::block_sample_count;
    #[test]
    fn one_way_and_maximum_endpoints_have_bounded_storage() {
        for &frames in mixrelay_audio_io::formats::BUFFER_SIZES {
            assert_eq!(block_sample_count(0, frames), Ok(0));
            assert_eq!(block_sample_count(64, frames), Ok(64 * frames as usize));
        }
        assert!(block_sample_count(0, 2049).is_err());
        assert!(block_sample_count(1, 2049).is_err());
        assert!(block_sample_count(65, 32).is_err());
        assert!(block_sample_count(1, 0).is_err());
    }
}
