mod direct;
mod endpoint_backend;
mod format;
mod maintenance;
mod runtime_latency;
#[cfg(test)]
pub(crate) use runtime_latency::tests::feedback_processor as test_feedback_processor;
#[cfg(test)]
mod input_alignment_tests;
mod shutdown;
mod worker_runtime;
use super::macos_pacing::{ApplicationClockSnapshot, ApplicationPacer, HardwarePacer, Pace};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(test)]
use external_io::source_token;
#[cfg(test)]
use mixrelay_audio_engine::ExternalIo;
use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, PeakMeterBank, ProcessorExchange, RenderPlan,
};
use mixrelay_audio_graph::{CompileContext, CompileLimits, GraphCompiler, RenderFormat};
#[cfg(test)]
use mixrelay_audio_io::process::TransferOutcome;
use mixrelay_audio_io::process::{ProcessCycle, ProcessReport};
use mixrelay_engine_service::{
    EngineGraphController, EngineGraphControllerConfig, control_service::ControlService,
    default_peak_meter_bank, endpoint_service, session::AdapterRuntimeStatus,
};
use mixrelay_processor_host::ProcessorCatalog;

#[cfg(test)]
pub(super) fn adapter_profile() -> mixrelay_engine_service::session::AdapterProfile {
    format::profile(mixrelay_audio_io::formats::SAMPLE_RATES.to_vec())
}

use endpoint_backend::MacVirtualEndpointBackend;

mod external_io;
use external_io::{
    MacEngineExternalIo, application_epoch, binding_sources, binding_token, read_application,
};

fn prepare_runtime() -> AdapterRuntimeStatus {
    // The processing domain is independent of each endpoint's native rate.
    // Endpoint format changes are ID-directed through VirtualEndpointBackend.
    AdapterRuntimeStatus::new(format::profile(
        mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
    ))
    .with_graph_format_control()
}

