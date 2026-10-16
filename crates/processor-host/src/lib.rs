use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use base64::engine::general_purpose::STANDARD as BASE64;
use mixrelay_audio_engine::{
    HostedAudioProcessor, HostedLatencySource, HostedProcessorFactory,
    HostedProcessorPrepareRequest,
};
use mixrelay_audio_graph::{
    BuiltInNodeResolver, CompileContext, CompiledPort, NodeConfig, NodeId, NodeResolutionError,
    NodeResolver, NodeSpec, PortDirection, ProviderNodeConfig, RenderFormat, ResolvedNode,
};
use mixrelay_control_protocol::{
    ExternalProcessorCatalog, ExternalProcessorDescriptor, PROTOCOL_VERSION,
    ProcessorEditorFailure, ProcessorEditorState, ProcessorEditorStatus,
};

mod discovery;
mod dry_delay;
mod dsp_lifecycle;
mod editor_observation;
mod editor_surface;
mod latency;
mod native_editor;
mod rack_formats;
mod reuse;
mod shared_transport;
mod state;
mod timing;
use shared_transport::{ControlMailbox, ParentAudio, SharedHeader, WorkerAudio, WorkerControl};
mod worker_paths;
use worker_paths::worker_paths;

pub use editor_observation::ProcessorEditorCloseObservation;
pub use latency::ProcessorLatencySnapshot;
pub use reuse::ProcessorReuseTransaction;

pub use rack_formats::{
    PROBE_ARGUMENT as PLUGIN_PROBE_ARGUMENT, RackFormat, RackLocator,
    SCAN_ARGUMENT as PLUGIN_SCAN_ARGUMENT, WORKER_ARGUMENT as PLUGIN_WORKER_ARGUMENT,
    run_probe as run_plugin_probe, run_scan as run_plugin_scan,
};

// Establish agent status before a vendor loads AppKit or creates an editor. Setting it
// only when our editor host starts is too late for discovery and instance creation.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "one process-preparation contract; AppKit setup can fail on macOS"
    )
)]
fn prepare_plugin_process() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        let main =
            MainThreadMarker::new().ok_or("plug-in process must start on the main thread")?;
        let app = NSApplication::sharedApplication(main);
        if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
            return Err("could not initialize background plug-in process".into());
        }
        app.finishLaunching();
    }
    Ok(())
}

#[cfg(not(target_has_atomic = "64"))]
compile_error!("MixRelay processor shared transport requires native 64-bit atomics");

const WORKER_START_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum wait for one processor state response, not an entire graph's providers.
/// Each independently hosted processor receives its own bounded capture opportunity.
pub const PROCESSOR_STATE_TIMEOUT: Duration = Duration::from_secs(5);
const SHARED_MAGIC: u64 = 0x4d52_5653_5433_3031;
const SHARED_VERSION: u32 = 8;
const PARAMETER_CAPACITY: usize = 4096;
const STATE_CAPACITY: usize = mixrelay_processor_state::MAX_STATE_BYTES;
const STATE_OK: u32 = 0;
const STATE_FAILED: u32 = 1;
const STATE_TOO_LARGE: u32 = 2;
const STATE_UNSUPPORTED: u32 = 3;
const WORKER_STARTING: u32 = 0;
const WORKER_READY: u32 = 1;
const WORKER_FAILED: u32 = 2;
const EDITOR_COMMAND_NONE: u32 = 0;
const EDITOR_COMMAND_OPEN: u32 = 1;
const EDITOR_COMMAND_CLOSE: u32 = 2;
const EDITOR_STATE_UNAVAILABLE: u32 = 0;
const EDITOR_STATE_CLOSED: u32 = 1;
const EDITOR_STATE_OPENING: u32 = 2;
const EDITOR_STATE_OPEN: u32 = 3;
const EDITOR_STATE_FAILED: u32 = 4;
const EDITOR_FAILURE_NONE: u32 = 0;
const EDITOR_FAILURE_NO_EDITOR: u32 = 1;
const EDITOR_FAILURE_DISPLAY_UNAVAILABLE: u32 = 2;
const EDITOR_FAILURE_HOST_WINDOW: u32 = 3;
const EDITOR_FAILURE_PLUGIN_REJECTED: u32 = 4;
const EDITOR_FAILURE_WORKER_UNAVAILABLE: u32 = 5;

#[derive(Debug, Clone)]
struct CatalogEntry {
    descriptor: ExternalProcessorDescriptor,
    rack: RackCatalogTarget,
}

#[derive(Debug, Clone)]
struct RackCatalogTarget {
    locator: RackLocator,
}

fn selected_ports(
    descriptor: &ExternalProcessorDescriptor,
    config: &ProviderNodeConfig,
) -> Result<mixrelay_audio_graph::ProviderPorts, String> {
    let selected_id = config
        .ports
        .as_ref()
        .map_or(descriptor.default_layout_id.as_str(), |ports| {
            ports.layout_id.as_str()
        });
    let actual = descriptor
        .layouts
        .iter()
        .find(|ports| ports.layout_id == selected_id)
        .ok_or("selected processor layout is not installed")?;
    actual.validate().map_err(|error| error.to_string())?;
    if let Some(authored) = &config.ports {
        authored.validate().map_err(|error| error.to_string())?;
        if !rack_formats::layouts::same_topology(actual, authored) {
            return Err("saved processor topology differs from the native catalog".into());
        }
    }
    Ok(actual.clone())
}

#[derive(Debug, Clone, Default)]
pub struct ProcessorCatalog {
    snapshot: Arc<RwLock<discovery::CatalogSnapshot>>,
    /// Discovery does not block catalog readers or the render thread; only rescans serialize.
    scan_lock: Arc<Mutex<()>>,
}

impl ProcessorCatalog {
    /// Returns the installed port contract without creating a worker.
    #[must_use]
    pub fn ports(&self, type_id: &str) -> Option<mixrelay_audio_graph::ProviderPorts> {
        let entry = self.entry(type_id)?;
        entry
            .descriptor
            .layouts
            .iter()
            .find(|ports| ports.layout_id == entry.descriptor.default_layout_id)
            .cloned()
    }
    /// Safely discovers native and cross-platform effects. Each candidate is probed in a
    /// disposable copy of the engine executable so a broken plug-in cannot terminate the graph
    /// service.
    #[must_use]
    pub fn discover() -> Self {
        let catalog = Self::default();
        catalog.rescan(&[]);
        catalog
    }

    /// Refreshes the discovery snapshot using standard format locations plus explicit user paths.
    /// Scanner and probe processes isolate malformed third-party bundles from the engine service.
    /// Failed formats/candidates retain their previous entries until a successful retry.
    pub fn rescan(&self, custom_paths: &[PathBuf]) {
        discovery::rescan(self, custom_paths);
    }

    #[must_use]
    pub fn protocol_catalog(&self) -> ExternalProcessorCatalog {
        let snapshot = self
            .snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ExternalProcessorCatalog {
            protocol_version: PROTOCOL_VERSION,
            processors: snapshot
                .entries
                .values()
                .map(|entry| entry.descriptor.clone())
                .collect(),
            diagnostics: snapshot.diagnostics.clone(),
        }
    }

    fn entry(&self, type_id: &str) -> Option<CatalogEntry> {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .get(type_id)
            .cloned()
    }

    #[must_use]
    pub fn parameter_is_writable(&self, type_id: &str, parameter_id: u32) -> bool {
        self.entry(type_id).is_some_and(|entry| {
            entry
                .descriptor
                .parameters
                .iter()
                .any(|parameter| parameter.id == parameter_id && !parameter.is_read_only)
        })
    }
}

/// Runs one `AU`, `AUv3`, `VST3`, `CLAP`, or `LV2` instance in the common isolated DSP worker.
///
/// # Errors
///
/// Returns a diagnostic when the target identity, shared transport, or plug-in cannot initialize.
pub fn run_plugin_worker(arguments: &[String]) -> Result<(), String> {
    prepare_plugin_process()?;
    let [encoded_locator, shared_path, config_path] = arguments else {
        return Err(
            "processor worker requires locator, shared-memory, and config paths".to_owned(),
        );
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let _stdin_guard =
        dsp_lifecycle::watch_parent(Arc::clone(&shutdown)).map_err(|error| error.to_string())?;
    let locator = RackLocator::decode(encoded_locator)?;
    let (mut audio_shared, editor_shared) = shared_transport::worker(Path::new(shared_path))?;
    let editor_shared = Arc::new(editor_shared);
    let layout = audio_shared.layout();
    let prepared = state::read_worker_configuration(config_path)?;
    let plugin = rack_formats::RackWorkerPlugin::load(
        &locator,
        &prepared.ports,
        prepared.sample_rate,
        layout.block_frames,
        &prepared.initial,
    )?;
    if plugin.input_channels() != layout.inputs || plugin.output_channels() != layout.outputs {
        return Err("loaded processor layout does not match the catalog".to_owned());
    }
    let prepared_rate = prepared.sample_rate;
    let has_editor = plugin.has_editor();
    latency::publish_intrinsic_frames(editor_shared.header(), plugin.intrinsic_latency_frames());
    let plugin = Arc::new(Mutex::new(plugin));
    let dsp_plugin = Arc::clone(&plugin);
    let dsp_shutdown = Arc::clone(&shutdown);
    let dsp_thread = thread::Builder::new()
        .name("mixrelay-plugin-dsp".to_owned())
        .spawn(move || {
            rack_worker_loop(
                &mut audio_shared,
                &dsp_plugin,
                layout,
                &dsp_shutdown,
                prepared_rate,
            );
        })
        .map_err(|error| error.to_string())?;
    // Retain the creator's instance owner until DSP has joined, including broker unwinding.
    let dsp_owner =
        dsp_lifecycle::DspThreadOwner::new(dsp_thread, Arc::clone(&shutdown), Arc::clone(&plugin));
    let editor_header = editor_shared.header();
    editor_header.editor_state.store(
        if has_editor {
            EDITOR_STATE_CLOSED
        } else {
            EDITOR_STATE_UNAVAILABLE
        },
        Ordering::Release,
    );
    if !has_editor {
        editor_header
            .editor_failure
            .store(EDITOR_FAILURE_NO_EDITOR, Ordering::Release);
    }
    editor_header
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let result = editor_broker_loop(&editor_shared, &plugin, locator.display_name(), &shutdown);
    dsp_owner.finish(result)
}

/// One processing block's duration at the prepared rate (48 kHz if it is unusable).
fn block_period(block_frames: usize, sample_rate: f64) -> Duration {
    let rate = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        48_000.0
    };
    Duration::from_secs_f64(f64::from(u32::try_from(block_frames).unwrap_or(u32::MAX)) / rate)
}

