use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    mem::size_of,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use memmap2::{MmapMut, MmapOptions};
use tonenari_audio_engine::{
    HostedAudioProcessor, HostedProcessorFactory, HostedProcessorPrepareRequest,
};
use tonenari_audio_graph::{
    AUDIO_PORT_ID, BuiltInNodeResolver, CompileContext, CompiledPort, NodeConfig, NodeId,
    NodeResolutionError, NodeResolver, NodeSpec, PortDirection, PortId, ProviderNodeConfig,
    RenderFormat, ResolvedNode,
};
use tonenari_control_protocol::{
    ExternalProcessorCatalog, ExternalProcessorDescriptor, PROTOCOL_VERSION,
    ProcessorEditorFailure, ProcessorEditorState, ProcessorEditorStatus,
};

mod discovery;
mod editor_observation;
mod editor_surface;
mod native_editor;
mod rack_formats;
mod reuse;
mod worker_paths;
use worker_paths::worker_paths;

pub use editor_observation::ProcessorEditorCloseObservation;

pub use rack_formats::{
    PROBE_ARGUMENT as PLUGIN_PROBE_ARGUMENT, RackFormat, RackLocator,
    SCAN_ARGUMENT as PLUGIN_SCAN_ARGUMENT, WORKER_ARGUMENT as PLUGIN_WORKER_ARGUMENT,
    run_probe as run_plugin_probe, run_scan as run_plugin_scan,
};

#[cfg(not(target_has_atomic = "64"))]
compile_error!("ToneNari processor shared transport requires native 64-bit atomics");

const WORKER_START_TIMEOUT: Duration = Duration::from_secs(15);
const SHARED_MAGIC: u64 = 0x544e_5653_5433_3031;
const SHARED_VERSION: u32 = 4;
const PARAMETER_CAPACITY: usize = 4096;
const STATE_CAPACITY: usize = 4 * 1024 * 1024;
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
static NEXT_CONTROL_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Header shared by the engine and one VST worker process.
///
/// The sample regions immediately following this header are protected by the sequence
/// atomics: the producer never overwrites a region until the consumer publishes the matching
/// consumed sequence. This keeps the engine side lock-free without reading memory while the
/// worker writes it.
#[repr(C, align(64))]
struct SharedHeader {
    magic: u64,
    version: u32,
    input_channels: u32,
    output_channels: u32,
    block_frames: u32,
    worker_status: AtomicU32,
    shutdown: AtomicU32,
    input_sequence: AtomicU64,
    input_consumed: AtomicU64,
    output_sequence: AtomicU64,
    output_consumed: AtomicU64,
    process_failures: AtomicU64,
    parameter_count: AtomicU32,
    parameters_pending: AtomicBool,
    parameters: [ParameterSlot; PARAMETER_CAPACITY],
    state_request: AtomicU64,
    state_kind: AtomicU32,
    state_response: AtomicU64,
    state_length: AtomicU32,
    state_result: AtomicU32,
    editor_command: AtomicU32,
    editor_sequence: AtomicU64,
    editor_consumed: AtomicU64,
    editor_state: AtomicU32,
    editor_failure: AtomicU32,
    editor_width: AtomicU32,
    editor_height: AtomicU32,
    editor_resizable: AtomicU32,
    instance_id: u64,
    /// Broker-owned lifecycle, separate from the parent's optimistic Opening status.
    editor_observed_open: AtomicBool,
    editor_close_generation: AtomicU64,
    editor_input: editor_surface::InputRing,
}

/// Slots are assigned once per worker lifetime by serialized control publishers. Stable IDs
/// let the consumer load each atomic value independently, without a torn ID/value pair.
#[repr(C)]
struct ParameterSlot {
    id: AtomicU32,
    value_bits: AtomicU64,
    pending: AtomicBool,
}

