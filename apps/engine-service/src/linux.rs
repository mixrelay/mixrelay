use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, ExternalIo, PeakMeterBank, ProcessorExchange, RenderPlan,
};
use mixrelay_audio_graph::{
    ApplicationCaptureRequest, CompileContext, CompileLimits, FROM_OS_PORT_ID, GraphCompiler,
    GraphDocument, RenderFormat, TO_OS_PORT_ID,
};
use mixrelay_audio_io::process::{CycleBlocks, ProcessCycle, ProcessReport, TransferOutcome};
use mixrelay_audio_io::session::{AudioBackendKind, CURRENT_TIMER_FORMAT, ClockAuthority};
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities, VirtualEndpointSpec,
};
use mixrelay_engine_service::{
    EngineGraphController, EngineGraphControllerConfig, GraphResourceBackend,
    control_service::ControlService, default_peak_meter_bank, session::AdapterRuntimeStatus,
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
#[path = "linux_application_identity.rs"]
mod application_identity;
#[path = "linux_applications.rs"]
mod applications;
#[path = "linux_clock.rs"]
mod clock;
#[path = "linux_devices.rs"]
mod devices;
#[path = "linux_endpoints.rs"]
mod endpoints;
#[path = "linux_format.rs"]
mod format;
use mixrelay_pipewire as native;
#[cfg(test)]
#[path = "linux_resource_tests.rs"]
mod resource_tests;
#[path = "linux_source_mute.rs"]
mod source_mute;
use applications::{CaptureSet as PipeWireCaptureSet, CaptureWatch};
use devices::{DeviceStreams, DeviceWatch};
use endpoints::{EndpointControls, EndpointEdit, EndpointSet, EndpointSpecs};
use mixrelay_audio_io::recovery::{RecoveryConsumer, RecoveryMailbox};
use mixrelay_engine_service::resource_requirements::{DeviceRequirements, ResourceRequirements};
use mixrelay_pipewire::{DriverCycle, DriverFormat, DriverProcess, GraphDriver};
use native::NativeStream;
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

const fn native_format(format: RenderFormat) -> native::StreamFormat {
    native::StreamFormat {
        sample_rate: format.sample_rate,
        block_frames: format.block_frames,
    }
}

pub(super) fn run(owner: Option<mixrelay_engine_process::EngineOwner>) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not initialize Linux async runtime: {error}"))?;
    // Admission precedes publishing PipeWire endpoints or starting audio owners.
    let service = runtime
        .block_on(ControlService::bind())
        .map_err(|error| error.to_string())?;
    let mut runtime_status = AdapterRuntimeStatus::new(adapter_profile())
        .with_graph_format_control()
        .with_observed_rate_following(format::observed_endpoint_rate);
    let initial_format = runtime_status.restore_preferences();
    let endpoint_backend = LinuxVirtualEndpointBackend::new();
    endpoint_backend
        .active_format
        .store(format::pack(initial_format), Ordering::Release);
    let runtime_status = Arc::new(runtime_status);
    let meters = default_peak_meter_bank();
    let initial_document = GraphDocument::new(1, Vec::new(), Vec::new());
    let processor =
        create_initial_processor(&initial_document, Arc::clone(&meters), initial_format)?;
    let exchange = Arc::new(ProcessorExchange::new(processor.plan().revision()));
    let _latency = crate::latency_maintenance::Observer::start(&exchange, processor.plan())?;
    let controller = Arc::new(
        EngineGraphController::new_with_initial_document(
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
            initial_document,
        )
        .with_graph_catalog(),
    );

    let _reclaimer = spawn_reclaimers(&exchange, &endpoint_backend)?;
    let _recovery = spawn_device_recovery(&endpoint_backend, &exchange)?;
    let worker_status = runtime_status.clone();
    let audio_thread = thread::Builder::new()
        .name("mixrelay-pipewire-audio".to_owned())
        .spawn(move || {
            run_audio_worker(processor, exchange, &endpoint_backend, &worker_status);
        })
        .map_err(|error| format!("could not start PipeWire audio worker: {error}"))?;
    let _worker = mixrelay_engine_service::session::AudioWorker::new(runtime_status, audio_thread);
    let format_monitor = format::Monitor::start(&controller)?;
    let maintenance = controller.start_control_maintenance()?;

    let result = runtime
        .block_on(service.serve(
            controller,
            mixrelay_engine_process::shutdown(
                owner,
                mixrelay_engine_service::session::shutdown_signal(),
            ),
            || {
                format_monitor.stop_observing();
                maintenance.stop_observing();
            },
        ))
        .map_err(|error| error.to_string());
    drop(format_monitor);
    result.and(maintenance.finish())
}

