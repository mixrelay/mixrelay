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
mod worker;
use crate::worker_completion;
mod worker_runtime;
#[cfg(test)]
use super::macos_pacing::{ApplicationClockSnapshot, ApplicationPacer, Pace};
use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, Instant};
#[cfg(test)]
use worker::{
    MacTimeline, NativePacers, RouteClock, WorkerCycle, next_native_pace, process_native_cycle,
};

#[cfg(test)]
use external_io::source_token;
use mixrelay_audio_engine::{BlockProcessor, PeakMeterBank, ProcessorExchange, RenderPlan};
#[cfg(test)]
use mixrelay_audio_engine::{ExternalBinding, ExternalIo};
use mixrelay_audio_graph::{CompileContext, CompileLimits, GraphCompiler, RenderFormat};
#[cfg(test)]
use mixrelay_audio_io::process::ProcessCycle;
#[cfg(test)]
use mixrelay_audio_io::process::TransferOutcome;
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
#[cfg(test)]
use external_io::MacEngineExternalIo;
use external_io::{application_epoch, binding_sources, binding_token, read_application};
use worker::AudioContext;

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
    let result = shutdown::finish(result, retired, async {
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
    let monitor_devices = devices.clone();
    let (worker, mut completion) =
        worker::start(processor, exchange, devices, runtime_status.clone())
            .map_err(|error| format!("could not start audio worker: {error}"))?;
    let observers =
        maintenance::ControlObservers::start(&runtime_status, &controller, &monitor_devices)?;
    let serving = service
        .serve(
            controller,
            completion.shutdown(stop_requested(signals)),
            || {
                observers.stop_observing();
            },
        )
        .await
        .map_err(|error| error.to_string());
    let observing = observers.finish();
    let joined = worker.finish();
    let completed = completion.finish().await.map_err(|error| error.to_string());
    let processor_cleanup = reclaimer.finish();
    let latency_stopped = latency.finish();
    let reporting_stopped = runtime_latency.finish().await;
    shutdown::collect([
        ("control", serving),
        ("observers", observing),
        ("audio_join", joined),
        ("audio_result", completed),
        ("reclaimer", processor_cleanup),
        ("latency_observer", latency_stopped),
        ("runtime_latency", reporting_stopped),
    ])
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