/// Sleeps until the parent advances the word blocking the worker, at most two blocks (a
/// missed wake costs at most that). Polls where the shared wake API is unavailable, after
/// a failed wait, and for good after repeated failures.
#[derive(Default)]
struct Waiter {
    #[cfg(target_os = "macos")]
    failures: u32,
}

impl Waiter {
    const POLL: Duration = Duration::from_micros(100);
    #[cfg(target_os = "macos")]
    const MAXIMUM_FAILURES: u32 = 8;

    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            clippy::unused_self,
            reason = "macOS retains failed-futex state; other platforms use stateless polling"
        )
    )]
    fn wait(&mut self, word: &std::sync::atomic::AtomicU64, observed: u64, block: Duration) {
        #[cfg(target_os = "macos")]
        if self.failures < Self::MAXIMUM_FAILURES
            && let Some(futex) = mixrelay_realtime::shared_futex()
        {
            if let mixrelay_realtime::FutexWake::Failed(_) = futex.wait(word, observed, block * 2) {
                self.failures += 1;
            } else {
                self.failures = 0;
                return;
            }
        }
        let _ = (word, observed, block);
        thread::sleep(Self::POLL);
    }
}

fn rack_worker_loop(
    shared: &mut WorkerAudio,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    layout: AudioLayout,
    parent_closed: &AtomicBool,
    sample_rate: f64,
) {
    let block = block_period(layout.block_frames, sample_rate);
    // One block of plug-in processing per release: finish within 80% of it.
    #[cfg(target_os = "macos")]
    let _realtime = mixrelay_realtime::RealtimeThread::promote(block, block / 2, block * 4 / 5)
        .inspect_err(|error| tracing::warn!(%error, "plug-in DSP runs at standard priority"))
        .ok();
    let mut waiter = Waiter::default();
    loop {
        let header = shared.header();
        if parent_closed.load(Ordering::Acquire) || header.shutdown.load(Ordering::Acquire) != 0 {
            return;
        }
        // Choose what to wait for before the final readiness check: whichever parent
        // transition then happens either makes input pending or changes the chosen word.
        let (word, value) = shared.blocking_word(shared.wake_words());
        if !shared.input_pending() {
            waiter.wait(word, value, block);
            continue;
        }
        let mut plugin = match plugin.try_lock() {
            Ok(plugin) => plugin,
            Err(std::sync::TryLockError::WouldBlock) => {
                // The editor holds the instance briefly; sleep rather than spin at
                // realtime priority against a lower-priority holder.
                thread::sleep(Duration::from_micros(50));
                continue;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                mark_plugin_worker_failed(shared.header());
                return;
            }
        };
        // A main-thread restart may fail while DSP waits for exclusive instance access.
        // Recheck under that same lock before touching a now-deactivated native instance.
        if parent_closed.load(Ordering::Acquire)
            || shared.header().shutdown.load(Ordering::Acquire) != 0
            || shared.header().worker_status.load(Ordering::Acquire) == WORKER_FAILED
        {
            return;
        }
        let Some(frames) =
            shared.receive(|source| copy_shared_input_to_rack(source, &mut plugin, layout))
        else {
            continue;
        };
        plugin.clear_outputs();
        if plugin.process(frames).is_err() {
            shared
                .header()
                .process_failures
                .fetch_add(1, Ordering::Relaxed);
            bypass_rack(&mut plugin, frames);
        }
        latency::publish_intrinsic_frames(shared.header(), plugin.intrinsic_latency_frames());
        shared.publish(|destination| copy_rack_output_to_shared(destination, &plugin, layout));
    }
}