fn create_initial_processor(
    document: &GraphDocument,
    meters: Arc<PeakMeterBank>,
    format: RenderFormat,
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
) -> Result<mixrelay_engine_service::resource_reclaimer::ResourceReclaimer, String> {
    let weak_exchange = Arc::downgrade(exchange);
    let weak_endpoints = Arc::downgrade(endpoints);
    mixrelay_engine_service::resource_reclaimer::ResourceReclaimer::start(
        "mixrelay-linux-reclaimer",
        move || {
            let Some(exchange) = weak_exchange.upgrade() else {
                return false;
            };
            exchange.reclaim_retired();
            drop(exchange);
            let Some(endpoints) = weak_endpoints.upgrade() else {
                return false;
            };
            endpoints.reclaim_retired();
            true
        },
    )
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
    exchange: &Arc<ProcessorExchange>,
) -> Result<DeviceRecoveryWorker, String> {
    let weak = Arc::downgrade(backend);
    let weak_exchange = Arc::downgrade(exchange);
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
                if let Some(exchange) = weak_exchange.upgrade() {
                    backend.poll_source_mute(&exchange);
                }
                if backend.device_recovery.idle() {
                    // Do not hold the configuration mutex across discovery/open.
                    let watch = backend
                        .device_watch
                        .lock()
                        .ok()
                        .and_then(|watch| watch.clone());
                    if let Some(watch) = watch
                        && watch.needs_observation()
                        && let Ok(mut prepared) = DeviceStreams::recover(&watch, &stop)
                        && !prepared.is_empty()
                        && !stop.load(Ordering::Acquire)
                        && backend.bind_device_inputs(&mut prepared).is_ok()
                    {
                        // A concurrent edit may have replaced/deleted these nodes.
                        // The audio epoch and per-stream lease checks reject stale results.
                        let _ = backend.device_recovery.publish(watch.generation, prepared);
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
    endpoints: Box<EndpointSet>,
    captures: Box<PipeWireCaptureSet>,
    devices: Box<DeviceStreams>,
    status: Arc<AdapterRuntimeStatus>,
    timeline: clock::GraphTimeline,
    clock_diagnostics: Arc<clock::DiscontinuityMailbox>,
}

fn run_audio_worker(
    processor: Box<BlockProcessor>,
    exchange: Arc<ProcessorExchange>,
    endpoint_backend: &LinuxVirtualEndpointBackend,
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
    let initial_format = processor.plan().format();
    let driver_format =
        match DriverFormat::new(initial_format.sample_rate, initial_format.block_frames) {
            Ok(format) => format,
            Err(error) => {
                tracing::error!(%error, "could not prepare PipeWire graph driver format");
                return;
            }
        };
    let clock_diagnostics = Arc::new(clock::DiscontinuityMailbox::default());
    let context = LinuxProcessState {
        processor,
        exchange,
        endpoint_backend,
        application_recovery,
        device_recovery,
        endpoints: Box::default(),
        captures: Box::default(),
        devices: Box::default(),
        status: status.clone(),
        timeline: clock::GraphTimeline::new(initial_format.sample_rate),
        clock_diagnostics: Arc::clone(&clock_diagnostics),
    };
    let driver = match GraphDriver::open(context, driver_format) {
        Ok(driver) => driver,
        Err(error) => {
            tracing::error!(%error, "could not start PipeWire graph driver");
            return;
        }
    };
    let started = Instant::now();
    let mut last_clock_event = 0;
    while !driver.ready() && !status.stop_requested() {
        if started.elapsed() > Duration::from_secs(5) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    while !status.stop_requested() && driver.ready() {
        // Control-plane health/shutdown wait, NOT an audio scheduling clock.
        report_clock_discontinuity(&clock_diagnostics, &mut last_clock_event);
        thread::sleep(Duration::from_millis(20));
    }
    drop(driver); // The driver owns and drains context before releasing its resources.
    report_clock_discontinuity(&clock_diagnostics, &mut last_clock_event);
}

fn report_clock_discontinuity(mailbox: &clock::DiscontinuityMailbox, last_event: &mut u64) {
    let Some(read) = mailbox.read_since(*last_event) else {
        return;
    };
    *last_event = read.event_number;
    let previous = read.event.previous;
    let current = read.event.current;
    tracing::warn!(
        event_number = read.event_number,
        overwritten = read.overwritten,
        previous_clock_id = previous.clock_id,
        previous_rate = previous.rate,
        previous_position = previous.position,
        previous_frames = previous.frames,
        previous_monotonic_ns = previous.monotonic_ns,
        current_clock_id = current.clock_id,
        current_rate = current.rate,
        current_position = current.position,
        current_frames = current.frames,
        current_monotonic_ns = current.monotonic_ns,
        mapped_epoch = read.event.mapped_epoch,
        render_position = read.event.render_position,
        "PipeWire native timeline discontinuity"
    );
}

#[cfg(test)]
mod clock_diagnostic_tests {
    use super::*;

    #[test]
    fn control_poll_reports_each_mailbox_event_once() {
        let mailbox = clock::DiscontinuityMailbox::default();
        let previous = DriverCycle {
            position: 0,
            monotonic_ns: 1,
            clock_id: 2,
            rate: 48_000,
            frames: 128,
        };
        let event = clock::NativeDiscontinuity {
            previous,
            current: DriverCycle {
                position: 256,
                monotonic_ns: 2,
                ..previous
            },
            mapped_epoch: 2,
            render_position: 128,
        };
        let mut last_event = 0;
        report_clock_discontinuity(&mailbox, &mut last_event);
        assert_eq!(last_event, 0);
        mailbox.publish(event);
        report_clock_discontinuity(&mailbox, &mut last_event);
        assert_eq!(last_event, 1);
        report_clock_discontinuity(&mailbox, &mut last_event);
        assert_eq!(last_event, 1);
    }
}

impl DriverProcess for LinuxProcessState<'_> {
    fn prepare_inputs(
        &mut self,
        inputs: &mixrelay_pipewire::GraphInputs,
    ) -> Result<(), mixrelay_pipewire::DriverError> {
        self.endpoints
            .bind_inputs(inputs)
            .map_err(mixrelay_pipewire::DriverError::Input)?;
        self.captures
            .bind_inputs(inputs)
            .map_err(mixrelay_pipewire::DriverError::Input)?;
        self.devices
            .bind_inputs(inputs)
            .map_err(mixrelay_pipewire::DriverError::Input)?;
        self.endpoint_backend.register_inputs(inputs)
    }
    fn process(&mut self, native: &DriverCycle) {
        if self.status.stop_requested() || self.status.checkpoint() {
            return;
        }
        if self.process_cycle(native).is_err() {
            self.status.request_stop();
        }
    }
}

impl LinuxProcessState<'_> {
    fn process_cycle(&mut self, native: &DriverCycle) -> Result<(), ()> {
        let previous_format = self.processor.plan().format();
        let activated = self
            .exchange
            .activate_pending_configured(&mut self.processor, |revision| {
                self.endpoint_backend.activate_pending(
                    revision,
                    &mut self.endpoints,
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
        format::observe_endpoints(&self.endpoints);
        let cycle = self.timeline.map(native)?;
        if let Some(event) = self.timeline.take_discontinuity() {
            self.clock_diagnostics.publish(event);
        }
        self.status
            .observe_clock(ClockAuthority::AudioServer, false);
        if cycle.frames == 0 {
            // A valid native iteration still owes the manual output dependency,
            // even when duration mapping has not accumulated one PCM frame yet.
            process_blocks_and_release(self, None, Self::process_block, Self::trigger_outputs)?;
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
        process_blocks_and_release(
            self,
            Some(blocks),
            Self::process_block,
            Self::trigger_outputs,
        )?;
        // A node.group preference is not proof of a shared driver. Report only
        // actual native callback IDs after every currently active owner agrees.
        let synchronized = self.endpoints.shares_clock(native.clock_id, native.rate)
            && self.captures.shares_clock(native.clock_id, native.rate)
            && self.devices.shares_clock(native.clock_id, native.rate);
        self.status
            .observe_clock(ClockAuthority::AudioServer, synchronized);
        Ok(())
    }

    fn trigger_outputs(&mut self) {
        self.endpoints.trigger_outputs();
        self.devices.trigger_outputs();
    }

    fn process_block(&mut self, cycle: ProcessCycle) -> Result<(), ()> {
        self.devices
            .observe_controls(self.processor.plan().controls());
        self.application_recovery
            .activate(self.captures.generation, |next| {
                self.captures.replace(next);
            });
        self.device_recovery
            .activate(self.devices.generation, |next| {
                self.devices.replace_disconnected(next);
            });
        self.status
            .observe_endpoint(!self.endpoints.is_empty() && self.endpoints.ready());
        let report = self
            .processor
            .process_external_cycle(
                cycle,
                &mut LinuxExternalIo {
                    endpoints: &mut self.endpoints,
                    application_captures: &mut self.captures,
                    devices: &mut self.devices,
                },
            )
            .map_err(|_| ())?;
        self.status
            .record_process(self.endpoints.finish_block(cycle.frames)?);
        let Some(report) = report else {
            return Ok(());
        };
        self.status.record_process(report);
        self.status.record_process(self.devices.take_report());
        self.status.record_process(self.endpoints.take_report());
        self.status.record_process(self.captures.take_report());
        self.status.observe_application_state(self.captures.state());
        Ok(())
    }
}

// Static callbacks keep the native-cycle sequencing testable without allocating
// or introducing a second dispatch/ownership path in the real-time process.
fn process_blocks_and_release<Owner>(
    owner: &mut Owner,
    blocks: Option<CycleBlocks>,
    mut process_block: impl FnMut(&mut Owner, ProcessCycle) -> Result<(), ()>,
    release_outputs: impl FnOnce(&mut Owner),
) -> Result<(), ()> {
    for block in blocks.into_iter().flatten() {
        process_block(owner, block)?;
    }
    release_outputs(owner);
    Ok(())
}

#[cfg(test)]
mod native_output_release_tests {
    use super::*;

    fn blocks() -> CycleBlocks {
        CycleBlocks::new(
            ProcessCycle {
                clock: mixrelay_audio_io::process::ClockDomain {
                    authority: ClockAuthority::AudioServer,
                    id: 26,
                    epoch: 1,
                },
                sample_rate: 48_000,
                frames: 512,
                first_frame: 0,
                monotonic_ns: Some(0),
                discontinuity: false,
            },
            mixrelay_audio_io::session::SessionFormat {
                sample_rate: 48_000,
                block_frames: 128,
            },
        )
        .unwrap()
    }

    #[test]
    fn output_release_follows_all_four_slices_once() {
        let mut counts = (0, 0);
        process_blocks_and_release(
            &mut counts,
            Some(blocks()),
            |counts, block| {
                assert_eq!(block.frames, 128);
                counts.0 += 1;
                assert_eq!(counts.1, 0);
                Ok(())
            },
            |counts| {
                assert_eq!(counts.0, 4);
                counts.1 += 1;
            },
        )
        .unwrap();
        assert_eq!(counts, (4, 1));
        counts = (0, 0);
        process_blocks_and_release(
            &mut counts,
            None,
            |_, _| panic!("zero render budget has no DSP slice"),
            |counts| counts.1 += 1,
        )
        .unwrap();
        assert_eq!(counts, (0, 1));
    }

    #[test]
    fn failed_slice_does_not_release_partial_cycle() {
        let mut counts = (0, 0);
        let result = process_blocks_and_release(
            &mut counts,
            Some(blocks()),
            |counts, _| {
                counts.0 += 1;
                if counts.0 == 2 { Err(()) } else { Ok(()) }
            },
            |counts| counts.1 += 1,
        );
        assert_eq!(result, Err(()));
        assert_eq!(counts, (2, 0));
    }
}

struct LinuxExternalIo<'a> {
    endpoints: &'a mut EndpointSet,
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
        if binding.port_id().as_str() == FROM_OS_PORT_ID {
            return self.endpoints.read(binding.node_id().as_str(), destination);
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
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if binding.port_id().as_str() == TO_OS_PORT_ID {
            return self.endpoints.write(binding.node_id().as_str(), source);
        }
        if binding.port_id().as_str() == mixrelay_audio_graph::AUDIO_PORT_ID {
            return self.devices.write(binding.node_id().as_str(), source);
        }
        TransferOutcome::complete(0)
    }
}

struct LinuxVirtualEndpointBackend {
    driver_client: Result<mixrelay_driver_control::DriverClient, String>,
    self_ref: Weak<Self>,
    active_format: AtomicU64,
    format_reserved: AtomicBool,
    capabilities: VirtualEndpointCapabilities,
    active_spec: Mutex<LinuxResourceSpec>,
    endpoint_controls: Mutex<EndpointControls>,
    pending: AtomicPtr<LinuxResourceUpdate>,
    retired: AtomicPtr<LinuxResourceUpdate>,
    device_generation: AtomicU64,
    device_watch: Mutex<Option<DeviceWatch>>,
    device_recovery: RecoveryMailbox<DeviceStreams>,
    application_generation: AtomicU64,
    application_watch: Mutex<Option<CaptureWatch>>,
    application_recovery: RecoveryMailbox<PipeWireCaptureSet>,
    // Weak, control-only access. None is initial setup, never shutdown fallback.
    graph_inputs: Mutex<Option<mixrelay_pipewire::GraphInputs>>,
    playback: source_mute::PlaybackController,
    #[cfg(test)]
    automatic_device_outputs: bool,
}

impl LinuxVirtualEndpointBackend {
    fn new() -> Arc<Self> {
        Arc::new_cyclic(Self::prepare)
    }

    #[cfg(test)]
    fn new_standalone_device_publishers() -> Arc<Self> {
        Arc::new_cyclic(|self_ref| {
            let mut backend = Self::prepare(self_ref);
            backend.automatic_device_outputs = true;
            backend
        })
    }

    fn prepare(self_ref: &std::sync::Weak<Self>) -> Self {
        Self {
            driver_client: mixrelay_driver_control::DriverClient::new()
                .map_err(|error| error.to_string()),
            self_ref: self_ref.clone(),
            active_format: AtomicU64::new(format::pack(render_format())),
            format_reserved: AtomicBool::new(false),
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: MAX_ENDPOINT_CHANNELS,
                max_capture_channels: MAX_ENDPOINT_CHANNELS,
                supported_sample_rates: format::RATES.to_vec(),
                supports_duplex: true,
                supports_dynamic_creation: true,
            },
            active_spec: Mutex::new(LinuxResourceSpec {
                endpoints: EndpointSpecs::new(),
                captures: Vec::new(),
                devices: DeviceRequirements::default(),
            }),
            endpoint_controls: Mutex::default(),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
            device_generation: AtomicU64::new(0),
            device_watch: Mutex::new(None),
            device_recovery: RecoveryMailbox::default(),
            application_generation: AtomicU64::new(0),
            application_watch: Mutex::new(None),
            application_recovery: RecoveryMailbox::default(),
            graph_inputs: Mutex::new(None),
            playback: source_mute::PlaybackController::default(),
            #[cfg(test)]
            automatic_device_outputs: false,
        }
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
        let Ok(Some((mut prepared, next_watch))) = watch.recover(stopping) else {
            return;
        };
        if stopping.load(Ordering::Acquire) {
            return;
        }
        if self.bind_application_inputs(&mut prepared).is_err() {
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
            && let Some(current) = registered.as_mut()
        {
            current.refresh_leases(&next_watch);
        }
    }

    fn poll_source_mute(&self, exchange: &ProcessorExchange) {
        let watch = self
            .application_watch
            .lock()
            .ok()
            .and_then(|value| value.clone());
        let spec = self.active_spec.lock().ok().map(|value| value.clone());
        if let Some(spec) = spec {
            let driver = if watch.is_some() && !spec.endpoints.is_empty() {
                self.driver_snapshot().ok()
            } else {
                None
            };
            self.playback.poll(
                exchange,
                &source_mute::PlaybackResources {
                    watch: watch.as_ref(),
                    endpoints: &spec.endpoints,
                    driver: driver.as_ref(),
                },
            );
        }
    }

    fn prepare_capture_edit(
        &self,
        requests: &[(String, ApplicationCaptureRequest)],
        format: RenderFormat,
    ) -> Result<(Box<PipeWireCaptureSet>, CaptureWatch), HostAudioBackendError> {
        let generation = self.application_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let previous = self
            .application_watch
            .lock()
            .map_err(|_| HostAudioBackendError::BackendFailure {
                message: "application recovery state is poisoned".into(),
            })?
            .clone();
        PipeWireCaptureSet::open_format(requests, generation, format, previous.as_ref())
            .map(|(captures, watch)| (Box::new(captures), watch))
            .map_err(|message| HostAudioBackendError::BackendFailure { message })
    }

    fn prepare_device_edit(
        &self,
        spec: &DeviceRequirements,
        format: RenderFormat,
    ) -> Result<Box<DeviceStreams>, HostAudioBackendError> {
        let generation = self.device_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let previous = self
            .device_watch
            .lock()
            .map_err(|_| HostAudioBackendError::BackendFailure {
                message: "device recovery state is poisoned".into(),
            })?
            .clone();
        #[cfg(test)]
        if self.automatic_device_outputs {
            return DeviceStreams::open_standalone_format(
                spec,
                generation,
                format,
                previous.as_ref(),
            )
            .map(Box::new)
            .map_err(|message| HostAudioBackendError::BackendFailure { message });
        }
        DeviceStreams::open_format(spec, generation, format, previous.as_ref())
            .map(Box::new)
            .map_err(|message| HostAudioBackendError::BackendFailure { message })
    }

    fn activate_pending(
        &self,
        revision: u64,
        endpoints: &mut Box<EndpointSet>,
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
        if update
            .captures
            .as_ref()
            .is_some_and(|next| !next.can_retain_from(captures))
        {
            self.pending.store(Box::into_raw(update), Ordering::Release);
            return Err("PipeWire retained captures no longer match the active source leases");
        }
        if update
            .devices
            .as_ref()
            .is_some_and(|next| !next.can_retain_from(devices))
        {
            self.pending.store(Box::into_raw(update), Ordering::Release);
            return Err("PipeWire retained devices no longer match the active source leases");
        }
        if update
            .endpoints
            .as_ref()
            .is_some_and(|next| !next.can_retain_from(endpoints))
        {
            self.pending.store(Box::into_raw(update), Ordering::Release);
            return Err("PipeWire retained virtual devices no longer match active owners");
        }
        if let Some(next) = &mut update.endpoints {
            next.retain_from(endpoints);
            std::mem::swap(endpoints, next);
        }
        if let Some(next) = &mut update.captures {
            next.retain_from(captures);
            std::mem::swap(captures, next);
        }
        if let Some(next) = &mut update.devices {
            next.retain_from(devices);
            std::mem::swap(devices, next);
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

    fn register_inputs(
        &self,
        inputs: &mixrelay_pipewire::GraphInputs,
    ) -> Result<(), mixrelay_pipewire::DriverError> {
        let mut registered = self.graph_inputs.lock().map_err(|_| {
            mixrelay_pipewire::DriverError::Input("driver registration poisoned".into())
        })?;
        let pending = self.pending.load(Ordering::Acquire);
        if !pending.is_null() {
            // SAFETY: only the initial unpublished driver calls this hook. Its RT
            // DSP consumer has not started; graph_inputs excludes control publication.
            // The unique pending envelope remains owned by the existing mailbox.
            unsafe { &mut *pending }
                .bind_inputs(inputs)
                .map_err(mixrelay_pipewire::DriverError::Input)?;
        }
        *registered = Some(inputs.clone());
        Ok(())
    }

    fn bind_application_inputs(&self, captures: &mut PipeWireCaptureSet) -> Result<(), String> {
        let registered = self
            .graph_inputs
            .lock()
            .map_err(|_| "driver registration poisoned")?;
        let inputs = registered.as_ref().ok_or("driver has not registered")?;
        inputs.ensure_live().map_err(|error| error.to_string())?;
        captures.bind_inputs(inputs)
    }

    fn bind_device_inputs(&self, devices: &mut DeviceStreams) -> Result<(), String> {
        let registered = self
            .graph_inputs
            .lock()
            .map_err(|_| "driver registration poisoned")?;
        let inputs = registered.as_ref().ok_or("driver has not registered")?;
        inputs.ensure_live().map_err(|error| error.to_string())?;
        devices.bind_inputs(inputs)
    }

    fn bind_format_inputs(&self, update: &mut LinuxResourceUpdate) -> Result<(), String> {
        let registered = self
            .graph_inputs
            .lock()
            .map_err(|_| "driver registration poisoned")?;
        let inputs = registered.as_ref().ok_or("driver has not registered")?;
        update.bind_inputs(inputs)
    }

    fn publish(&self, mut update: LinuxResourceUpdate) -> Result<(), HostAudioBackendError> {
        let registered =
            self.graph_inputs
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "driver registration poisoned".into(),
                })?;
        if let Some(inputs) = registered.as_ref() {
            update
                .bind_inputs(inputs)
                .map_err(|message| HostAudioBackendError::BackendFailure { message })?;
        }
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

    fn prepare_endpoint_edit(
        &self,
        next: &EndpointSpecs,
        previous: &EndpointSpecs,
        format: RenderFormat,
    ) -> Result<Option<EndpointEdit>, HostAudioBackendError> {
        if next == previous {
            return Ok(None);
        }
        let controls =
            self.endpoint_controls
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "PipeWire endpoint control state is poisoned".into(),
                })?;
        EndpointEdit::prepare(next, previous, &controls, format).map(Some)
    }

    fn publish_edit(
        &self,
        mut update: LinuxResourceUpdate,
        endpoints: Option<EndpointEdit>,
    ) -> Result<(), HostAudioBackendError> {
        let Some(endpoints) = endpoints else {
            return self.publish(update);
        };
        let mut controls =
            self.endpoint_controls
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "PipeWire endpoint control state is poisoned".into(),
                })?;
        *controls = endpoints.publish(|endpoints| {
            update.endpoints = Some(endpoints);
            self.publish(update)
        })?;
        Ok(())
    }
}

impl VirtualEndpointBackend for LinuxVirtualEndpointBackend {
    fn settings(&self) -> Option<mixrelay_audio_io::VirtualEndpointSettings> {
        self.driver_snapshot()
            .ok()
            .map(|snapshot| snapshot.settings)
    }

    fn sample_rate(
        &self,
        id: &mixrelay_audio_io::VirtualEndpointId,
    ) -> Result<u32, HostAudioBackendError> {
        self.driver_snapshot()?
            .settings
            .devices
            .into_iter()
            .find(|device| &device.spec.id == id)
            .and_then(|device| device.observed_sample_rate)
            .ok_or_else(|| driver_failure("Driver device rate unavailable"))
    }

    fn set_sample_rate(
        &self,
        id: &mixrelay_audio_io::VirtualEndpointId,
        rate: u32,
    ) -> Result<(), HostAudioBackendError> {
        let snapshot = self.driver_snapshot()?;
        let device = snapshot
            .settings
            .devices
            .iter()
            .find(|device| &device.spec.id == id)
            .ok_or_else(|| driver_failure("Driver device is missing"))?;
        let client = self.driver_client.as_ref().map_err(driver_failure)?;
        let mut selection = None;
        for (direction, formats) in [
            (
                mixrelay_driver_control::NativeDirection::Render,
                &device.native_formats.render,
            ),
            (
                mixrelay_driver_control::NativeDirection::Capture,
                &device.native_formats.capture,
            ),
        ] {
            let Some(current) = formats.current else {
                continue;
            };
            let desired = mixrelay_audio_io::NativeStreamFormat {
                sample_rate: rate,
                ..current
            };
            if !formats.supported.contains(&desired) {
                return Err(driver_failure(
                    "Driver does not support the requested native format",
                ));
            }
            if selection.is_none() {
                selection = Some((direction, desired));
            }
        }
        let (direction, format) =
            selection.ok_or_else(|| driver_failure("Driver format unavailable"))?;
        // The Driver's clock is shared by both native directions. One revision-checked
        // mutation updates their rate together, preserving each PCM representation.
        client
            .mutate_blocking(&mixrelay_driver_control::DriverCommand::SetFormat {
                expected_revision: snapshot.revision,
                endpoint_id: id.clone(),
                direction,
                format,
            })
            .map_err(driver_failure)?;
        Ok(())
    }

    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }

    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        self.resolve_graph_devices(desired).map(|_| ())
    }

    fn resolve_graph_devices(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<Vec<VirtualEndpointSpec>, HostAudioBackendError> {
        if desired.is_empty() {
            return Ok(Vec::new());
        }
        let snapshot = self.driver_snapshot()?;
        desired
            .iter()
            .map(|reference| {
                snapshot
                    .settings
                    .devices
                    .iter()
                    .find(|device| device.spec.id == reference.id)
                    .map(|device| device.spec.clone())
                    .ok_or_else(|| {
                        driver_failure(format!(
                            "Driver device is missing: {}",
                            reference.id.as_str()
                        ))
                    })
            })
            .collect()
    }
}