pub(super) async fn run() -> Result<(), String> {
    use mixrelay_macos_transport::app_lifecycle::{CONTROLLER_LEASE, ENGINE_LEASE, ProcessLease};
    let Some(engine_lease) =
        ProcessLease::acquire_for_controller(ENGINE_LEASE, CONTROLLER_LEASE)
            .map_err(|error| format!("could not claim engine lifecycle: {error}"))?
    else {
        tracing::info!("controller is absent; refusing demand-started audio execution");
        return Ok(());
    };
    engine_lease
        .record("running")
        .map_err(|error| format!("engine lifecycle record failed: {error}"))?;
    // Stops are caught before anything can fail, and the crash guard covers the whole run.
    let signals = mixrelay_engine_service::session::ShutdownSignals::register()
        .map_err(|error| format!("could not observe shutdown requests: {error}"))?;
    let (guard, tripped) = mixrelay_engine_service::crash_guard::CrashGuard::arm(
        mixrelay_engine_service::config_directory(),
    );
    let result = run_owned(signals, tripped).await;
    endpoint_service::stop_virtual_sources();
    let retired = super::macos_devices::MacDevices::finish_retirement();
    // A killed controller cannot run its own Quit handler. Its OS-owned lease
    // still releases, so the surviving engine finalizes recordings and then
    // shuts down the independent HAL inventory, preserving the stopped state.
    let result = shutdown::finish(result.and(retired), async {
        mixrelay_driver_control::DriverClient::new()
            .map_err(|error| error.to_string())?
            .set_active(false)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await;
    let outcome = result
        .as_ref()
        .map_or_else(|error| format!("failed\n{error}"), |()| "stopped".into());
    engine_lease
        .record(&outcome)
        .map_err(|error| format!("engine exit record failed: {error}"))?;
    if result.is_ok()
        && let Some(guard) = guard
    {
        guard.finish();
    }
    result
}

async fn run_owned(
    signals: mixrelay_engine_service::session::ShutdownSignals,
    repeated_failures: bool,
) -> Result<(), String> {
    // Reserve the control endpoint before native publication or audio ownership.
    // A second service must fail without preparing devices beside the running one.
    let service = ControlService::bind(mixrelay_local_socket::socket_directory())
        .await
        .map_err(|error| error.to_string())?;
    initialize_native_transport()?;
    let mut runtime_status = prepare_runtime();
    let format = runtime_status.restore_preferences();
    let runtime_status = Arc::new(runtime_status);
    let meters = default_peak_meter_bank();
    let initial_document = mixrelay_audio_graph::GraphDocument::new(1, Vec::new(), Vec::new());
    let processor = create_initial_processor(&initial_document, format, Arc::clone(&meters))?;
    let exchange = Arc::new(ProcessorExchange::new(processor.plan().revision()));
    let latency = crate::latency_maintenance::Observer::start(&exchange, processor.plan())?;
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
    let controller = EngineGraphController::new_with_initial_document(
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
            paid_access: Arc::new(mixrelay_licensing::has_local_paid_access),
        },
        initial_document,
    );
    // Saved device references are resolved through Driver's blocking control API.
    // Recall must finish before audio starts, but outside the Tokio IO executor.
    let controller = restore_controller(controller, repeated_failures).await?;
    // Declared before AudioWorker so reclamation remains available until the
    // audio owner has stopped and joined, including partial-startup failures.
    let reclaimer =
        maintenance::start_reclaimer(Arc::downgrade(&exchange), Arc::downgrade(&devices))?;
    let runtime_latency = runtime_latency::Reporter::start(
        Arc::clone(&exchange),
        Arc::clone(&devices),
        mixrelay_control_server::ControlBackend::incarnation(controller.as_ref()),
    )?;
    let worker_status = runtime_status.clone();
    let monitor_devices = devices.clone();
    let audio_thread = std::thread::Builder::new()
        .name("mixrelay-audio-worker".into())
        .spawn(move || {
            if let Err(error) = run_audio_worker(
                processor,
                AudioContext {
                    exchange: &exchange,
                    runtime_status: &worker_status,
                    devices: &devices,
                },
            ) {
                tracing::error!(%error, "MixRelay audio worker failed");
            }
        })
        .map_err(|error| format!("could not start audio worker: {error}"))?;
    let worker =
        mixrelay_engine_service::session::AudioWorker::new(runtime_status.clone(), audio_thread);
    let observers =
        maintenance::ControlObservers::start(&runtime_status, &controller, &monitor_devices)?;
    service
        .serve(controller, stop_requested(signals), || {
            observers.stop_observing();
        })
        .await
        .map_err(|error| error.to_string())
        .and(observers.finish())
        .and(worker.finish())
        .and(reclaimer.finish())
        .and(latency.finish())
        .and(runtime_latency.finish().await)
}

async fn restore_controller(
    controller: EngineGraphController,
    repeated_failures: bool,
) -> Result<Arc<EngineGraphController>, String> {
    tokio::task::spawn_blocking(move || {
        Arc::new(
            controller
                .after_repeated_failures(repeated_failures)
                .with_graph_catalog(),
        )
    })
    .await
    .map_err(|error| format!("could not restore graph catalog: {error}"))
}

/// An OS shutdown request or the controller releasing the engine.
async fn stop_requested(
    signals: mixrelay_engine_service::session::ShutdownSignals,
) -> std::io::Result<()> {
    tokio::select! {
        result = signals.requested() => result,
        () = controller_released() => Ok(()),
    }
}

async fn controller_released() {
    shutdown::wait_for_controller(mixrelay_macos_transport::app_lifecycle::CONTROLLER_LEASE).await;
}

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
    BlockProcessor::with_peak_meters(Arc::new(render_plan), meters)
        .map(Box::new)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
struct AudioContext<'a> {
    exchange: &'a ProcessorExchange,
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
        // Same-spec native recovery must not wait for an unrelated graph edit.
        activation.activate_recovered(active_devices);
        self.exchange.activate_pending_configured(processor, |_| {
            activation.activate(active_devices);
            Ok::<(), String>(())
        })
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
    let mut timeline = MacTimeline::new();
    let mut pacers = NativePacers::default();
    let mut awaiting_format = false;
    let mut runtime = worker_runtime::WorkerRuntime::new();
    // Whether a direct (IOProc-rendered) plan must be looked for again. Looking allocates
    // and queries devices, so it happens off realtime and only after something changed,
    // or once a second in case a device became eligible on its own.
    let mut direct_recheck = true;
    let mut direct_checked = Instant::now();
    let result = (|| {
        while !runtime_status.stop_requested() {
            if runtime_status.checkpoint() {
                timeline.reset();
                pacers = NativePacers::default();
                runtime.off_realtime(|| std::thread::sleep(Duration::from_millis(1)));
                timeline.deadline = Instant::now();
                continue;
            }
            // Activation is a lock-free handoff of prepared plans and device sets (atomic
            // checks, then pointer swaps; retirement happens elsewhere), so it stays in the
            // realtime class.
            if context.activate(&mut processor, &mut active_devices, &mut activation)? {
                timeline.reset();
                pacers = NativePacers::default();
                direct_recheck = true;
            }
            let format = processor.plan().format();
            if !active_devices.processing_ready(format.sample_rate) {
                if !std::mem::replace(&mut awaiting_format, true) {
                    runtime.off_realtime(|| {
                        tracing::info!(
                            processing_rate = format.sample_rate,
                            "CoreAudio rendering gated until device format is ready"
                        );
                    });
                }
                runtime_status.observe_clock_idle();
                timeline.reset();
                pacers = NativePacers::default();
                direct_recheck = true;
                runtime.off_realtime(|| std::thread::sleep(Duration::from_millis(1)));
                continue;
            }
            if std::mem::take(&mut awaiting_format) {
                runtime.off_realtime(|| {
                    tracing::info!(
                        processing_rate = format.sample_rate,
                        "CoreAudio device format ready; rendering admitted"
                    );
                });
            }
            runtime_status.activate_format(format);
            if direct_recheck || direct_checked.elapsed() >= Duration::from_secs(1) {
                direct_recheck = false;
                direct_checked = Instant::now();
                if run_direct(&mut runtime, &active_devices, &mut processor, context)? {
                    timeline.reset();
                    pacers = NativePacers::default();
                    direct_recheck = true;
                    continue;
                }
            }
            process_worker_block(
                &mut timeline,
                &mut pacers,
                &mut WorkerBlock {
                    processor: &mut processor,
                    devices: &active_devices,
                    status: runtime_status,
                    runtime: &mut runtime,
                },
            )?;
        }
        Ok(())
    })();
    runtime.finish();
    result
}