fn editor_broker_loop(
    shared: &Arc<WorkerControl>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    parent_closed: &AtomicBool,
) -> Result<(), String> {
    let mut host: Option<native_editor::NativeEditorHost> = None;
    let mut last_idle = Instant::now();
    let mut result = Ok(());
    loop {
        let header = shared.header();
        if parent_closed.load(Ordering::Acquire) || header.shutdown.load(Ordering::Acquire) != 0 {
            if let (Some(host), Ok(mut plugin)) = (host.as_mut(), plugin.lock()) {
                // Service failure stops DSP, but the initialized native object still owns its
                // GUI. Close it on its main thread before removing the parent window or instance.
                host.close(&mut plugin);
            }
            return result;
        }
        if let Err(error) = service_plugin_control(shared, plugin) {
            result = Err(error);
            continue;
        }
        apply_pending_editor_command(shared, plugin, title, &mut host);
        // Native state APIs may allocate, block, or require the UI thread. They run only on
        // this broker, never in the engine callback or the worker's DSP processing loop.
        apply_pending_state_request(shared, plugin);
        if last_idle.elapsed() >= Duration::from_millis(16) {
            tick_editor(shared, plugin, &mut host);
            last_idle = Instant::now();
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn service_plugin_control(
    shared: &WorkerControl,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
) -> Result<(), String> {
    let header = shared.header();
    let mut plugin = match plugin.try_lock() {
        Ok(plugin) => plugin,
        Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
        Err(std::sync::TryLockError::Poisoned(_)) => {
            mark_plugin_worker_failed(header);
            header.shutdown.store(1, Ordering::Release);
            return Err("processor instance lock was poisoned".to_owned());
        }
    };
    if let Err(error) = plugin.service_main_thread() {
        // Publish failure before releasing the lock so no DSP call can race a failed restart.
        mark_plugin_worker_failed(header);
        header.shutdown.store(1, Ordering::Release);
        return Err(format!("processor main-thread service failed: {error}"));
    }
    // Native parameter controllers belong to the instance's creator thread. DSP only
    // consumes the prepared format event queues on its next real audio block. Servicing
    // the mailbox here also applies edits while no PCM or editor window is present.
    apply_pending_rack_parameter(header, &mut plugin);
    Ok(())
}

fn apply_pending_state_request(
    shared: &WorkerControl,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
) {
    let header = shared.header();
    let Some((request, kind)) = shared.pending_request() else {
        return;
    };
    if shared.request_cancelled(request) {
        // A deferred native edit must not leave a timed-out reader's request unacknowledged.
        shared.publish(request, Err(ProcessorStateError::Unavailable));
        return;
    }
    if kind == editor_surface::EDITOR_FRAME {
        if !header.editor_observed_open.load(Ordering::Acquire) {
            shared.publish(request, Err(ProcessorStateError::Unavailable));
        }
        return;
    }
    let mut plugin = match plugin.try_lock() {
        Ok(plugin) => plugin,
        Err(std::sync::TryLockError::WouldBlock) => return,
        Err(std::sync::TryLockError::Poisoned(_)) => {
            mark_plugin_worker_failed(header);
            return;
        }
    };
    // Deliver accepted controls to the adapter before capture. Some formats queue native
    // events until the next real audio block; release the lock and retry after DSP in that
    // case, within the reader's existing timeout, without manufacturing an OFF-node block.
    apply_pending_rack_parameter(shared.header(), &mut plugin);
    let state = plugin.save_state();
    if matches!(state, Err(ProcessorStateError::Pending)) {
        return;
    }
    latency::publish_intrinsic_frames(header, plugin.intrinsic_latency_frames());
    // Native capture needs the instance lock; transporting the completed bytes does not.
    // Release it before waiting for the control reader so DSP can resume immediately.
    drop(plugin);
    shared.publish(request, state);
}

fn apply_pending_editor_command(
    shared: &Arc<WorkerControl>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let header = shared.header();
    let sequence = header.editor_sequence.load(Ordering::Acquire);
    if sequence == header.editor_consumed.load(Ordering::Relaxed) {
        return;
    }
    let command = header.editor_command.load(Ordering::Relaxed);
    if command == EDITOR_COMMAND_CLOSE {
        let snapshot = if let Some(host) = host.as_mut() {
            let Ok(mut plugin) = plugin.lock() else {
                mark_plugin_worker_failed(header);
                publish_editor_failure(header, EDITOR_FAILURE_WORKER_UNAVAILABLE);
                return;
            };
            host.close(&mut plugin);
            let snapshot = plugin.tick_editor();
            latency::publish_intrinsic_frames(header, plugin.intrinsic_latency_frames());
            host.refresh(snapshot);
            snapshot
        } else {
            rack_formats::EditorSnapshot::default()
        };
        header
            .editor_failure
            .store(EDITOR_FAILURE_NONE, Ordering::Release);
        publish_observed_editor_snapshot(header, snapshot);
    } else if command == EDITOR_COMMAND_OPEN {
        open_editor(shared, plugin, title, host);
    }
    header.editor_consumed.store(sequence, Ordering::Release);
}

fn open_editor(
    shared: &Arc<WorkerControl>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let header = shared.header();
    let has_editor = if let Ok(plugin) = plugin.lock() {
        plugin.has_editor()
    } else {
        mark_plugin_worker_failed(header);
        publish_editor_failure(header, EDITOR_FAILURE_WORKER_UNAVAILABLE);
        return;
    };
    if !has_editor {
        publish_editor_failure(header, EDITOR_FAILURE_NO_EDITOR);
        return;
    }
    if host.is_none() {
        match native_editor::NativeEditorHost::new(Arc::clone(shared)) {
            Ok(created) => *host = Some(created),
            Err(native_editor::EditorHostFailure::DisplayUnavailable) => {
                publish_editor_failure(header, EDITOR_FAILURE_DISPLAY_UNAVAILABLE);
                return;
            }
            Err(_) => {
                publish_editor_failure(header, EDITOR_FAILURE_HOST_WINDOW);
                return;
            }
        }
    }
    let result = if let Ok(mut plugin) = plugin.lock() {
        let result = host
            .as_mut()
            .expect("editor host was initialized")
            .open(&mut plugin, title);
        latency::publish_intrinsic_frames(header, plugin.intrinsic_latency_frames());
        result
    } else {
        mark_plugin_worker_failed(header);
        Err(native_editor::EditorHostFailure::PluginRejected)
    };
    match result {
        Ok(snapshot) if snapshot.open => {
            header
                .editor_failure
                .store(EDITOR_FAILURE_NONE, Ordering::Relaxed);
            publish_observed_editor_snapshot(header, snapshot);
        }
        Ok(_) | Err(native_editor::EditorHostFailure::PluginRejected) => {
            publish_editor_failure(header, EDITOR_FAILURE_PLUGIN_REJECTED);
        }
        Err(native_editor::EditorHostFailure::DisplayUnavailable) => {
            publish_editor_failure(header, EDITOR_FAILURE_DISPLAY_UNAVAILABLE);
        }
        Err(native_editor::EditorHostFailure::HostWindow) => {
            publish_editor_failure(header, EDITOR_FAILURE_HOST_WINDOW);
        }
    }
}

fn tick_editor(
    shared: &WorkerControl,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let Some(editor_host) = host.as_mut() else {
        return;
    };
    let header = shared.header();
    // The host retains close/resize events until the plug-in can consume them. Native
    // mouse tracking can run a nested event loop; pumping it under this mutex stalls DSP.
    editor_host.poll_events();
    let mut plugin = match plugin.try_lock() {
        Ok(plugin) => plugin,
        Err(std::sync::TryLockError::WouldBlock) => return,
        Err(std::sync::TryLockError::Poisoned(_)) => {
            mark_plugin_worker_failed(header);
            return;
        }
    };
    let events = editor_host.take_events();
    if events.close_requested {
        editor_host.close(&mut plugin);
    } else if let Some((width, height)) = events.resized {
        let _ = plugin.resize_editor(width, height);
    }
    let snapshot = plugin.tick_editor();
    latency::publish_intrinsic_frames(header, plugin.intrinsic_latency_frames());
    editor_host.refresh(snapshot);
    publish_observed_editor_snapshot(header, snapshot);
}

fn publish_observed_editor_snapshot(header: &SharedHeader, snapshot: rack_formats::EditorSnapshot) {
    publish_editor_snapshot(header, snapshot);
    header.editor_state.store(
        if snapshot.open {
            EDITOR_STATE_OPEN
        } else if header.editor_failure.load(Ordering::Acquire) != EDITOR_FAILURE_NONE {
            // Idle native ticks are not retries and cannot clear a failed-open result.
            EDITOR_STATE_FAILED
        } else {
            EDITOR_STATE_CLOSED
        },
        Ordering::Release,
    );
    let was_open = header
        .editor_observed_open
        .swap(snapshot.open, Ordering::AcqRel);
    if was_open && !snapshot.open {
        // Only the native broker can acknowledge a close. Requests, crashes, initial closed
        // state, and repeated idle ticks cannot manufacture a durable-state checkpoint.
        header
            .editor_close_generation
            .fetch_add(1, Ordering::Release);
    }
}

fn publish_editor_snapshot(header: &SharedHeader, snapshot: rack_formats::EditorSnapshot) {
    let (width, height) = snapshot.size.unwrap_or((0, 0));
    header.editor_width.store(width, Ordering::Relaxed);
    header.editor_height.store(height, Ordering::Relaxed);
    header
        .editor_resizable
        .store(u32::from(snapshot.resizable), Ordering::Relaxed);
}

fn publish_editor_failure(header: &SharedHeader, failure: u32) {
    publish_editor_snapshot(header, rack_formats::EditorSnapshot::default());
    header.editor_failure.store(failure, Ordering::Relaxed);
    header
        .editor_state
        .store(EDITOR_STATE_FAILED, Ordering::Release);
}

fn apply_pending_rack_parameter(
    header: &SharedHeader,
    plugin: &mut rack_formats::RackWorkerPlugin,
) {
    consume_parameters(header, |id, value| {
        if plugin.set_parameter(id, value).is_err() {
            header.process_failures.fetch_add(1, Ordering::Relaxed);
        }
    });
    latency::publish_intrinsic_frames(header, plugin.intrinsic_latency_frames());
}

fn mark_plugin_worker_failed(header: &SharedHeader) {
    latency::publish_intrinsic_frames(header, None);
    header.worker_status.store(WORKER_FAILED, Ordering::Release);
}

fn consume_parameters(header: &SharedHeader, mut apply: impl FnMut(u32, f64)) {
    if !header.parameters_pending.swap(false, Ordering::AcqRel) {
        return;
    }
    let count = (header.parameter_count.load(Ordering::Acquire) as usize).min(PARAMETER_CAPACITY);
    for slot in &header.parameters[..count] {
        if slot.pending.swap(false, Ordering::AcqRel) {
            apply(
                slot.id.load(Ordering::Relaxed),
                f64::from_bits(slot.value_bits.load(Ordering::Acquire)),
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AudioLayout {
    inputs: usize,
    outputs: usize,
    block_frames: usize,
}

#[derive(Debug, Clone)]
pub struct CatalogNodeResolver {
    catalog: ProcessorCatalog,
}

impl CatalogNodeResolver {
    #[must_use]
    pub const fn new(catalog: ProcessorCatalog) -> Self {
        Self { catalog }
    }
}

impl NodeResolver for CatalogNodeResolver {
    fn resolve(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        let NodeConfig::Provider(config) = &node.config else {
            return BuiltInNodeResolver.resolve(node, context);
        };
        let ports =
            match self.catalog.entry(config.type_id.as_str()) {
                Some(entry) => selected_ports(&entry.descriptor, config)
                    .map_err(|reason| NodeResolutionError::InvalidConfiguration { reason })?,
                None => config.ports.clone().ok_or_else(|| {
                    NodeResolutionError::CapabilityUnavailable {
                        reason: format!("processor is not installed: {}", config.type_id),
                    }
                })?,
            };
        ports
            .validate()
            .map_err(|error| NodeResolutionError::InvalidConfiguration {
                reason: error.to_string(),
            })?;
        if ports
            .inputs
            .iter()
            .chain(&ports.outputs)
            .any(|bus| bus.channels > context.limits.max_channels_per_port)
        {
            return Err(NodeResolutionError::InvalidConfiguration {
                reason: "processor bus width exceeds graph limits".to_owned(),
            });
        }
        Ok(ResolvedNode {
            inputs: ports
                .inputs
                .iter()
                .map(|bus| CompiledPort::new(bus.id.clone(), PortDirection::Input, bus.channels))
                .collect(),
            outputs: ports
                .outputs
                .iter()
                .map(|bus| CompiledPort::new(bus.id.clone(), PortDirection::Output, bus.channels))
                .collect(),
        })
    }
}

#[derive(Clone)]
pub struct ExternalProcessorFactory {
    catalog: ProcessorCatalog,
    controls: ProcessorControlRegistry,
}

struct UnavailableProcessor;

impl HostedAudioProcessor for UnavailableProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32], _frames: usize) -> bool {
        output.fill(0.0);
        true
    }
}

impl ExternalProcessorFactory {
    #[must_use]
    pub fn new(catalog: ProcessorCatalog) -> Self {
        Self {
            catalog,
            controls: ProcessorControlRegistry::default(),
        }
    }

    #[must_use]
    pub const fn with_controls(
        catalog: ProcessorCatalog,
        controls: ProcessorControlRegistry,
    ) -> Self {
        Self { catalog, controls }
    }
}

impl HostedProcessorFactory for ExternalProcessorFactory {
    fn prepare_inactive(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Option<Box<dyn HostedAudioProcessor>>, String> {
        self.controls.verify_state(&request.config.parameters)?;
        let mut config = request.config.clone();
        if let Some(entry) = self.catalog.entry(config.type_id.as_str()) {
            config.ports = Some(selected_ports(&entry.descriptor, &config)?);
        }
        let normalized = HostedProcessorPrepareRequest {
            config: &config,
            ..request
        };
        Ok(self.controls.reuse_request(&normalized))
    }

    fn prepare(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String> {
        self.controls.verify_state(&request.config.parameters)?;
        let Some(entry) = self.catalog.entry(request.config.type_id.as_str()) else {
            if !request.config.ports.as_ref().is_some_and(|ports| {
                ports.total_input_channels() == Ok(request.input_channels)
                    && ports.total_output_channels() == Ok(request.output_channels)
            }) {
                return Err("unavailable processor needs its saved port contract".into());
            }
            return Ok(Box::new(UnavailableProcessor));
        };
        let selected = selected_ports(&entry.descriptor, request.config)?;
        if selected
            .total_input_channels()
            .map_err(|error| error.to_string())?
            != request.input_channels
            || selected
                .total_output_channels()
                .map_err(|error| error.to_string())?
                != request.output_channels
        {
            return Err("compiled plug-in layout does not match the catalog".to_owned());
        }
        // Reuse is tied to the resolved topology even when the authored node asks for
        // the current default. A catalog change must never inherit a same-width worker
        // whose sidechain ordering or native configuration identity has changed.
        let mut config = request.config.clone();
        config.ports = Some(selected);
        let request = HostedProcessorPrepareRequest {
            config: &config,
            ..request
        };
        if let Some(processor) = self.controls.reuse_request(&request) {
            return Ok(processor);
        }
        let mut processor = HostedExternalProcessor::new(
            &entry,
            request.config,
            request.format,
            self.controls.optional_state_store().as_ref(),
        )?;
        let control = processor.control();
        *control
            .reuse
            .lock()
            .map_err(|_| "processor reuse metadata is poisoned")? =
            Some(reuse::ReuseSignature::new(&request));
        processor.registration = Some(self.controls.register(request.node_id, &control));
        Ok(Box::new(processor) as Box<dyn HostedAudioProcessor>)
    }
}

/// Weak registry used by the control plane to update active plug-ins without rebuilding the
/// graph. Weak entries ensure a removed node cannot be kept alive by UI bookkeeping.
type WeakProcessorControls = BTreeMap<NodeId, Vec<Weak<HostedProcessorControl>>>;
type SharedProcessorControls = RwLock<WeakProcessorControls>;

#[derive(Clone, Default)]
pub struct ProcessorControlRegistry {
    controls: Arc<SharedProcessorControls>,
    state_store: Arc<RwLock<Option<mixrelay_processor_state::StateStore>>>,
}

/// Stable control-plane failures; unsupported formats must not be saved as empty success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorStateError {
    Unavailable,
    Busy,
    Pending,
    Timeout,
    Unsupported,
    NativeFailure,
    TooLarge,
    InvalidResponse,
    Storage,
}

impl std::fmt::Display for ProcessorStateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "processor worker is unavailable",
            Self::Busy => "processor control is busy",
            Self::Pending => "processor state request is still pending",
            Self::Timeout => "processor state export timed out",
            Self::Unsupported => "processor native state export is unsupported",
            Self::NativeFailure => "processor native state export failed",
            Self::TooLarge => "processor state exceeds 64 MiB",
            Self::InvalidResponse => "processor state response is invalid",
            Self::Storage => "processor state could not be stored durably",
        })
    }
}

impl std::error::Error for ProcessorStateError {}

impl ProcessorControlRegistry {
    /// Captures native state on the worker's editor/control thread. Call only from a control
    /// thread, before saving or replacing a graph. Native state is authoritative: replace the
    /// provider's parameter object with this snapshot, rather than overlaying stale values.
    /// Hold a [`Self::reuse_transaction`] through capture and the owning publication. The
    /// captured reference temporarily authorizes reuse; failed publication must roll it back.
    ///
    /// # Errors
    /// Returns an error for inactive/crashed workers, concurrent or timed-out requests,
    /// unsupported native state APIs, storage failure, or exports exceeding 64 MiB.
    pub fn snapshot_state(
        &self,
        node_id: &NodeId,
        timeout: Duration,
    ) -> Result<serde_json::Value, ProcessorStateError> {
        let control = self
            .live_control(node_id)
            .map_err(|_| ProcessorStateError::Unavailable)?;
        self.snapshot_control_state(&control, timeout)
    }

    fn register(
        &self,
        node_id: &NodeId,
        control: &Arc<HostedProcessorControl>,
    ) -> ProcessorControlRegistration {
        let mut controls = self
            .controls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = controls.entry(node_id.clone()).or_default();
        entries.retain(|entry| entry.strong_count() != 0);
        let registered = Arc::downgrade(control);
        entries.push(registered.clone());
        ProcessorControlRegistration {
            controls: Arc::downgrade(&self.controls),
            node_id: node_id.clone(),
            registered,
        }
    }

    /// Coalesces a normalized parameter value into the worker's lock-free mailbox.
    ///
    /// # Errors
    ///
    /// Returns a stable diagnostic when the value is invalid or the node is no longer active.
    pub fn set_parameter(&self, node_id: &NodeId, id: u32, value: f64) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err("processor parameters must be normalized between 0 and 1".to_owned());
        }
        let mut controls = self
            .controls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = controls
            .get_mut(node_id)
            .ok_or_else(|| format!("processor node is not active: {node_id}"))?;
        entries.retain(|entry| entry.strong_count() != 0);
        let control = entries
            .last()
            .and_then(Weak::upgrade)
            .ok_or_else(|| format!("processor node is not active: {node_id}"))?;
        drop(controls);
        control.set_parameter(id, value)
    }

    /// Returns the live native-editor state for the active instance behind `node_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the graph no longer owns a worker for the requested node.
    pub fn editor_status(&self, node_id: &NodeId) -> Result<ProcessorEditorStatus, String> {
        let control = self.live_control(node_id)?;
        Ok(control.editor_status(node_id))
    }

    /// Latest sampled input-publication to output-receipt time, including worker scheduling.
    /// Returns no value before measurement, after expiry, or for an inactive/failed instance.
    #[must_use]
    pub fn processing_time_ns(&self, node_id: &NodeId) -> Option<u64> {
        let control = self.live_control(node_id).ok()?;
        if !control.active_owner.load(Ordering::Acquire)
            || control
                .shared
                .header()
                .worker_status
                .load(Ordering::Acquire)
                != WORKER_READY
        {
            return None;
        }
        control.timing.latest()
    }

    /// Publishes an idempotent open/close request to the plug-in worker and returns immediately.
    /// The UI polls [`Self::editor_status`] so a slow or modal third-party editor can never block
    /// the engine's HTTP control thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the graph no longer owns a worker for the requested node.
    pub fn set_editor_open(
        &self,
        node_id: &NodeId,
        open: bool,
    ) -> Result<ProcessorEditorStatus, String> {
        let control = self.live_control(node_id)?;
        control.set_editor_open(open);
        Ok(control.editor_status(node_id))
    }

    /// Reports whether a node still owns a worker, including a failed worker whose
    /// state export must report its error rather than silently reuse an old preset.
    #[must_use]
    pub fn has_worker(&self, node_id: &NodeId) -> bool {
        self.live_control(node_id).is_ok()
    }

    /// Returns the exact active owner only when its worker has failed.
    #[must_use]
    pub fn failed_worker_instance(&self, node_id: &NodeId) -> Option<u64> {
        let control = self.active_control(node_id)?;
        let header = control.shared.header();
        (header.worker_status.load(Ordering::Acquire) == WORKER_FAILED
            || header.shutdown.load(Ordering::Acquire) != 0)
            .then_some(header.instance_id)
    }

    /// Reserves one explicit retry for the exact failed active worker instance.
    /// The cooldown lives with that worker owner, so retired identities are never retained.
    /// # Errors
    /// Rejects absent/healthy/stale workers and retries inside the per-instance cooldown.
    pub fn begin_failed_worker_retry(
        &self,
        node_id: &NodeId,
        expected_instance_id: u64,
        cooldown: Duration,
    ) -> Result<(), &'static str> {
        let control = self
            .active_control(node_id)
            .ok_or("processor is not active")?;
        let header = control.shared.header();
        if header.instance_id != expected_instance_id {
            return Err("processor instance changed");
        }
        if header.worker_status.load(Ordering::Acquire) != WORKER_FAILED
            && header.shutdown.load(Ordering::Acquire) == 0
        {
            return Err("processor worker has not failed");
        }
        let mut last_retry = control
            .last_retry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if last_retry.is_some_and(|previous| now.saturating_duration_since(previous) < cooldown) {
            return Err("processor retry is cooling down");
        }
        *last_retry = Some(now);
        Ok(())
    }

    fn active_control(&self, node_id: &NodeId) -> Option<Arc<HostedProcessorControl>> {
        let mut controls = self
            .controls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = controls.get_mut(node_id)?;
        entries.retain(|entry| entry.strong_count() != 0);
        entries.iter().rev().find_map(|entry| {
            let control = entry.upgrade()?;
            control
                .active_owner
                .load(Ordering::Acquire)
                .then_some(control)
        })
    }

    fn live_control(&self, node_id: &NodeId) -> Result<Arc<HostedProcessorControl>, String> {
        let mut controls = self
            .controls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = controls
            .get_mut(node_id)
            .ok_or_else(|| format!("processor node is not active: {node_id}"))?;
        entries.retain(|entry| entry.strong_count() != 0);
        entries
            .last()
            .and_then(Weak::upgrade)
            .ok_or_else(|| format!("processor node is not active: {node_id}"))
    }
}