fn driver_failure(error: impl std::fmt::Display) -> HostAudioBackendError {
    HostAudioBackendError::BackendFailure {
        message: error.to_string(),
    }
}

impl LinuxVirtualEndpointBackend {
    fn driver_snapshot(
        &self,
    ) -> Result<mixrelay_driver_control::DriverSnapshot, HostAudioBackendError> {
        let snapshot = self
            .driver_client
            .as_ref()
            .map_err(driver_failure)?
            .snapshot_blocking()
            .map_err(driver_failure)?;
        if snapshot.state != mixrelay_driver_control::DriverState::Ready {
            return Err(driver_failure("Driver service is not ready"));
        }
        Ok(snapshot)
    }
}

impl GraphResourceBackend for LinuxVirtualEndpointBackend {
    fn execution_released(&self) -> bool {
        native::NativeStream::active_owners() == 0
    }

    fn resolve_application_capture(
        &self,
        request: &ApplicationCaptureRequest,
    ) -> Result<mixrelay_engine_service::graph_composition::SharedSource, HostAudioBackendError>
    {
        application_identity::resolve(request)
    }

    fn device_inventory(
        &self,
        _selectors: &[&mixrelay_audio_io::DeviceSelector],
    ) -> Result<Vec<mixrelay_audio_io::AudioDeviceDescriptor>, HostAudioBackendError> {
        mixrelay_audio_io::linux::devices()
    }