/// Hands rendering to a device `IOProc` when a direct plan exists, and supervises it until
/// it ends. Looking for, running and dropping the plan all happen off realtime.
fn run_direct(
    runtime: &mut worker_runtime::WorkerRuntime,
    devices: &super::macos_devices::DeviceSet,
    processor: &mut Box<BlockProcessor>,
    context: AudioContext<'_>,
) -> Result<bool, String> {
    let plan = runtime.off_realtime(|| {
        super::macos_devices::direct::DirectPlan::prepare(devices, processor.plan())
    });
    let Some(plan) = plan else {
        return Ok(false);
    };
    runtime.record(worker_runtime::Path::Direct);
    runtime.off_realtime(|| {
        let result = direct::run(&plan, processor, context);
        drop(plan);
        result
    })?;
    Ok(true)
}

struct WorkerBlock<'a> {
    processor: &'a mut BlockProcessor,
    devices: &'a super::macos_devices::DeviceSet,
    status: &'a AdapterRuntimeStatus,
    runtime: &'a mut worker_runtime::WorkerRuntime,
}

fn process_worker_block(
    timeline: &mut MacTimeline,
    pacers: &mut NativePacers,
    block: &mut WorkerBlock<'_>,
) -> Result<(), String> {
    let route = RouteClock::observe(block.processor, block.devices);
    let (pace, clock_id) = next_native_pace(pacers, block.devices, block.processor.plan(), 16_384);
    let Some((frames, discontinuity)) = timeline.accept((pace, clock_id), &route, block.status)
    else {
        // Waking this thread creates no credit; only the selected native clock
        // advances the processor's device-owned frame coordinates.
        block.runtime.wait_for_credit(clock_id);
        return Ok(());
    };
    let format = block.processor.plan().format();
    let period = Duration::from_secs_f64(f64::from(frames) / f64::from(format.sample_rate));
    // The scheduling budget follows the wake cadence, not this (possibly partial) block:
    // once per device callback under a device clock, once per graph block otherwise.
    let block_period =
        Duration::from_secs_f64(f64::from(format.block_frames) / f64::from(format.sample_rate));
    let wake_period = (clock_id == worker_runtime::HARDWARE_CLOCK)
        .then(|| block.devices.hardware_clock_progress())
        .flatten()
        .filter(|hardware| hardware.native_rate != 0 && hardware.quantum != 0)
        .map_or(block_period, |hardware| {
            Duration::from_secs_f64(f64::from(hardware.quantum) / f64::from(hardware.native_rate))
        });
    block.runtime.render_at(wake_period);
    block.runtime.record(match clock_id {
        worker_runtime::HARDWARE_CLOCK => worker_runtime::Path::Hardware,
        worker_runtime::SHARED_SOURCE_CLOCK => worker_runtime::Path::SharedSource,
        _ => worker_runtime::Path::Timer,
    });
    let mut io = MacEngineExternalIo::new(block.devices);
    io.alignment = std::mem::take(&mut timeline.capture_alignment);
    let result = process_native_cycle(
        block.processor,
        &mut io,
        WorkerCycle {
            timeline,
            frames,
            discontinuity: discontinuity || timeline.discontinuity_pending,
        },
    );
    timeline.capture_alignment = io.alignment;
    let report = result?;
    block
        .devices
        .record_latency_cycle(block.processor, report, timeline.epoch);
    runtime_latency::record_sources(
        block.processor,
        io.sources,
        report.is_some(),
        timeline.epoch,
    );
    timeline.discontinuity_pending = report.is_none();
    if let Some(report) = report {
        block.status.record_process(report);
    }
    timeline.frame += u64::from(frames);
    if !timeline.native {
        wait_for_block(&mut timeline.deadline, period);
    }
    Ok(())
}