struct ProcessorControlRegistration {
    controls: Weak<SharedProcessorControls>,
    node_id: NodeId,
    registered: Weak<HostedProcessorControl>,
}

impl Drop for ProcessorControlRegistration {
    fn drop(&mut self) {
        let Some(controls) = self.controls.upgrade() else {
            return;
        };
        let mut controls = controls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let remove_node = if let Some(entries) = controls.get_mut(&self.node_id) {
            entries.retain(|entry| {
                entry.strong_count() != 0 && !Weak::ptr_eq(entry, &self.registered)
            });
            entries.is_empty()
        } else {
            false
        };
        if remove_node {
            controls.remove(&self.node_id);
        }
    }
}

struct HostedProcessorControl {
    shared: ControlMailbox,
    // Keep this after `shared`: its Drop unlinks only after the mailbox mapping closes.
    transport_file_cleanup: SharedTransportFileCleanup,
    input_writer: Mutex<()>,
    reuse: Mutex<Option<reuse::ReuseSignature>>,
    active_owner: AtomicBool,
    last_retry: Mutex<Option<Instant>>,
    timing: timing::TimingMetrics,
}

impl HostedProcessorControl {
    fn new(shared: ControlMailbox) -> Self {
        Self {
            shared,
            transport_file_cleanup: SharedTransportFileCleanup::default(),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
            last_retry: Mutex::new(None),
            timing: timing::TimingMetrics::default(),
        }
    }

    fn own_transport_file(&self, path: PathBuf) {
        self.transport_file_cleanup.0.get_or_init(|| path);
    }