impl ParameterSlot {
    fn empty() -> Self {
        Self {
            id: AtomicU32::new(0),
            value_bits: AtomicU64::new(0),
            pending: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Clone)]
struct CatalogEntry {
    descriptor: ExternalProcessorDescriptor,
    rack: RackCatalogTarget,
}

#[derive(Debug, Clone)]
struct RackCatalogTarget {
    locator: RackLocator,
    layout_index: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessorCatalog {
    snapshot: Arc<RwLock<discovery::CatalogSnapshot>>,
    /// Discovery does not block catalog readers or the render thread; only rescans serialize.
    scan_lock: Arc<Mutex<()>>,
}

impl ProcessorCatalog {
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
    let [encoded_locator, layout_index, shared_path, config_path] = arguments else {
        return Err(
            "processor worker requires locator, layout, shared-memory, and config paths".to_owned(),
        );
    };
    let locator = RackLocator::decode(encoded_locator)?;
    let layout_index = layout_index
        .parse::<usize>()
        .map_err(|error| format!("invalid processor layout index: {error}"))?;
    let shared_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(shared_path)
        .map_err(|error| error.to_string())?;
    // Separate mappings keep the DSP loop and UI broker from sharing Rust mutable ownership.
    // Synchronization of the common header is exclusively atomic; only the DSP mapping touches
    // sample storage.
    let mut audio_shared =
        unsafe { MmapOptions::new().map_mut(&shared_file) }.map_err(|error| error.to_string())?;
    let editor_shared = Arc::new(
        unsafe { MmapOptions::new().map_mut(&shared_file) }.map_err(|error| error.to_string())?,
    );
    let header = shared_header(&audio_shared)?;
    let layout = AudioLayout {
        inputs: header.input_channels as usize,
        outputs: header.output_channels as usize,
        block_frames: header.block_frames as usize,
    };
    let (sample_rate, parameters) = read_worker_configuration(config_path)?;
    let plugin = rack_formats::RackWorkerPlugin::load(
        &locator,
        layout_index,
        sample_rate,
        layout.block_frames,
        &parameters,
    )?;
    if plugin.input_channels() != layout.inputs || plugin.output_channels() != layout.outputs {
        return Err("loaded processor layout does not match the catalog".to_owned());
    }
    let has_editor = plugin.has_editor();
    let plugin = Arc::new(Mutex::new(plugin));
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_reader = Arc::clone(&shutdown);
    let parent_watch_shared = Arc::clone(&editor_shared);
    let _stdin_guard = thread::Builder::new()
        .name("tonenari-plugin-parent-watch".to_owned())
        .spawn(move || {
            let mut input = std::io::stdin();
            let mut byte = [0_u8; 1];
            while input.read(&mut byte).is_ok_and(|read| read != 0) {}
            shared_header_unchecked(&parent_watch_shared)
                .shutdown
                .store(1, Ordering::Release);
            shutdown_reader.store(true, Ordering::Release);
        })
        .map_err(|error| error.to_string())?;
    let dsp_plugin = Arc::clone(&plugin);
    let dsp_shutdown = Arc::clone(&shutdown);
    let dsp_thread = thread::Builder::new()
        .name("tonenari-plugin-dsp".to_owned())
        .spawn(move || rack_worker_loop(&mut audio_shared, &dsp_plugin, layout, &dsp_shutdown))
        .map_err(|error| error.to_string())?;
    let editor_header = shared_header(&editor_shared)?;
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
    editor_broker_loop(&editor_shared, &plugin, locator.display_name(), &shutdown);
    shutdown.store(true, Ordering::Release);
    let _ = dsp_thread.join();
    Ok(())
}

fn rack_worker_loop(
    shared: &mut MmapMut,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    layout: AudioLayout,
    parent_closed: &AtomicBool,
) {
    loop {
        let header = shared_header_unchecked(shared);
        if parent_closed.load(Ordering::Acquire) || header.shutdown.load(Ordering::Acquire) != 0 {
            return;
        }
        let sequence = header.input_sequence.load(Ordering::Acquire);
        let parameter_pending = header.parameters_pending.load(Ordering::Acquire);
        let audio_pending = sequence != 0
            && sequence != header.input_consumed.load(Ordering::Relaxed)
            && header.output_sequence.load(Ordering::Acquire)
                == header.output_consumed.load(Ordering::Relaxed);
        if !parameter_pending && !audio_pending {
            thread::sleep(Duration::from_micros(100));
            continue;
        }
        let Ok(mut plugin) = plugin.try_lock() else {
            thread::yield_now();
            continue;
        };
        apply_pending_rack_parameter(shared, &mut plugin);
        if sequence == 0
            || sequence == header.input_consumed.load(Ordering::Relaxed)
            || header.output_sequence.load(Ordering::Acquire)
                != header.output_consumed.load(Ordering::Relaxed)
        {
            continue;
        }
        copy_shared_input_to_rack(shared, &mut plugin, layout);
        shared_header_unchecked(shared)
            .input_consumed
            .store(sequence, Ordering::Release);
        plugin.clear_outputs();
        if plugin.process(layout.block_frames).is_err() {
            shared_header_unchecked(shared)
                .process_failures
                .fetch_add(1, Ordering::Relaxed);
            bypass_rack(&mut plugin, layout);
        }
        copy_rack_output_to_shared(shared, &plugin, layout);
        shared_header_unchecked(shared)
            .output_sequence
            .store(sequence, Ordering::Release);
    }
}

fn editor_broker_loop(
    shared: &Arc<MmapMut>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    parent_closed: &AtomicBool,
) {
    let mut host: Option<native_editor::NativeEditorHost> = None;
    let mut last_idle = Instant::now();
    loop {
        let header = shared_header_unchecked(shared);
        if parent_closed.load(Ordering::Acquire) || header.shutdown.load(Ordering::Acquire) != 0 {
            if let (Some(host), Ok(mut plugin)) = (host.as_mut(), plugin.lock()) {
                host.close(&mut plugin);
            }
            return;
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

fn apply_pending_state_request(shared: &MmapMut, plugin: &Mutex<rack_formats::RackWorkerPlugin>) {
    let header = shared_header_unchecked(shared);
    let request = header.state_request.load(Ordering::Acquire);
    if request == header.state_response.load(Ordering::Relaxed) {
        return;
    }
    if header.state_kind.load(Ordering::Relaxed) == editor_surface::EDITOR_FRAME {
        if !header.editor_observed_open.load(Ordering::Acquire) {
            publish_state_response(shared, request, Err(ProcessorStateError::Unavailable));
        }
        return;
    }
    let Ok(mut plugin) = plugin.try_lock() else {
        return;
    };
    // Flush accepted control edits before capturing the same native instance. A concurrent
    // later edit belongs to the next snapshot, not a stale parameter overlay on this state.
    apply_pending_rack_parameter(shared, &mut plugin);
    publish_state_response(shared, request, plugin.save_state());
}

fn publish_state_response(
    shared: &MmapMut,
    request: u64,
    state: Result<Vec<u8>, ProcessorStateError>,
) {
    let header = shared_header_unchecked(shared);
    let (status, bytes) = match state {
        Ok(bytes) if bytes.len() <= STATE_CAPACITY => (STATE_OK, bytes),
        Ok(_) => (STATE_TOO_LARGE, Vec::new()),
        Err(ProcessorStateError::Unsupported) => (STATE_UNSUPPORTED, Vec::new()),
        Err(_) => (STATE_FAILED, Vec::new()),
    };
    // SAFETY: only the worker broker writes the state region. The parent reads it only after
    // the release response and cannot publish another request until that read has finished.
    // Timed-out requests retain ownership until acknowledged; they are never overwritten.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            shared.as_ptr().add(size_of::<SharedHeader>()).cast_mut(),
            bytes.len(),
        );
    }
    header
        .state_length
        .store(u32::try_from(bytes.len()).unwrap(), Ordering::Relaxed);
    header.state_result.store(status, Ordering::Relaxed);
    header.state_response.store(request, Ordering::Release);
}

fn apply_pending_editor_command(
    shared: &Arc<MmapMut>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let header = shared_header_unchecked(shared);
    let sequence = header.editor_sequence.load(Ordering::Acquire);
    if sequence == header.editor_consumed.load(Ordering::Relaxed) {
        return;
    }
    let command = header.editor_command.load(Ordering::Relaxed);
    if command == EDITOR_COMMAND_CLOSE {
        let snapshot = if let Some(host) = host.as_mut() {
            let Ok(mut plugin) = plugin.lock() else {
                return;
            };
            host.close(&mut plugin);
            let snapshot = plugin.tick_editor();
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
    shared: &Arc<MmapMut>,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    title: &str,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let header = shared_header_unchecked(shared);
    let has_editor = plugin.lock().is_ok_and(|plugin| plugin.has_editor());
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
    let result = plugin.lock().map_or_else(
        |_| Err(native_editor::EditorHostFailure::PluginRejected),
        |mut plugin| {
            host.as_mut()
                .expect("editor host was initialized")
                .open(&mut plugin, title)
        },
    );
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
    shared: &MmapMut,
    plugin: &Mutex<rack_formats::RackWorkerPlugin>,
    host: &mut Option<native_editor::NativeEditorHost>,
) {
    let Some(editor_host) = host.as_mut() else {
        return;
    };
    // The host retains close/resize events until the plug-in can consume them. Native
    // mouse tracking can run a nested event loop; pumping it under this mutex stalls DSP.
    editor_host.poll_events();
    let Ok(mut plugin) = plugin.try_lock() else {
        return;
    };
    let events = editor_host.take_events();
    if events.close_requested {
        editor_host.close(&mut plugin);
    } else if let Some((width, height)) = events.resized {
        let _ = plugin.resize_editor(width, height);
    }
    let snapshot = plugin.tick_editor();
    editor_host.refresh(snapshot);
    let header = shared_header_unchecked(shared);
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

fn apply_pending_rack_parameter(shared: &MmapMut, plugin: &mut rack_formats::RackWorkerPlugin) {
    let header = shared_header_unchecked(shared);
    consume_parameters(header, |id, value| {
        if plugin.set_parameter(id, value).is_err() {
            header.process_failures.fetch_add(1, Ordering::Relaxed);
        }
    });
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
struct AudioLayout {
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
        let entry = self.catalog.entry(config.type_id.as_str()).ok_or_else(|| {
            NodeResolutionError::CapabilityUnavailable {
                reason: format!("processor is not installed: {}", config.type_id),
            }
        })?;
        let input_channels = entry.descriptor.input_channels;
        let output_channels = entry.descriptor.output_channels;
        if input_channels == 0
            || output_channels == 0
            || input_channels > context.limits.max_channels_per_port
            || output_channels > context.limits.max_channels_per_port
        {
            return Err(NodeResolutionError::CapabilityUnavailable {
                reason: format!(
                    "processor channel layout {input_channels} in / {output_channels} out is unsupported"
                ),
            });
        }
        Ok(ResolvedNode {
            inputs: vec![CompiledPort::new(
                PortId::from(AUDIO_PORT_ID),
                PortDirection::Input,
                input_channels,
            )],
            outputs: vec![CompiledPort::new(
                PortId::from(AUDIO_PORT_ID),
                PortDirection::Output,
                output_channels,
            )],
        })
    }
}

#[derive(Clone)]
pub struct ExternalProcessorFactory {
    catalog: ProcessorCatalog,
    controls: ProcessorControlRegistry,
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
    fn prepare(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String> {
        let entry = self
            .catalog
            .entry(request.config.type_id.as_str())
            .ok_or_else(|| format!("processor is not installed: {}", request.config.type_id))?;
        if request.input_channels != entry.descriptor.input_channels
            || request.output_channels != entry.descriptor.output_channels
        {
            return Err("compiled plug-in layout does not match the catalog".to_owned());
        }
        if let Some(processor) = self.controls.reuse_request(&request) {
            return Ok(processor);
        }
        let mut processor = HostedExternalProcessor::new(&entry, request.config, request.format)?;
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
            Self::TooLarge => "processor state exceeds 4 MiB",
            Self::InvalidResponse => "processor state response is invalid",
        })
    }
}

impl std::error::Error for ProcessorStateError {}

impl ProcessorControlRegistry {
    /// Captures native state on the worker's editor/control thread. Call only from a control
    /// thread, before saving or replacing a graph. Native state is authoritative: replace the
    /// provider's parameter object with this snapshot, rather than overlaying stale values.
    ///
    /// # Errors
    /// Returns an error for inactive/crashed workers, concurrent or timed-out requests,
    /// unsupported native state APIs, or exports exceeding the 4 MiB transport limit.
    pub fn snapshot_state(
        &self,
        node_id: &NodeId,
        timeout: Duration,
    ) -> Result<serde_json::Value, ProcessorStateError> {
        self.live_control(node_id)
            .map_err(|_| ProcessorStateError::Unavailable)?
            .snapshot_state(timeout)
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
    shared: MmapMut,
    writer: Mutex<()>,
    input_writer: Mutex<()>,
    reuse: Mutex<Option<reuse::ReuseSignature>>,
    active_owner: AtomicBool,
}

impl HostedProcessorControl {
    fn snapshot_state(&self, timeout: Duration) -> Result<serde_json::Value, ProcessorStateError> {
        let header = shared_header_unchecked(&self.shared);
        header
            .editor_input
            .cancel(header.editor_sequence.load(Ordering::Acquire));
        let deadline = Instant::now() + timeout.min(Duration::from_secs(5));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.request_blob(editor_surface::NATIVE_STATE, remaining) {
                Ok(bytes) => {
                    let state = serde_json::json!({ "stateBase64": BASE64.encode(bytes) });
                    if let Some(signature) = self
                        .reuse
                        .lock()
                        .map_err(|_| ProcessorStateError::Unavailable)?
                        .as_mut()
                    {
                        signature.checkpoint(&state);
                    }
                    return Ok(state);
                }
                Err(ProcessorStateError::Busy | ProcessorStateError::Pending)
                    if !remaining.is_zero()
                        && header.state_kind.load(Ordering::Acquire)
                            == editor_surface::EDITOR_FRAME =>
                {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn request_blob(&self, kind: u32, timeout: Duration) -> Result<Vec<u8>, ProcessorStateError> {
        // Do not queue unbounded callers behind a slow vendor. This mutex is control-only;
        // the worker and all audio paths exclusively use atomics and never acquire it.
        let _writer = self
            .writer
            .try_lock()
            .map_err(|_| ProcessorStateError::Busy)?;
        let header = shared_header_unchecked(&self.shared);
        if header.worker_status.load(Ordering::Acquire) != WORKER_READY
            || header.shutdown.load(Ordering::Acquire) != 0
        {
            return Err(ProcessorStateError::Unavailable);
        }
        let previous = header.state_request.load(Ordering::Relaxed);
        if previous != header.state_response.load(Ordering::Acquire) {
            return Err(ProcessorStateError::Pending);
        }
        let request = previous.wrapping_add(1);
        header.state_kind.store(kind, Ordering::Relaxed);
        header.state_request.store(request, Ordering::Release);
        let deadline = Instant::now() + timeout.min(Duration::from_secs(5));
        loop {
            if header.state_response.load(Ordering::Acquire) == request {
                break;
            }
            if header.worker_status.load(Ordering::Acquire) != WORKER_READY
                || header.shutdown.load(Ordering::Acquire) != 0
            {
                return Err(ProcessorStateError::Unavailable);
            }
            if Instant::now() >= deadline {
                return Err(ProcessorStateError::Timeout);
            }
            thread::sleep(Duration::from_millis(1));
        }
        match header.state_result.load(Ordering::Relaxed) {
            STATE_OK => {}
            STATE_TOO_LARGE => return Err(ProcessorStateError::TooLarge),
            STATE_UNSUPPORTED => return Err(ProcessorStateError::Unsupported),
            _ => return Err(ProcessorStateError::NativeFailure),
        }
        let length = header.state_length.load(Ordering::Relaxed) as usize;
        if length > STATE_CAPACITY {
            return Err(ProcessorStateError::InvalidResponse);
        }
        // SAFETY: release/acquire response grants exclusive read ownership of this bounded
        // region until this control publisher releases the mutex and sends a later request.
        let bytes = unsafe {
            std::slice::from_raw_parts(self.shared.as_ptr().add(size_of::<SharedHeader>()), length)
        };
        Ok(bytes.to_vec())
    }

    fn set_parameter(&self, id: u32, value: f64) -> Result<(), String> {
        // HTTP/Tauri requests may arrive concurrently. Serializing publishers prevents a
        // parameter ID from being paired with another request's value; the worker never locks.
        let _writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let header = shared_header_unchecked(&self.shared);
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
        let _writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let header = shared_header_unchecked(&self.shared);
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
        let header = shared_header_unchecked(&self.shared);
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

struct HostedExternalProcessor {
    child_stdin: Option<ChildStdin>,
    worker_monitor: Option<WorkerMonitor>,
    shared: Option<MmapMut>,
    shared_path: PathBuf,
    config_path: PathBuf,
    layout: AudioLayout,
    next_sequence: u64,
    control: Option<Arc<HostedProcessorControl>>,
    registration: Option<ProcessorControlRegistration>,
}

impl HostedExternalProcessor {
    fn new(
        entry: &CatalogEntry,
        config: &ProviderNodeConfig,
        format: RenderFormat,
    ) -> Result<Self, String> {
        let input_channels = usize::from(entry.descriptor.input_channels);
        let output_channels = usize::from(entry.descriptor.output_channels);
        let block_frames = format.block_frames as usize;
        let (shared_path, config_path) = worker_paths().map_err(|error| error.to_string())?;
        let shared_file =
            create_shared_file(&shared_path, input_channels, output_channels, block_frames)?;
        if let Err(error) =
            write_worker_config(&config_path, format.sample_rate, &config.parameters)
        {
            // Windows cannot unlink an open file, so release the handle before rollback.
            drop(shared_file);
            cleanup_worker_paths(&shared_path, &config_path);
            return Err(error);
        }
        let (shared, control) = match map_worker_regions(&shared_file) {
            Ok(regions) => regions,
            Err(error) => {
                drop(shared_file);
                cleanup_worker_paths(&shared_path, &config_path);
                return Err(error);
            }
        };
        // Both mappings retain their kernel objects; keeping this handle open would only delay
        // Windows cleanup when the processor is removed.
        drop(shared_file);
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
        Ok(Self {
            child_stdin,
            worker_monitor: Some(worker_monitor),
            shared: Some(shared),
            shared_path,
            config_path,
            layout: AudioLayout {
                inputs: input_channels,
                outputs: output_channels,
                block_frames,
            },
            next_sequence: 1,
            control: Some(control),
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
        .arg(entry.rack.locator.encode()?)
        .arg(entry.rack.layout_index.to_string());
    command
        .arg(shared_path)
        .arg(config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.spawn().map_err(|error| error.to_string())
}

fn map_worker_regions(file: &File) -> Result<(MmapMut, Arc<HostedProcessorControl>), String> {
    // SAFETY: the private transport file is fixed-size and retained for both mappings' lifetime.
    let audio = unsafe { MmapOptions::new().map_mut(file) }.map_err(|error| error.to_string())?;
    // A distinct mapping lets the control plane publish atomics without sharing mutable sample
    // buffer ownership with the audio thread.
    let control = unsafe { MmapOptions::new().map_mut(file) }.map_err(|error| error.to_string())?;
    Ok((
        audio,
        Arc::new(HostedProcessorControl {
            shared: control,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        }),
    ))
}

impl HostedAudioProcessor for HostedExternalProcessor {
    fn set_active(&mut self, active: bool) {
        if let Some(control) = &self.control {
            control.active_owner.store(active, Ordering::Release);
        }
    }
    fn reuse_identity(&self) -> Option<u64> {
        self.control
            .as_ref()
            .map(|control| shared_header_unchecked(&control.shared).instance_id)
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool {
        self.set_active(true);
        if frames > self.layout.block_frames
            || input.len() < frames.saturating_mul(self.layout.inputs)
            || output.len() < frames.saturating_mul(self.layout.outputs)
        {
            return false;
        }
        let shared = self
            .shared
            .as_mut()
            .expect("hosted processor retains audio mapping until drop");
        let expected_output = self.next_sequence.saturating_sub(1);
        let published_output = shared_header_unchecked(shared)
            .output_sequence
            .load(Ordering::Acquire);
        let has_output = expected_output != 0 && published_output == expected_output;
        if has_output {
            copy_shared_output_to_interleaved(shared, output, self.layout, frames);
            shared_header_unchecked(shared)
                .output_consumed
                .store(published_output, Ordering::Release);
        }
        if shared_header_unchecked(shared)
            .input_sequence
            .load(Ordering::Acquire)
            == shared_header_unchecked(shared)
                .input_consumed
                .load(Ordering::Relaxed)
        {
            copy_interleaved_input_to_shared(shared, input, self.layout, frames);
            shared_header_unchecked(shared)
                .input_sequence
                .store(self.next_sequence, Ordering::Release);
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        if !has_output {
            // The worker missed the one-block deadline. Return `false` so the graph performs
            // its deterministic channel-preserving bypass for this block.
            return false;
        }
        true
    }
}

impl Drop for HostedExternalProcessor {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared_header_unchecked(shared)
                .shutdown
                .store(1, Ordering::Release);
        }
        // Closing stdin also makes an orphaned worker exit if the mapping is no longer polled.
        drop(self.child_stdin.take());
        if let Some(mut monitor) = self.worker_monitor.take() {
            monitor.stop_and_join();
        }
        // Windows refuses to unlink mapped files. Drop every mapping and weak registration
        // before removing the transport paths; Unix follows the same order for consistency.
        drop(self.registration.take());
        drop(self.control.take());
        drop(self.shared.take());
        cleanup_worker_paths(&self.shared_path, &self.config_path);
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
            .name("tonenari-processor-monitor".to_owned())
            .spawn(move || {
                let mut child = thread_child
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .expect("worker child is transferred exactly once");
                let mut stop_deadline = None;
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            if !thread_stop.load(Ordering::Acquire) {
                                shared_header_unchecked(&control.shared)
                                    .worker_status
                                    .store(WORKER_FAILED, Ordering::Release);
                            }
                            return;
                        }
                        Err(_) => {
                            shared_header_unchecked(&control.shared)
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
    let _ = std::fs::remove_file(config_path);
}

fn create_shared_file(
    path: &Path,
    input_channels: usize,
    output_channels: usize,
    block_frames: usize,
) -> Result<File, String> {
    let input_channels_u32 = u32::try_from(input_channels)
        .map_err(|_| "VST3 input channel count is too large".to_owned())?;
    let output_channels_u32 = u32::try_from(output_channels)
        .map_err(|_| "VST3 output channel count is too large".to_owned())?;
    let block_frames_u32 =
        u32::try_from(block_frames).map_err(|_| "VST3 block size is too large".to_owned())?;
    let length = shared_length(input_channels, output_channels, block_frames)?;
    let file_length =
        u64::try_from(length).map_err(|_| "shared processor buffer is too large".to_owned())?;
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    if let Err(error) = file.set_len(file_length) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    // SAFETY: the newly created file is exclusively owned and large enough for the header.
    let mut mapping = match unsafe { MmapOptions::new().map_mut(&file) } {
        Ok(mapping) => mapping,
        Err(error) => {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(error.to_string());
        }
    };
    let header = SharedHeader {
        magic: SHARED_MAGIC,
        version: SHARED_VERSION,
        input_channels: input_channels_u32,
        output_channels: output_channels_u32,
        block_frames: block_frames_u32,
        worker_status: AtomicU32::new(WORKER_STARTING),
        shutdown: AtomicU32::new(0),
        input_sequence: AtomicU64::new(0),
        input_consumed: AtomicU64::new(0),
        output_sequence: AtomicU64::new(0),
        output_consumed: AtomicU64::new(0),
        process_failures: AtomicU64::new(0),
        parameter_count: AtomicU32::new(0),
        parameters_pending: AtomicBool::new(false),
        parameters: std::array::from_fn(|_| ParameterSlot::empty()),
        state_request: AtomicU64::new(0),
        state_kind: AtomicU32::new(editor_surface::NATIVE_STATE),
        state_response: AtomicU64::new(0),
        state_length: AtomicU32::new(0),
        state_result: AtomicU32::new(STATE_OK),
        editor_command: AtomicU32::new(EDITOR_COMMAND_NONE),
        editor_sequence: AtomicU64::new(0),
        editor_consumed: AtomicU64::new(0),
        editor_state: AtomicU32::new(EDITOR_STATE_UNAVAILABLE),
        editor_failure: AtomicU32::new(EDITOR_FAILURE_NONE),
        editor_width: AtomicU32::new(0),
        editor_height: AtomicU32::new(0),
        editor_resizable: AtomicU32::new(0),
        instance_id: NEXT_CONTROL_INSTANCE.fetch_add(1, Ordering::Relaxed),
        editor_observed_open: AtomicBool::new(false),
        editor_close_generation: AtomicU64::new(0),
        editor_input: editor_surface::InputRing::new(),
    };
    // SAFETY: OS mappings begin on a page boundary, whose alignment exceeds the 64-byte header
    // requirement, and the mapping has at least `SharedHeader` bytes.
    #[allow(clippy::cast_ptr_alignment)]
    unsafe {
        mapping.as_mut_ptr().cast::<SharedHeader>().write(header);
    };
    if let Err(error) = mapping.flush() {
        drop(mapping);
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(file)
}

fn shared_length(inputs: usize, outputs: usize, frames: usize) -> Result<usize, String> {
    let samples = inputs
        .checked_add(outputs)
        .and_then(|channels| channels.checked_mul(frames))
        .ok_or_else(|| "shared processor sample count overflowed".to_owned())?;
    input_offset()
        .checked_add(
            samples
                .checked_mul(size_of::<f32>())
                .ok_or_else(|| "shared processor byte count overflowed".to_owned())?,
        )
        .ok_or_else(|| "shared processor mapping size overflowed".to_owned())
}

fn shared_header(mapping: &MmapMut) -> Result<&SharedHeader, String> {
    if mapping.len() < size_of::<SharedHeader>() {
        return Err("shared processor mapping is truncated".to_owned());
    }
    let header = shared_header_unchecked(mapping);
    if header.magic != SHARED_MAGIC || header.version != SHARED_VERSION {
        return Err("shared processor mapping has an incompatible header".to_owned());
    }
    let expected = shared_length(
        header.input_channels as usize,
        header.output_channels as usize,
        header.block_frames as usize,
    )?;
    if mapping.len() != expected {
        return Err("shared processor mapping has an invalid size".to_owned());
    }
    Ok(header)
}

fn shared_header_unchecked(mapping: &MmapMut) -> &SharedHeader {
    // SAFETY: every caller owns a mapping validated or created by this module. Page-aligned mmap
    // storage satisfies the header's alignment, and the header lives for the mapping lifetime.
    #[allow(clippy::cast_ptr_alignment)]
    unsafe {
        &*mapping.as_ptr().cast::<SharedHeader>()
    }
}

fn input_offset() -> usize {
    size_of::<SharedHeader>() + STATE_CAPACITY
}

fn output_offset(inputs: usize, frames: usize) -> usize {
    input_offset() + inputs * frames * size_of::<f32>()
}

fn write_worker_config(
    path: &Path,
    sample_rate: u32,
    parameters: &serde_json::Value,
) -> Result<(), String> {
    let body = serde_json::json!({
        "sampleRate": sample_rate,
        "parameters": parameters,
    });
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(body.to_string().as_bytes())
        .map_err(|error| error.to_string())
}

fn read_worker_configuration(path: &str) -> Result<(f64, serde_json::Value), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let config: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let sample_rate = config
        .get("sampleRate")
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| "VST3 worker config has an invalid sample rate".to_owned())?;
    let parameters = config
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Ok((sample_rate, parameters))
}

fn wait_for_worker(child: &mut Child, shared: &MmapMut) -> Result<(), String> {
    let deadline = Instant::now() + WORKER_START_TIMEOUT;
    loop {
        match shared_header(shared)?.worker_status.load(Ordering::Acquire) {
            WORKER_READY => return Ok(()),
            WORKER_FAILED => return Err("VST3 worker initialization failed".to_owned()),
            _ => {}
        }
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("VST3 worker exited during initialization".to_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("VST3 worker initialization timed out".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn copy_interleaved_input_to_shared(
    shared: &mut MmapMut,
    input: &[f32],
    layout: AudioLayout,
    frames: usize,
) {
    let sample_capacity = layout.inputs * layout.block_frames;
    // SAFETY: the mapping size was validated from these dimensions and the producer owns the
    // input region until it publishes `input_sequence`.
    #[allow(clippy::cast_ptr_alignment)]
    let destination = unsafe {
        std::slice::from_raw_parts_mut(
            shared.as_mut_ptr().add(input_offset()).cast::<f32>(),
            sample_capacity,
        )
    };
    destination.fill(0.0);
    destination[..layout.inputs * frames].copy_from_slice(&input[..layout.inputs * frames]);
}

fn copy_shared_input_to_rack(
    shared: &MmapMut,
    plugin: &mut rack_formats::RackWorkerPlugin,
    layout: AudioLayout,
) {
    // SAFETY: the worker owns the published input region until it releases `input_consumed`.
    #[allow(clippy::cast_ptr_alignment)]
    let source = unsafe {
        std::slice::from_raw_parts(
            shared.as_ptr().add(input_offset()).cast::<f32>(),
            layout.inputs * layout.block_frames,
        )
    };
    for (channel_index, channel) in plugin.inputs_mut().iter_mut().enumerate() {
        for frame in 0..layout.block_frames {
            channel[frame] = source[frame * layout.inputs + channel_index];
        }
    }
}

fn copy_rack_output_to_shared(
    shared: &mut MmapMut,
    plugin: &rack_formats::RackWorkerPlugin,
    layout: AudioLayout,
) {
    // SAFETY: the worker exclusively owns the output region until `output_sequence` is released.
    #[allow(clippy::cast_ptr_alignment)]
    let destination = unsafe {
        std::slice::from_raw_parts_mut(
            shared
                .as_mut_ptr()
                .add(output_offset(layout.inputs, layout.block_frames))
                .cast::<f32>(),
            layout.outputs * layout.block_frames,
        )
    };
    for frame in 0..layout.block_frames {
        for (channel_index, channel) in plugin.outputs().iter().enumerate() {
            destination[frame * layout.outputs + channel_index] = channel[frame];
        }
    }
}

fn copy_shared_output_to_interleaved(
    shared: &MmapMut,
    output: &mut [f32],
    layout: AudioLayout,
    frames: usize,
) {
    // SAFETY: acquire of `output_sequence` happened before this call and prevents the worker from
    // reusing the output region until `output_consumed` is released.
    #[allow(clippy::cast_ptr_alignment)]
    let source = unsafe {
        std::slice::from_raw_parts(
            shared
                .as_ptr()
                .add(output_offset(layout.inputs, layout.block_frames))
                .cast::<f32>(),
            layout.outputs * layout.block_frames,
        )
    };
    output[..layout.outputs * frames].copy_from_slice(&source[..layout.outputs * frames]);
}

fn bypass_rack(plugin: &mut rack_formats::RackWorkerPlugin, layout: AudioLayout) {
    plugin.bypass(layout.block_frames);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_mapping(inputs: usize, outputs: usize, frames: usize) -> (PathBuf, PathBuf, MmapMut) {
        let (shared_path, config_path) = worker_paths().expect("private worker paths");
        let file = create_shared_file(&shared_path, inputs, outputs, frames)
            .expect("shared transport should be created");
        // SAFETY: `create_shared_file` fixed the mapping size for this test.
        let mapping =
            unsafe { MmapOptions::new().map_mut(&file) }.expect("shared transport should map");
        (shared_path, config_path, mapping)
    }

    #[test]
    fn stalled_or_failed_worker_does_not_block_an_independent_processor() {
        let mut unavailable = test_processor();
        let mut healthy = test_processor();
        let mut output = [0.0; 8];
        let input = [0.25; 8];
        assert!(!unavailable.process(&input, &mut output, 4));
        assert!(!healthy.process(&input, &mut output, 4));
        for sequence in 1..=64 {
            let shared = healthy.shared.as_mut().unwrap();
            let offset = output_offset(2, 4);
            for bytes in shared[offset..offset + 8 * size_of::<f32>()].chunks_exact_mut(4) {
                bytes.copy_from_slice(&0.5_f32.to_ne_bytes());
            }
            let header = shared_header_unchecked(shared);
            header.input_consumed.store(sequence, Ordering::Release);
            header.output_sequence.store(sequence, Ordering::Release);
            if sequence == 32 {
                shared_header_unchecked(unavailable.shared.as_ref().unwrap())
                    .worker_status
                    .store(WORKER_FAILED, Ordering::Release);
            }
            assert!(!unavailable.process(&input, &mut output, 4));
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

    fn test_processor() -> HostedExternalProcessor {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        HostedExternalProcessor {
            child_stdin: None,
            worker_monitor: None,
            shared: Some(shared),
            shared_path,
            config_path,
            layout: AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 4,
            },
            next_sequence: 1,
            control: None,
            registration: None,
        }
    }

    #[test]
    fn shared_audio_regions_round_trip_without_growing() {
        let (shared_path, config_path, mut shared) = shared_mapping(2, 2, 4);
        let input = [1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0];
        let layout = AudioLayout {
            inputs: 2,
            outputs: 2,
            block_frames: 4,
        };
        copy_interleaved_input_to_shared(&mut shared, &input, layout, 4);
        // SAFETY: this test owns the worker-side output region of the private mapping.
        #[allow(clippy::cast_ptr_alignment)]
        let worker_output = unsafe {
            std::slice::from_raw_parts_mut(
                shared
                    .as_mut_ptr()
                    .add(output_offset(layout.inputs, layout.block_frames))
                    .cast::<f32>(),
                layout.outputs * layout.block_frames,
            )
        };
        worker_output.copy_from_slice(&[5.0, 50.0, 6.0, 60.0, 7.0, 70.0, 8.0, 80.0]);
        let mut output = [0.0; 8];
        copy_shared_output_to_interleaved(&shared, &mut output, layout, 4);
        for (actual, expected) in output
            .iter()
            .zip([5.0, 50.0, 6.0, 60.0, 7.0, 70.0, 8.0, 80.0])
        {
            assert!((actual - expected).abs() <= f32::EPSILON);
        }
        assert_eq!(shared.len(), shared_length(2, 2, 4).unwrap());

        drop(shared);
        cleanup_worker_paths(&shared_path, &config_path);
        assert!(!shared_path.exists());
    }

    #[test]
    fn control_mailboxes_coalesce_and_registration_does_not_extend_lifetime() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl {
            shared,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        });
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
        let header = shared_header_unchecked(&control.shared);
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
        let control = HostedProcessorControl {
            shared,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        };
        for id in 0..u32::try_from(PARAMETER_CAPACITY).unwrap() {
            control.set_parameter(id, f64::from(id) / 4096.0).unwrap();
        }
        assert!(control.set_parameter(u32::MAX, 0.5).is_err());
        control.set_parameter(0, 0.75).unwrap();
        let mut count = 0;
        consume_parameters(shared_header_unchecked(&control.shared), |id, value| {
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
            let control = Arc::new(HostedProcessorControl {
                shared,
                writer: Mutex::new(()),
                input_writer: Mutex::new(()),
                reuse: Mutex::new(None),
                active_owner: AtomicBool::new(false),
            });
            shared_header_unchecked(&control.shared)
                .worker_status
                .store(WORKER_READY, Ordering::Release);
            let worker_control = Arc::clone(&control);
            let worker = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                let header = shared_header_unchecked(&worker_control.shared);
                while header.state_request.load(Ordering::Acquire) == 0 {
                    assert!(Instant::now() < deadline);
                    thread::yield_now();
                }
                publish_state_response(&worker_control.shared, 1, state);
            });
            let result = control.snapshot_state(Duration::from_secs(2));
            worker.join().unwrap();
            if let Some(message) = expected_error {
                assert_eq!(result.unwrap_err(), message);
            } else {
                let saved = result.unwrap();
                let bytes = BASE64
                    .decode(saved["stateBase64"].as_str().unwrap())
                    .unwrap();
                assert_eq!(bytes, vec![0, 1, 255, 13, 0]);
                assert!(saved.get("parameterValues").is_none());
            }
            drop(control);
            cleanup_worker_paths(&shared_path, &config_path);
        }
    }

    #[test]
    fn concurrent_parameter_edits_never_pair_a_value_with_another_id() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl {
            shared,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        });
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
            consume_parameters(shared_header_unchecked(&control.shared), |id, value| {
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
        let control = HostedProcessorControl {
            shared,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        };
        let header = shared_header_unchecked(&control.shared);
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
        assert_eq!(header.state_request.load(Ordering::Acquire), 1);
        publish_state_response(&control.shared, 1, Ok(vec![8]));
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Timeout)
        );
        assert_eq!(header.state_request.load(Ordering::Acquire), 2);
        header.worker_status.store(WORKER_FAILED, Ordering::Release);
        assert!(
            control
                .snapshot_state(Duration::ZERO)
                .unwrap_err()
                .eq(&ProcessorStateError::Unavailable)
        );
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn monitor_reaps_a_crashed_worker_and_marks_transport_failed() {
        let (shared_path, config_path, shared) = shared_mapping(2, 2, 4);
        let control = Arc::new(HostedProcessorControl {
            shared,
            writer: Mutex::new(()),
            input_writer: Mutex::new(()),
            reuse: Mutex::new(None),
            active_owner: AtomicBool::new(false),
        });
        #[cfg(unix)]
        let mut command = Command::new("/usr/bin/false");
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd.exe");
            // Disable AutoRun so the fixture exits without executing user shell setup.
            command.args(["/d", "/c", "exit", "1"]);
            command
        };
        let child = command.spawn().expect("fixture process should spawn");
        let mut monitor = WorkerMonitor::spawn(child, Arc::clone(&control))
            .map_err(|(error, _)| error)
            .expect("monitor should spawn");
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared_header_unchecked(&control.shared)
            .worker_status
            .load(Ordering::Acquire)
            != WORKER_FAILED
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            shared_header_unchecked(&control.shared)
                .worker_status
                .load(Ordering::Acquire),
            WORKER_FAILED
        );
        monitor.stop_and_join();
        drop(control);
        cleanup_worker_paths(&shared_path, &config_path);
    }
}