struct RouteClock {
    maximum_frames: u32,
    synchronized: bool,
    independent_idle: bool,
}
impl RouteClock {
    fn observe(processor: &BlockProcessor, devices: &super::macos_devices::DeviceSet) -> Self {
        let mut result = Self {
            maximum_frames: processor.plan().format().block_frames,
            synchronized: true,
            independent_idle: true,
        };
        for (input, binding) in [
            (true, processor.plan().source_bindings()),
            (false, processor.plan().sink_bindings()),
        ]
        .into_iter()
        .flat_map(|(input, bindings)| bindings.iter().map(move |binding| (input, binding)))
        {
            let id = binding.node_id().as_str();
            if devices.is_binding_inactive(id, input) {
                continue;
            }
            if let Some(ready) = devices.clock_synchronized(id, input) {
                result.synchronized &= ready;
                result.independent_idle = false;
            } else if let Some(clock) = binding_clock(binding) {
                result.synchronized &= clock.synchronized != 0
                    && clock.graph_rate == processor.plan().format().sample_rate;
                result.independent_idle &= clock.running == 0;
            } else {
                result.synchronized = false;
                result.independent_idle = false;
            }
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Application,
    Virtual,
}

#[derive(Default)]
struct NativePacers {
    hardware: HardwarePacer,
    application: ApplicationPacer,
    selected_application: Option<(u64, SourceKind)>,
}
impl NativePacers {
    fn select_application_master(&mut self, token: Option<(u64, SourceKind)>) {
        if token != self.selected_application
            && let Some(previous) = self.selected_application
        {
            set_source_master(previous, false);
        }
        self.selected_application = token;
        if let Some(token) = token {
            set_source_master(token, true);
        }
    }
}
impl Drop for NativePacers {
    fn drop(&mut self) {
        self.select_application_master(None);
    }
}

fn set_source_master((token, kind): (u64, SourceKind), master: bool) {
    let sources = match kind {
        SourceKind::Virtual => endpoint_service::virtual_source_registry(),
        SourceKind::Application => endpoint_service::source_registry(),
    };
    if let Some(sources) = sources {
        sources.set_clock_master(token, master);
    }
}

fn binding_clock(binding: &ExternalBinding) -> Option<ApplicationClockSnapshot> {
    let clock = binding_sources(binding, endpoint_service::source_registry())?
        .clock(binding_token(binding))?;
    Some(ApplicationClockSnapshot {
        epoch: clock.epoch,
        native_frames: clock.native_frames,
        native_rate: clock.native_rate,
        graph_rate: clock.graph_rate,
        running: clock.running,
        synchronized: clock.synchronized,
    })
}

fn next_native_pace(
    pacers: &mut NativePacers,
    devices: &super::macos_devices::DeviceSet,
    plan: &RenderPlan,
    capacity: u32,
) -> (Pace, u64) {
    // Device-owned frame credit remains authoritative across Driver and physical IOProcs.
    if let Some(hardware) = devices.hardware_clock_progress() {
        let format = plan.format();
        devices.set_clock_master(Some(hardware.owner_id));
        devices.set_graph_quantum(
            u32::try_from(
                (u64::from(hardware.quantum) * u64::from(format.sample_rate))
                    .div_ceil(u64::from(hardware.native_rate)),
            )
            .unwrap_or(u32::MAX)
            .max(format.block_frames),
        );
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
    let format = plan.format();
    devices.set_clock_master(None);
    devices.set_graph_quantum(format.block_frames);
    for binding in plan.source_bindings() {
        if let Some(clock) = binding_clock(binding)
            && clock.running != 0
        {
            pacers.select_application_master(Some((
                binding_token(binding),
                if binding.virtual_endpoint_id().is_some() {
                    SourceKind::Virtual
                } else {
                    SourceKind::Application
                },
            )));
            return (
                pacers.application.next(
                    (binding_token(binding), clock),
                    (format.sample_rate, format.block_frames, capacity),
                ),
                3,
            );
        }
    }
    pacers.select_application_master(None);
    (Pace::Unknown, 0)
}

#[derive(Clone, Copy)]
struct WorkerCycle<'a> {
    timeline: &'a MacTimeline,
    frames: u32,
    discontinuity: bool,
}

struct MacTimeline {
    capture_alignment: mixrelay_audio_io::capture_alignment::CaptureAlignment,
    frame: u64,
    epoch: u64,
    deadline: Instant,
    native: bool,
    clock_id: u64,
    discontinuity_pending: bool,
}

impl MacTimeline {
    fn new() -> Self {
        Self {
            capture_alignment: mixrelay_audio_io::capture_alignment::CaptureAlignment::default(),
            frame: 0,
            epoch: 0,
            deadline: Instant::now(),
            native: false,
            clock_id: 0,
            discontinuity_pending: false,
        }
    }
    fn reset(&mut self) {
        self.capture_alignment = mixrelay_audio_io::capture_alignment::CaptureAlignment::default();
        self.frame = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.deadline = Instant::now();
    }

    fn accept(
        &mut self,
        pacing: (Pace, u64),
        route: &RouteClock,
        status: &AdapterRuntimeStatus,
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
                if route.independent_idle {
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
) -> Result<Option<ProcessReport>, String> {
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