    fn snapshot_state(&self, timeout: Duration) -> Result<Vec<u8>, ProcessorStateError> {
        let header = self.shared.header();
        header
            .editor_input
            .cancel(header.editor_sequence.load(Ordering::Acquire));
        let deadline = Instant::now() + timeout.min(PROCESSOR_STATE_TIMEOUT);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.request_blob(editor_surface::NATIVE_STATE, remaining) {
                Ok(bytes) => return Ok(bytes),
                Err(ProcessorStateError::Busy | ProcessorStateError::Pending)
                    if !remaining.is_zero()
                        && self.shared.last_request_kind() == editor_surface::EDITOR_FRAME =>
                {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn request_blob(&self, kind: u32, timeout: Duration) -> Result<Vec<u8>, ProcessorStateError> {
        self.shared.try_lock()?.request_blob(kind, timeout)
    }

    fn set_parameter(&self, id: u32, value: f64) -> Result<(), String> {
        // HTTP/Tauri requests may arrive concurrently. Serializing publishers prevents a
        // parameter ID from being paired with another request's value; the worker never locks.
        let _writer = self.shared.lock();
        let header = self.shared.header();
        let count = header.parameter_count.load(Ordering::Relaxed) as usize;
        let index = match header.parameters[..count]
            .iter()
            .position(|slot| slot.id.load(Ordering::Relaxed) == id)
        {
            Some(index) => index,
            None if count < PARAMETER_CAPACITY => {
                header.parameters[count].id.store(id, Ordering::Relaxed);
                header
                    .parameter_count
                    .store(u32::try_from(count + 1).unwrap(), Ordering::Release);
                count
            }
            None => return Err("processor parameter mailbox capacity exceeded".to_owned()),
        };
        let slot = &header.parameters[index];
        slot.value_bits.store(value.to_bits(), Ordering::Release);
        slot.pending.store(true, Ordering::Release);
        header.parameters_pending.store(true, Ordering::Release);
        Ok(())
    }

    fn set_editor_open(&self, open: bool) {
        let _writer = self.shared.lock();
        let header = self.shared.header();
        if open
            && header.editor_command.load(Ordering::Acquire) == EDITOR_COMMAND_OPEN
            && matches!(
                header.editor_state.load(Ordering::Acquire),
                EDITOR_STATE_OPEN | EDITOR_STATE_OPENING
            )
            && header.editor_failure.load(Ordering::Acquire) == EDITOR_FAILURE_NONE
        {
            return;
        }
        let next = header
            .editor_sequence
            .load(Ordering::Relaxed)
            .wrapping_add(1);
        header
            .editor_failure
            .store(EDITOR_FAILURE_NONE, Ordering::Relaxed);
        if open {
            header
                .editor_state
                .store(EDITOR_STATE_OPENING, Ordering::Relaxed);
        }
        header.editor_command.store(
            if open {
                EDITOR_COMMAND_OPEN
            } else {
                EDITOR_COMMAND_CLOSE
            },
            Ordering::Relaxed,
        );
        header.editor_sequence.store(next, Ordering::Release);
    }

    fn editor_status(&self, node_id: &NodeId) -> ProcessorEditorStatus {
        let header = self.shared.header();
        let worker_ready = header.worker_status.load(Ordering::Acquire) == WORKER_READY;
        let state = if worker_ready {
            match header.editor_state.load(Ordering::Acquire) {
                EDITOR_STATE_CLOSED => ProcessorEditorState::Closed,
                EDITOR_STATE_OPENING => ProcessorEditorState::Opening,
                EDITOR_STATE_OPEN => ProcessorEditorState::Open,
                EDITOR_STATE_FAILED => ProcessorEditorState::Failed,
                _ => ProcessorEditorState::Unavailable,
            }
        } else {
            ProcessorEditorState::Failed
        };
        let raw_failure = if worker_ready {
            header.editor_failure.load(Ordering::Acquire)
        } else {
            EDITOR_FAILURE_WORKER_UNAVAILABLE
        };
        let failure = match raw_failure {
            EDITOR_FAILURE_NO_EDITOR => Some(ProcessorEditorFailure::NoEditor),
            EDITOR_FAILURE_DISPLAY_UNAVAILABLE => Some(ProcessorEditorFailure::DisplayUnavailable),
            EDITOR_FAILURE_HOST_WINDOW => Some(ProcessorEditorFailure::HostWindowFailed),
            EDITOR_FAILURE_PLUGIN_REJECTED => Some(ProcessorEditorFailure::PluginRejected),
            EDITOR_FAILURE_WORKER_UNAVAILABLE => Some(ProcessorEditorFailure::WorkerUnavailable),
            _ => None,
        };
        let width = header.editor_width.load(Ordering::Relaxed);
        let height = header.editor_height.load(Ordering::Relaxed);
        ProcessorEditorStatus {
            protocol_version: PROTOCOL_VERSION,
            node_id: node_id.as_str().to_owned(),
            state,
            width: (width != 0).then_some(width),
            height: (height != 0).then_some(height),
            resizable: header.editor_resizable.load(Ordering::Relaxed) != 0,
            failure,
        }
    }
}

#[derive(Default)]
struct SharedTransportFileCleanup(OnceLock<PathBuf>);

impl Drop for SharedTransportFileCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.get() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl HostedLatencySource for HostedProcessorControl {
    fn frames(&self) -> Option<u32> {
        let header = self.shared.header();
        if header.worker_status.load(Ordering::Acquire) != WORKER_READY
            || header.shutdown.load(Ordering::Acquire) != 0
        {
            return None;
        }
        let intrinsic = latency::decode_intrinsic_frames(
            header.intrinsic_latency_frames.load(Ordering::Acquire),
        )?;
        header.prepared_block_frames().checked_add(intrinsic)
    }
}

struct HostedExternalProcessor {
    child_stdin: Option<ChildStdin>,
    worker_monitor: Option<WorkerMonitor>,
    shared: Option<ParentAudio>,
    config_path: PathBuf,
    layout: AudioLayout,
    dry_delay: dry_delay::DryDelay,
    next_sequence: u64,
    timing: timing::TimingProbe,
    control: Option<Arc<HostedProcessorControl>>,
    registration: Option<ProcessorControlRegistration>,
}

impl HostedExternalProcessor {
    fn new(
        entry: &CatalogEntry,
        config: &ProviderNodeConfig,
        format: RenderFormat,
        state_store: Option<&mixrelay_processor_state::StateStore>,
    ) -> Result<Self, String> {
        let selected = selected_ports(&entry.descriptor, config)?;
        let input_channels = usize::from(
            selected
                .total_input_channels()
                .map_err(|error| error.to_string())?,
        );
        let output_channels = usize::from(
            selected
                .total_output_channels()
                .map_err(|error| error.to_string())?,
        );
        let block_frames = format.block_frames as usize;
        let dry_delay = dry_delay::DryDelay::for_ports(&selected, block_frames)
            .map_err(|error| error.to_string())?;
        let (shared_path, config_path) = worker_paths().map_err(|error| error.to_string())?;
        let layout = AudioLayout {
            inputs: input_channels,
            outputs: output_channels,
            block_frames,
        };
        let (shared, control) = shared_transport::create(&shared_path, layout)?;
        let control = Arc::new(HostedProcessorControl::new(control));
        if let Err(error) = state::write_worker_config(
            &config_path,
            format.sample_rate,
            &config.parameters,
            state_store,
            &selected,
        ) {
            drop(control);
            drop(shared);
            cleanup_worker_paths(&shared_path, &config_path);
            return Err(error);
        }
        let mut child = match spawn_processor_worker(entry, &shared_path, &config_path) {
            Ok(child) => child,
            Err(error) => {
                drop(control);
                drop(shared);
                cleanup_worker_paths(&shared_path, &config_path);
                return Err(error);
            }
        };
        let child_stdin = child.stdin.take();
        if let Err(error) = wait_for_worker(&mut child, &shared) {
            drop(child_stdin);
            let _ = child.kill();
            let _ = child.wait();
            drop(control);
            drop(shared);
            cleanup_worker_paths(&shared_path, &config_path);
            return Err(error);
        }
        let worker_monitor = match WorkerMonitor::spawn(child, Arc::clone(&control)) {
            Ok(monitor) => monitor,
            Err((error, mut child)) => {
                drop(child_stdin);
                let _ = child.kill();
                let _ = child.wait();
                drop(control);
                drop(shared);
                cleanup_worker_paths(&shared_path, &config_path);
                return Err(error);
            }
        };
        control.own_transport_file(shared_path.clone());
        Ok(Self {
            child_stdin,
            worker_monitor: Some(worker_monitor),
            shared: Some(shared),
            config_path,
            layout,
            next_sequence: 1,
            dry_delay,
            control: Some(control),
            timing: timing::TimingProbe::default(),
            registration: None,
        })
    }

    fn control(&self) -> Arc<HostedProcessorControl> {
        Arc::clone(
            self.control
                .as_ref()
                .expect("hosted processor retains control mapping until drop"),
        )
    }
}

fn spawn_processor_worker(
    entry: &CatalogEntry,
    shared_path: &Path,
    config_path: &Path,
) -> Result<Child, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg(PLUGIN_WORKER_ARGUMENT)
        .arg(entry.rack.locator.encode()?);
    command
        .arg(shared_path)
        .arg(config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.spawn().map_err(|error| error.to_string())
}

impl HostedAudioProcessor for HostedExternalProcessor {
    fn suspend_processing(&mut self) {
        if let Some(shared) = &mut self.shared {
            shared.discard_history();
        }
        self.dry_delay.reset();
    }

    fn set_active(&mut self, active: bool) {
        if let Some(control) = &self.control {
            control.active_owner.store(active, Ordering::Release);
        }
    }
    fn reuse_identity(&self) -> Option<u64> {
        self.control
            .as_ref()
            .map(|control| control.shared.header().instance_id)
    }

    fn latency_source(&self) -> Option<Arc<dyn HostedLatencySource>> {
        self.control.as_ref().map(|control| {
            // Retain this prepared worker's mapping across plan adoption and retirement.
            let source: Arc<dyn HostedLatencySource> =
                Arc::<HostedProcessorControl>::clone(control);
            source
        })
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool {
        self.set_active(true);
        let Some(input_samples) = self.layout.inputs.checked_mul(frames) else {
            return false;
        };
        let Some(output_samples) = self.layout.outputs.checked_mul(frames) else {
            return false;
        };
        if frames == 0
            || frames > self.layout.block_frames
            || input.len() < input_samples
            || output.len() < output_samples
        {
            return false;
        }
        let shared = self
            .shared
            .as_mut()
            .expect("hosted processor retains audio mapping until drop");
        let received = shared.receive(output, frames, |sequence| {
            if let Some(control) = &self.control {
                self.timing.received(sequence, &control.timing);
            }
        });
        if shared.submit(self.next_sequence, input, frames, || {
            if let Some(control) = &self.control {
                self.timing.submitted(self.next_sequence, &control.timing);
            }
        }) {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        // A missed deadline must not jump ahead to current input. Keep the dry path
        // aligned to the prepared IPC delay even while healthy output is available.
        // Intrinsic plug-in latency is a separate compensation boundary.
        self.dry_delay
            .advance(input, (!received).then_some(output), frames)
    }
}

impl Drop for HostedExternalProcessor {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared.header().shutdown.store(1, Ordering::Release);
        }
        // Closing stdin also makes an orphaned worker exit if the mapping is no longer polled.
        drop(self.child_stdin.take());
        if let Some(mut monitor) = self.worker_monitor.take() {
            monitor.stop_and_join();
        }
        // A retained latency reader keeps the control mapping and its transport-file cleanup
        // guard alive. Release the parent mapping before this processor's control Arc; the final
        // control handle unlinks the path only after its mailbox mapping has closed.
        drop(self.registration.take());
        drop(self.shared.take());
        drop(self.control.take());
        cleanup_worker_config_path(&self.config_path);
    }
}

struct WorkerMonitor {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WorkerMonitor {
    fn spawn(child: Child, control: Arc<HostedProcessorControl>) -> Result<Self, (String, Child)> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let child_slot = Arc::new(Mutex::new(Some(child)));
        let thread_child = Arc::clone(&child_slot);
        let thread = thread::Builder::new()
            .name("mixrelay-processor-monitor".to_owned())
            .spawn(move || {
                let mut child = thread_child
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .expect("worker child is transferred exactly once");
                let mut stop_deadline = None;
                let mut next_timing_sample = Instant::now();
                loop {
                    if Instant::now() >= next_timing_sample {
                        control.timing.request();
                        next_timing_sample = Instant::now() + Duration::from_secs(1);
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            if !thread_stop.load(Ordering::Acquire) {
                                tracing::warn!(pid = child.id(), %status, "processor worker exited unexpectedly");
                                control
                                    .shared
                                    .header()
                                    .worker_status
                                    .store(WORKER_FAILED, Ordering::Release);
                            }
                            return;
                        }
                        Err(_) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            control
                                .shared
                                .header()
                                .worker_status
                                .store(WORKER_FAILED, Ordering::Release);
                            return;
                        }
                        Ok(None) => {}
                    }
                    if thread_stop.load(Ordering::Acquire) {
                        let deadline = stop_deadline
                            .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
                        if Instant::now() >= *deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            return;
                        }
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            });
        match thread {
            Ok(thread) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Err(error) => {
                let child = child_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .expect("failed monitor spawn leaves child with caller");
                Err((error.to_string(), child))
            }
        }
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WorkerMonitor {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn cleanup_worker_paths(shared_path: &Path, config_path: &Path) {
    let _ = std::fs::remove_file(shared_path);
    cleanup_worker_config_path(config_path);
}

fn cleanup_worker_config_path(config_path: &Path) {
    let _ = std::fs::remove_file(config_path);
}

fn wait_for_worker(child: &mut Child, shared: &ParentAudio) -> Result<(), String> {
    let deadline = Instant::now() + WORKER_START_TIMEOUT;
    loop {
        match shared.header().worker_status.load(Ordering::Acquire) {
            WORKER_READY => return Ok(()),
            WORKER_FAILED => return Err("processor worker initialization failed".to_owned()),
            _ => {}
        }
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("processor worker exited during initialization".to_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("processor worker initialization timed out".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn copy_shared_input_to_rack(
    source: &[f32],
    plugin: &mut rack_formats::RackWorkerPlugin,
    layout: AudioLayout,
) {
    let frames = source.len().checked_div(layout.inputs).unwrap_or(0);
    for (channel_index, channel) in plugin.inputs_mut().iter_mut().enumerate() {
        for frame in 0..frames {
            channel[frame] = source[frame * layout.inputs + channel_index];
        }
    }
}

fn copy_rack_output_to_shared(
    destination: &mut [f32],
    plugin: &rack_formats::RackWorkerPlugin,
    layout: AudioLayout,
) {
    let frames = destination.len().checked_div(layout.outputs).unwrap_or(0);
    for frame in 0..frames {
        for (channel_index, channel) in plugin.outputs().iter().enumerate() {
            destination[frame * layout.outputs + channel_index] = channel[frame];
        }
    }
}

fn bypass_rack(plugin: &mut rack_formats::RackWorkerPlugin, frames: usize) {
    plugin.bypass(frames);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn the_worker_waiter_returns_on_a_changed_word_and_polls_after_failures() {
        let word = std::sync::atomic::AtomicU64::new(3);
        let mut waiter = Waiter::default();
        let started = std::time::Instant::now();
        // A stale snapshot ends the wait at once.
        waiter.wait(&word, 2, Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(1));
        // After repeated failures the futex is no longer trusted; a short poll remains.
        waiter.failures = Waiter::MAXIMUM_FAILURES;
        waiter.wait(&word, 3, Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn plug_in_blocks_last_their_frames_at_the_prepared_rate() {
        assert_eq!(
            block_period(128, 48_000.0),
            Duration::from_secs_f64(128.0 / 48_000.0)
        );
        // An unusable rate falls back to 48 kHz rather than a zero or infinite period.
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(block_period(480, rate), Duration::from_millis(10));
        }
    }

    #[test]
    fn saved_selection_is_not_retargeted_by_equal_totals_or_catalog_order() {
        let stereo = mixrelay_audio_graph::ProviderPorts::canonical_stereo("stereo");
        let mut sidechain = stereo.clone();
        sidechain.layout_id = "stereo-sidechain".into();
        let mut key_bus = stereo.inputs[0].clone();
        key_bus.id = mixrelay_audio_graph::PortId::from("key");
        key_bus.kind = mixrelay_audio_graph::ProviderBusKind::Sidechain;
        sidechain.inputs.push(key_bus);
        let mut descriptor = ExternalProcessorDescriptor {
            type_id: "clap:fixture".into(),
            display_name: "Fixture".into(),
            vendor: String::new(),
            version: String::new(),
            format: mixrelay_control_protocol::ExternalProcessorFormat::Clap,
            layouts: vec![stereo.clone(), sidechain.clone()],
            default_layout_id: stereo.layout_id.clone(),
            has_editor: false,
            parameters: Vec::new(),
        };
        let mut config = ProviderNodeConfig {
            type_id: descriptor.type_id.as_str().into(),
            ports: Some(sidechain.clone()),
            parameters: serde_json::json!({}),
        };
        descriptor.layouts.reverse();
        assert_eq!(selected_ports(&descriptor, &config).unwrap(), sidechain);
        let mut renamed = sidechain.clone();
        renamed.inputs[1].name = "Translated key".into();
        config.ports = Some(renamed);
        assert_eq!(selected_ports(&descriptor, &config).unwrap(), sidechain);
        let mut wrong = sidechain.clone();
        wrong.inputs[1].kind = mixrelay_audio_graph::ProviderBusKind::Main;
        config.ports = Some(wrong);
        assert!(selected_ports(&descriptor, &config).is_err());
        config.ports = Some(sidechain);
        descriptor.layouts = vec![stereo.clone()];
        assert!(selected_ports(&descriptor, &config).is_err());
        config.ports = None;
        assert_eq!(selected_ports(&descriptor, &config).unwrap(), stereo);
    }

    fn saved_ports(inputs: u16, outputs: u16) -> mixrelay_audio_graph::ProviderPorts {
        let mut ports = mixrelay_audio_graph::ProviderPorts::canonical_stereo("fixture-layout");
        ports.inputs[0].channels = inputs;
        ports.inputs[0].channel_format = mixrelay_audio_graph::ProviderChannelFormat::Discrete;
        ports.outputs[0].channels = outputs;
        ports.outputs[0].channel_format = mixrelay_audio_graph::ProviderChannelFormat::Discrete;
        ports
    }

    #[test]
    fn missing_plugin_uses_saved_ports_and_silence_instead_of_passthrough() {
        let config = ProviderNodeConfig {
            type_id: "vst3:not-installed".into(),
            parameters: serde_json::json!({}),
            ports: Some(saved_ports(8, 8)),
        };
        let node = NodeSpec {
            control: mixrelay_audio_graph::NodeControl::default(),
            id: NodeId::from("missing"),
            label: None,
            config: NodeConfig::Provider(config.clone()),
        };
        let context = CompileContext {
            format: RenderFormat {
                sample_rate: 48_000,
                block_frames: 32,
            },
            limits: mixrelay_audio_graph::CompileLimits::default(),
        };
        let resolver = CatalogNodeResolver::new(ProcessorCatalog::default());
        let ports = resolver.resolve(&node, &context).unwrap();
        assert_eq!(ports.inputs[0].channels(), 8);
        let mut processor = ExternalProcessorFactory::new(ProcessorCatalog::default())
            .prepare(HostedProcessorPrepareRequest {
                node_id: &node.id,
                config: &config,
                format: context.format,
                input_channels: 8,
                output_channels: 8,
            })
            .unwrap();
        let mut output = [1.0; 256];
        assert!(processor.process(&[0.75; 256], &mut output, 32));
        assert!(
            output
                .iter()
                .all(|sample| sample.to_bits() == 0.0_f32.to_bits())
        );
        let mut invalid = node;
        if let NodeConfig::Provider(config) = &mut invalid.config {
            config.ports = Some(saved_ports(0, 65));
        }
        assert!(resolver.resolve(&invalid, &context).is_err());
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn measure_shared_audio_transport() {
        const ITERATIONS: u32 = 100_000;
        if std::env::var_os("MIXRELAY_PROCESSOR_TRANSPORT_BENCH").is_none() {
            return;
        }
        for channels in [2, 8, 64] {
            for frames in [32, 256] {
                let (shared_path, config_path, shared, mailbox) =
                    audio_mapping(channels, channels, frames);
                let control = Arc::new(HostedProcessorControl::new(mailbox));
                control.own_transport_file(shared_path.clone());
                let mut processor = HostedExternalProcessor {
                    child_stdin: None,
                    worker_monitor: None,
                    shared: Some(shared),
                    config_path,
                    layout: AudioLayout {
                        inputs: channels,
                        outputs: channels,
                        block_frames: frames,
                    },
                    next_sequence: 1,
                    dry_delay: dry_delay::DryDelay::new(channels, channels, frames).unwrap(),
                    control: Some(control),
                    timing: timing::TimingProbe::default(),
                    registration: None,
                };
                let input = vec![0.25; channels * frames];
                let mut output = vec![0.0; channels * frames];
                assert!(processor.process(&input, &mut output, frames));
                let mut rounds = [0.0; 7];
                for elapsed in &mut rounds {
                    let start = Instant::now();
                    for _ in 0..ITERATIONS {
                        let shared = processor.shared.as_ref().expect("live benchmark mapping");
                        shared.acknowledge_benchmark_block();
                        assert!(processor.process(
                            std::hint::black_box(&input),
                            std::hint::black_box(&mut output),
                            frames,
                        ));
                    }
                    *elapsed = start.elapsed().as_secs_f64() * 1e9 / f64::from(ITERATIONS);
                }
                rounds.sort_by(f64::total_cmp);
                println!(
                    "processor_transport channels={channels} frames={frames} median_ns={:.3}",
                    rounds[3]
                );
            }
        }
    }

    fn shared_mapping(
        inputs: usize,
        outputs: usize,
        frames: usize,
    ) -> (PathBuf, PathBuf, ControlMailbox) {
        let (shared_path, config_path) = worker_paths().expect("private worker paths");
        let (audio, control) = shared_transport::create(
            &shared_path,
            AudioLayout {
                inputs,
                outputs,
                block_frames: frames,
            },
        )
        .expect("shared transport");
        drop(audio);
        (shared_path, config_path, control)
    }

    #[test]
    fn worker_setup_cleanup_removes_the_owned_transport() {
        let (shared_path, config_path) = worker_paths().expect("private worker paths");
        let (audio, control) = shared_transport::create(
            &shared_path,
            AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 128,
            },
        )
        .expect("shared transport");
        drop(control);
        drop(audio);
        assert!(shared_path.exists());
        cleanup_worker_paths(&shared_path, &config_path);
        assert!(!shared_path.exists());
    }

    fn audio_mapping(
        inputs: usize,
        outputs: usize,
        frames: usize,
    ) -> (PathBuf, PathBuf, ParentAudio, ControlMailbox) {
        let (shared_path, config_path) = worker_paths().expect("private worker paths");
        let (audio, control) = shared_transport::create(
            &shared_path,
            AudioLayout {
                inputs,
                outputs,
                block_frames: frames,
            },
        )
        .expect("shared transport");
        (shared_path, config_path, audio, control)
    }

    #[test]
    fn stalled_or_failed_worker_does_not_block_an_independent_processor() {
        let (mut unavailable, _) = test_processor();
        let (mut healthy, healthy_path) = test_processor();
        let (mut worker, _) = shared_transport::worker(&healthy_path).unwrap();
        let mut output = [0.0; 8];
        let input = [0.25; 8];
        assert!(unavailable.process(&input, &mut output, 4));
        assert!(healthy.process(&input, &mut output, 4));
        assert!(
            output
                .iter()
                .all(|sample| sample.to_bits() == 0.0_f32.to_bits())
        );
        for sequence in 1..=64 {
            assert_eq!(worker.receive(|_| {}), Some(4));
            assert!(worker.publish(|output| output.fill(0.5)));
            if sequence == 32 {
                (unavailable.shared.as_ref().unwrap())
                    .header()
                    .worker_status
                    .store(WORKER_FAILED, Ordering::Release);
            }
            assert!(unavailable.process(&input, &mut output, 4));
            assert_eq!(output.map(f32::to_bits), input.map(f32::to_bits));
            assert!(healthy.process(&input, &mut output, 4));
            assert!(
                output
                    .iter()
                    .all(|sample| (*sample - 0.5).abs() < f32::EPSILON)
            );
        }
        assert_eq!(
            unavailable.next_sequence, 2,
            "a stalled mailbox cannot accumulate requests"
        );
        assert_eq!(healthy.next_sequence, 66);
    }

    fn test_processor() -> (HostedExternalProcessor, PathBuf) {
        let (shared_path, config_path, shared, mailbox) = audio_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl::new(mailbox));
        control.own_transport_file(shared_path.clone());
        let processor = HostedExternalProcessor {
            child_stdin: None,
            worker_monitor: None,
            shared: Some(shared),
            config_path,
            layout: AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 4,
            },
            next_sequence: 1,
            dry_delay: dry_delay::DryDelay::new(2, 2, 4).unwrap(),
            control: Some(control),
            timing: timing::TimingProbe::default(),
            registration: None,
        };
        (processor, shared_path)
    }

    #[test]
    fn startup_padding_does_not_trigger_immediate_graph_bypass_or_repeat_audio() {
        let (mut processor, shared_path) = test_processor();
        let (mut worker, _) = shared_transport::worker(&shared_path).unwrap();
        let first = [1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0];
        let mut output = [f32::NAN; 8];
        assert!(
            processor.process(&first, &mut output, 4),
            "startup silence is a valid delayed output, not a request for immediate graph bypass"
        );
        assert_eq!(output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        assert_eq!(worker.receive(|source| assert_eq!(source, first)), Some(4));
        assert!(worker.publish(|destination| destination.copy_from_slice(&first)));
        let second = [5.0, 50.0, 6.0, 60.0, 7.0, 70.0, 8.0, 80.0];
        assert!(processor.process(&second, &mut output, 4));
        assert_eq!(output.map(f32::to_bits), first.map(f32::to_bits));
        assert_eq!(worker.receive(|source| assert_eq!(source, second)), Some(4));
        assert!(worker.publish(|destination| destination.copy_from_slice(&second)));
        assert!(processor.process(&[0.0; 8], &mut output, 4));
        assert_eq!(output.map(f32::to_bits), second.map(f32::to_bits));
    }

    #[test]
    fn missed_response_preserves_dry_delay_across_uneven_callbacks_and_suspend() {
        let (mut processor, _) = test_processor();
        let mut output = [f32::NAN; 8];
        assert!(processor.process(&[1.0; 6], &mut output[..6], 3));
        assert_eq!(output[..6], [0.0; 6]);
        // The callback straddles startup padding, but there is no worker response.
        assert!(processor.process(&[2.0; 4], &mut output[..4], 2));
        assert_eq!(output[..4], [0.0, 0.0, 1.0, 1.0]);
        assert!(processor.process(&[3.0; 8], &mut output, 4));
        assert_eq!(
            output.map(f32::to_bits),
            [1.0_f32, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0].map(f32::to_bits)
        );
        let allocations = allocation_counter::measure(|| {
            for _ in 0..100 {
                processor.suspend_processing();
            }
            assert!(processor.process(&[4.0; 8], &mut output, 4));
            assert_eq!(output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
            assert!(processor.process(&[5.0; 8], &mut output, 4));
            assert_eq!(output.map(f32::to_bits), [4.0_f32; 8].map(f32::to_bits));
        });
        assert_eq!(allocations.count_total, 0);
    }

    #[test]
    fn suspended_processor_rejects_late_worker_audio_and_resumes_fresh_packets() {
        let (mut processor, shared_path) = test_processor();
        let (mut worker, _) = shared_transport::worker(&shared_path).unwrap();
        let mut output = [f32::NAN; 8];
        assert!(processor.process(&[7.0; 8], &mut output, 4));
        assert_eq!(worker.receive(|_| {}), Some(4));
        processor.suspend_processing();
        assert!(worker.publish(|pcm| pcm.fill(7.0)));
        assert!(processor.process(&[2.0; 8], &mut output, 4));
        assert_eq!(output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        assert_eq!(worker.receive(|pcm| assert_eq!(pcm, [2.0; 8])), Some(4));
        assert!(worker.publish(|pcm| pcm.fill(3.0)));
        assert!(processor.process(&[4.0; 8], &mut output, 4));
        assert_eq!(output.map(f32::to_bits), [3.0_f32; 8].map(f32::to_bits));
    }

    #[test]
    fn consumed_input_before_output_publication_does_not_stall_the_mailbox() {
        let (mut processor, shared_path) = test_processor();
        let (mut worker, _) = shared_transport::worker(&shared_path).unwrap();
        let input = [0.25; 8];
        let mut output = [0.0; 8];
        assert!(processor.process(&input, &mut output, 4));
        assert_eq!(worker.receive(|_| {}), Some(4));
        assert!(processor.process(&input, &mut output, 4));
        assert_eq!(output.map(f32::to_bits), input.map(f32::to_bits));
        assert_eq!(
            processor.next_sequence, 3,
            "consumed input releases the slot before output"
        );
        assert!(worker.publish(|output| output.fill(1.0)));
        assert!(
            processor.process(&input, &mut output, 4),
            "late output must not prevent aligned dry audio"
        );
        assert_eq!(output.map(f32::to_bits), input.map(f32::to_bits));
        assert_eq!(worker.receive(|_| {}), Some(4));
        assert!(worker.publish(|output| output.fill(2.0)));
        assert!(
            processor.process(&input, &mut output, 4),
            "the busy interval uses aligned dry audio"
        );
        assert_eq!(output.map(f32::to_bits), input.map(f32::to_bits));
        for sequence in 3..=66 {
            assert_eq!(worker.receive(|_| {}), Some(4));
            let processed = f32::from(u16::try_from(sequence).unwrap());
            assert!(worker.publish(|output| output.fill(processed)));
            assert!(processor.process(&input, &mut output, 4));
            assert_eq!(processor.next_sequence, sequence + 2);
            assert!(
                output
                    .iter()
                    .all(|sample| sample.to_bits() == processed.to_bits())
            );
            assert_eq!(
                processor.shared.as_ref().unwrap().acknowledged_output(),
                sequence
            );
        }
    }

    #[test]
    fn shared_audio_regions_round_trip_without_growing() {
        let (shared_path, config_path, mut shared, control) = audio_mapping(2, 2, 4);
        let (mut worker, _) = shared_transport::worker(&shared_path).unwrap();
        let original_length = std::fs::metadata(&shared_path).unwrap().len();
        let input = [1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0];
        assert!(shared.submit(1, &input, 4, || {}));
        assert_eq!(worker.receive(|source| assert_eq!(source, input)), Some(4));
        let expected = [5.0, 50.0, 6.0, 60.0, 7.0, 70.0, 8.0, 80.0];
        assert!(worker.publish(|destination| destination.copy_from_slice(&expected)));
        let mut output = [0.0; 8];
        assert!(shared.receive(&mut output, 4, |_| {}));
        assert_eq!(output.map(f32::to_bits), expected.map(f32::to_bits));
        assert_eq!(
            std::fs::metadata(&shared_path).unwrap().len(),
            original_length
        );
        drop(worker);
        drop(shared);
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
        assert!(!shared_path.exists());
    }

    #[test]
    fn timing_status_requires_an_active_healthy_owner() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl::new(shared));
        let registry = ProcessorControlRegistry::default();
        let node = NodeId::from("timed-node");
        let registration = registry.register(&node, &control);
        let mut probe = timing::TimingProbe::default();
        control.timing.request();
        probe.submitted(1, &control.timing);
        thread::sleep(Duration::from_millis(1));
        probe.received(1, &control.timing);
        assert_eq!(registry.processing_time_ns(&node), None);
        control.active_owner.store(true, Ordering::Release);
        let header = control.shared.header();
        header.worker_status.store(WORKER_READY, Ordering::Release);
        assert!(registry.processing_time_ns(&node).is_some());
        header.worker_status.store(WORKER_FAILED, Ordering::Release);
        assert_eq!(registry.processing_time_ns(&node), None);
        drop(registration);
        drop(control);
        assert_eq!(registry.processing_time_ns(&node), None);
        cleanup_worker_paths(&shared_path, &config_path);
    }

    #[test]
    fn retry_identity_tracks_only_the_active_failed_owner_and_is_rate_limited_per_instance() {
        let (failed_path, failed_config, failed_mailbox) = shared_mapping(2, 2, 4);
        let failed = Arc::new(HostedProcessorControl::new(failed_mailbox));
        failed.active_owner.store(true, Ordering::Release);
        failed
            .shared
            .header()
            .worker_status
            .store(WORKER_FAILED, Ordering::Release);
        let failed_id = failed.shared.header().instance_id;
        let registry = ProcessorControlRegistry::default();
        let node = NodeId::from("retry-node");
        let failed_registration = registry.register(&node, &failed);

        assert_eq!(registry.failed_worker_instance(&node), Some(failed_id));
        registry
            .begin_failed_worker_retry(&node, failed_id, Duration::from_mins(1))
            .expect("first explicit retry should be admitted");
        assert_eq!(
            registry.begin_failed_worker_retry(&node, failed_id, Duration::from_mins(1)),
            Err("processor retry is cooling down")
        );

        failed.active_owner.store(false, Ordering::Release);
        let (healthy_path, healthy_config, healthy_mailbox) = shared_mapping(2, 2, 4);
        let healthy = Arc::new(HostedProcessorControl::new(healthy_mailbox));
        healthy.active_owner.store(true, Ordering::Release);
        healthy
            .shared
            .header()
            .worker_status
            .store(WORKER_READY, Ordering::Release);
        let healthy_id = healthy.shared.header().instance_id;
        let healthy_registration = registry.register(&node, &healthy);
        assert_ne!(healthy_id, failed_id);
        assert_eq!(registry.failed_worker_instance(&node), None);
        assert_eq!(
            registry.begin_failed_worker_retry(&node, failed_id, Duration::ZERO),
            Err("processor instance changed")
        );

        healthy
            .shared
            .header()
            .worker_status
            .store(WORKER_FAILED, Ordering::Release);
        assert_eq!(registry.failed_worker_instance(&node), Some(healthy_id));
        registry
            .begin_failed_worker_retry(&node, healthy_id, Duration::ZERO)
            .expect("a replacement crash gets its own retry budget");

        drop(healthy_registration);
        drop(failed_registration);
        drop(healthy);
        drop(failed);
        cleanup_worker_paths(&healthy_path, &healthy_config);
        cleanup_worker_paths(&failed_path, &failed_config);
    }

    #[test]
    fn control_mailboxes_coalesce_and_registration_does_not_extend_lifetime() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl::new(shared));
        let registry = ProcessorControlRegistry::default();
        let node_id = NodeId::from("vst-node");
        let registration = registry.register(&node_id, &control);
        registry
            .set_parameter(&node_id, 7, 0.25)
            .expect("first parameter should publish");
        registry
            .set_parameter(&node_id, 9, 0.75)
            .expect("different parameter must retain the earlier ID");
        registry.set_parameter(&node_id, 7, 0.5).unwrap();
        let header = control.shared.header();
        let mut received = Vec::new();
        consume_parameters(header, |id, value| received.push((id, value)));
        assert_eq!(received, vec![(7, 0.5), (9, 0.75)]);
        consume_parameters(header, |_, _| panic!("consumed changes must not repeat"));

        header.worker_status.store(WORKER_READY, Ordering::Release);
        let opening = registry
            .set_editor_open(&node_id, true)
            .expect("editor request should publish");
        assert_eq!(opening.state, ProcessorEditorState::Opening);
        assert_eq!(header.editor_sequence.load(Ordering::Acquire), 1);
        assert_eq!(
            header.editor_command.load(Ordering::Relaxed),
            EDITOR_COMMAND_OPEN
        );

        header.editor_width.store(800, Ordering::Relaxed);
        header.editor_height.store(600, Ordering::Relaxed);
        header.editor_resizable.store(1, Ordering::Relaxed);
        header
            .editor_state
            .store(EDITOR_STATE_OPEN, Ordering::Release);
        let opened = registry
            .editor_status(&node_id)
            .expect("live editor status should be readable");
        assert_eq!(opened.state, ProcessorEditorState::Open);
        assert_eq!((opened.width, opened.height), (Some(800), Some(600)));
        assert!(opened.resizable);

        registry
            .set_editor_open(&node_id, false)
            .expect("close request should publish");
        assert_eq!(header.editor_sequence.load(Ordering::Acquire), 2);
        assert_eq!(
            header.editor_command.load(Ordering::Relaxed),
            EDITOR_COMMAND_CLOSE
        );

        drop(registration);
        assert!(registry.controls.read().unwrap().is_empty());
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
        assert!(!shared_path.exists());
    }

    #[test]
    fn parameter_mailbox_rejects_capacity_without_losing_other_ids() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = HostedProcessorControl::new(shared);
        for id in 0..u32::try_from(PARAMETER_CAPACITY).unwrap() {
            control.set_parameter(id, f64::from(id) / 4096.0).unwrap();
        }
        assert!(control.set_parameter(u32::MAX, 0.5).is_err());
        control.set_parameter(0, 0.75).unwrap();
        let mut count = 0;
        consume_parameters(control.shared.header(), |id, value| {
            let expected = if id == 0 {
                0.75
            } else {
                f64::from(id) / 4096.0
            };
            assert!((value - expected).abs() < f64::EPSILON);
            count += 1;
        });
        assert_eq!(count, PARAMETER_CAPACITY);
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }

    #[test]
    fn state_snapshot_roundtrips_binary_and_rejects_failed_or_oversized_exports() {
        for (state, expected_error) in [
            (Ok(vec![0, 1, 255, 13, 0]), None),
            (
                Err(ProcessorStateError::NativeFailure),
                Some(ProcessorStateError::NativeFailure),
            ),
            (
                Ok(vec![0; STATE_CAPACITY + 1]),
                Some(ProcessorStateError::TooLarge),
            ),
            (
                Err(ProcessorStateError::Unsupported),
                Some(ProcessorStateError::Unsupported),
            ),
        ] {
            let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
            let control = Arc::new(HostedProcessorControl::new(shared));
            control
                .shared
                .header()
                .worker_status
                .store(WORKER_READY, Ordering::Release);
            let (_, worker_control) = shared_transport::worker(&shared_path).unwrap();
            let worker = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while worker_control.pending_request().is_none() {
                    assert!(Instant::now() < deadline);
                    thread::yield_now();
                }
                worker_control.publish(1, state);
            });
            let result = control.snapshot_state(Duration::from_secs(2));
            worker.join().unwrap();
            if let Some(message) = expected_error {
                assert_eq!(result.unwrap_err(), message);
            } else {
                let saved = result.unwrap();
                assert_eq!(saved, vec![0, 1, 255, 13, 0]);
            }
            drop(control);
            cleanup_worker_paths(&shared_path, &config_path);
        }
    }

    #[test]
    fn concurrent_parameter_edits_never_pair_a_value_with_another_id() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl::new(shared));
        let finished = Arc::new(AtomicBool::new(false));
        let producer_control = Arc::clone(&control);
        let producer_finished = Arc::clone(&finished);
        let producer = thread::spawn(move || {
            for _ in 0..1000 {
                for id in 0..64 {
                    producer_control
                        .set_parameter(id, f64::from(id) / 64.0)
                        .unwrap();
                }
            }
            producer_finished.store(true, Ordering::Release);
        });
        let mut seen = [false; 64];
        let mut consume = || {
            consume_parameters(control.shared.header(), |id, value| {
                assert!((value - f64::from(id) / 64.0).abs() < f64::EPSILON);
                seen[id as usize] = true;
            });
        };
        while !finished.load(Ordering::Acquire) {
            consume();
        }
        producer.join().unwrap();
        consume();
        assert!(seen.into_iter().all(|seen| seen));
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }

    #[test]
    fn timed_out_state_request_is_not_reused_until_worker_acknowledges() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = HostedProcessorControl::new(shared);
        let (_, worker) = shared_transport::worker(&shared_path).unwrap();
        let header = control.shared.header();
        header.worker_status.store(WORKER_READY, Ordering::Release);
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Timeout)
        );
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Pending)
        );
        assert_eq!(worker.pending_request().unwrap().0, 1);
        worker.publish(1, Ok(vec![8]));
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Timeout)
        );
        assert_eq!(worker.pending_request().unwrap().0, 2);
        header.worker_status.store(WORKER_FAILED, Ordering::Release);
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Unavailable)
        );
        drop(worker);
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }

    #[cfg(unix)]
    #[test]
    fn monitor_reaps_a_crashed_worker_and_marks_transport_failed() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl::new(shared));
        let mut command = Command::new("/usr/bin/false");
        let child = command.spawn().expect("fixture process should spawn");
        let mut monitor = WorkerMonitor::spawn(child, Arc::clone(&control))
            .map_err(|(error, _)| error)
            .expect("monitor should spawn");
        let deadline = Instant::now() + Duration::from_secs(2);
        while control
            .shared
            .header()
            .worker_status
            .load(Ordering::Acquire)
            != WORKER_FAILED
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            control
                .shared
                .header()
                .worker_status
                .load(Ordering::Acquire),
            WORKER_FAILED
        );
        monitor.stop_and_join();
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }
}