    fn prepare_format_change(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
        playback: mixrelay_engine_service::PlaybackBindings<'_>,
        _current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<Box<dyn mixrelay_engine_service::PreparedGraphFormat>, HostAudioBackendError> {
        let playback = source_mute::OwnedPlan::prepare(&playback)
            .map_err(|capability| HostAudioBackendError::CapabilityUnavailable { capability })?;
        format::prepare(self, document, requested, playback)
    }
    fn reconcile(
        &self,
        document: &mixrelay_audio_graph::GraphDocument,
        playback: mixrelay_engine_service::PlaybackBindings<'_>,
    ) -> Result<(), HostAudioBackendError> {
        let playback = source_mute::OwnedPlan::prepare(&playback)
            .map_err(|capability| HostAudioBackendError::CapabilityUnavailable { capability })?;
        let next = LinuxResourceSpec::from_document(document)?;
        let format = format::unpack(self.active_format.load(Ordering::Acquire));
        let mut active =
            self.active_spec
                .lock()
                .map_err(|_| HostAudioBackendError::BackendFailure {
                    message: "PipeWire endpoint state is poisoned".to_owned(),
                })?;
        if *active == next {
            self.playback.stage(document.revision, playback);
            return Ok(());
        }
        let endpoints = self.prepare_endpoint_edit(&next.endpoints, &active.endpoints, format)?;
        let (captures, mut application_watch) = if active.captures == next.captures {
            (None, None)
        } else {
            let (captures, watch) = self.prepare_capture_edit(&next.captures, format)?;
            (Some(captures), Some(watch))
        };
        let devices = if active.devices == next.devices {
            None
        } else {
            Some(self.prepare_device_edit(&next.devices, format)?)
        };
        let watch = devices.as_ref().map(|streams| {
            streams.initialize_controls(document);
            streams.watch(next.devices.clone())
        });
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
        if let (Some(prepared), Some(current)) =
            (application_watch.as_mut(), registered_applications.as_ref())
        {
            prepared.refresh_leases(current);
        }
        self.publish_edit(
            LinuxResourceUpdate {
                revision: document.revision,
                endpoints: None,
                captures,
                devices,
            },
            endpoints,
        )?;
        if let Some(watch) = watch {
            *registered = Some(watch);
        }
        if let Some(watch) = application_watch {
            *registered_applications = Some(watch);
        }
        *active = next;
        self.playback.stage(document.revision, playback);
        Ok(())
    }
}

impl Drop for LinuxVirtualEndpointBackend {
    fn drop(&mut self) {
        self.playback.clear_all();
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
    endpoints: EndpointSpecs,
    captures: Vec<(String, ApplicationCaptureRequest)>,
    devices: DeviceRequirements,
}

impl LinuxResourceSpec {
    fn from_document(
        document: &mixrelay_audio_graph::GraphDocument,
    ) -> Result<Self, HostAudioBackendError> {
        let requirements = ResourceRequirements::from_document(document);
        Ok(Self {
            endpoints: endpoints::specs(requirements.endpoints)?,
            captures: requirements.captures,
            devices: requirements.devices,
        })
    }
}

struct LinuxResourceUpdate {
    revision: u64,
    endpoints: Option<Box<EndpointSet>>,
    captures: Option<Box<PipeWireCaptureSet>>,
    devices: Option<Box<DeviceStreams>>,
}

impl LinuxResourceUpdate {
    fn bind_inputs(&mut self, inputs: &mixrelay_pipewire::GraphInputs) -> Result<(), String> {
        inputs.ensure_live().map_err(|error| error.to_string())?;
        if let Some(endpoints) = &mut self.endpoints {
            endpoints.bind_inputs(inputs)?;
        }
        if let Some(captures) = &mut self.captures {
            captures.bind_inputs(inputs)?;
        }
        if let Some(devices) = &mut self.devices {
            devices.bind_inputs(inputs)?;
        }
        Ok(())
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
