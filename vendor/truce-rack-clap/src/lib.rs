//! CLAP host implementation for the truce-rack framework.
//!
//! CLAP is the simplest format to host because the entire API is
//! pure C with no platform-specific oddities. truce-rack-clap uses
//! `clap-sys` for raw bindings and `libloading` to open plugin
//! bundles at runtime — no C++ glue is involved.
//!
//! # Lifecycle
//!
//! Each `.clap` bundle ships a single `clap_entry` symbol. The
//! scanner opens the bundle, calls `entry.init(path)`, asks the
//! factory at `CLAP_PLUGIN_FACTORY_ID` to enumerate its plugins,
//! converts each `clap_plugin_descriptor` into a [`PluginInfo`]
//! and then calls `entry.deinit()` to release scanner-only state.
//!
//! [`ClapScanner::load`] re-opens the bundle, holds the entry +
//! library alive for the lifetime of the returned [`ClapPlugin`],
//! and on `Drop` calls the plugin's `destroy` followed by the
//! entry's `deinit`.

use truce_rack_core::buffer::AudioBuffer;
use truce_rack_core::bus::BusLayout;
use truce_rack_core::error::{Error, Result};
use truce_rack_core::events::EventList;
use truce_rack_core::info::{ParameterInfo, PluginCategory, PluginInfo, PresetInfo};
use truce_rack_core::plugin::{
    MainThreadUpdate, Plugin, PluginCore, ProcessContext, ProcessStatus,
};
use truce_rack_core::scanner::PluginScanner;
use truce_rack_core::transport::TransportInfo;
use truce_rack_core::wrapper::run_audio_block_with;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
    CLAP_EVENT_PARAM_VALUE, CLAP_EVENT_TRANSPORT, CLAP_TRANSPORT_HAS_BEATS_TIMELINE,
    CLAP_TRANSPORT_HAS_SECONDS_TIMELINE, CLAP_TRANSPORT_HAS_TEMPO,
    CLAP_TRANSPORT_HAS_TIME_SIGNATURE, CLAP_TRANSPORT_IS_LOOP_ACTIVE, CLAP_TRANSPORT_IS_PLAYING,
    CLAP_TRANSPORT_IS_RECORDING, clap_event_header, clap_event_midi, clap_event_note,
    clap_event_param_value, clap_event_transport, clap_input_events, clap_output_events,
    clap_transport_flags,
};
use clap_sys::ext::audio_ports::{CLAP_EXT_AUDIO_PORTS, clap_plugin_audio_ports};
use clap_sys::ext::audio_ports_config::{
    CLAP_EXT_AUDIO_PORTS_CONFIG, CLAP_EXT_AUDIO_PORTS_CONFIG_INFO, clap_plugin_audio_ports_config,
    clap_plugin_audio_ports_config_info,
};
use clap_sys::ext::gui::{
    CLAP_EXT_GUI, CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11,
    clap_plugin_gui, clap_window, clap_window_handle,
};
use clap_sys::ext::latency::{CLAP_EXT_LATENCY, clap_plugin_latency};
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_BYPASS, CLAP_PARAM_IS_ENUM,
    CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_READONLY, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS, clap_param_info, clap_plugin_params,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::ext::timer_support::{CLAP_EXT_TIMER_SUPPORT, clap_plugin_timer_support};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR,
    CLAP_PROCESS_SLEEP, CLAP_PROCESS_TAIL, clap_process,
};
use clap_sys::stream::{clap_istream, clap_ostream};

use std::ffi::{CStr, CString, OsStr, c_char};
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Instant;

mod audio_ports;
mod host;
mod parameter_ids;
#[cfg(test)]
mod scanner_tests;
mod timer_support;

use audio_ports::AudioPortCatalog;
use host::HostContext;
use parameter_ids::{ParameterIds, ParameterMetadata};

/// Format identifier — used as the `format` field on
/// [`PluginInfo`] entries this crate returns.
pub const FORMAT: &str = "clap";

/// Filename extension every CLAP plugin uses, including the
/// leading dot.
pub const CLAP_EXTENSION: &str = ".clap";

/// Symbol name `clap_entry` plugins must export.
const ENTRY_SYMBOL: &[u8] = b"clap_entry\0";

/// CLAP scanner.
///
/// Walks the standard CLAP plugin directories for the current OS
/// and returns one entry per discovered CLAP plugin. Calling
/// [`PluginScanner::scan_path`] lets a host scan a custom
/// directory or one exact `.clap` bundle (useful for sandboxed
/// test fixtures or per-application bundled plugins).
#[derive(Debug, Default)]
pub struct ClapScanner;

impl ClapScanner {
    /// Construct a default scanner. There's no config state; the
    /// type exists so consumers have a stable handle to scan
    /// from.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PluginScanner for ClapScanner {
    type Plugin = ClapPlugin;

    fn scan(&self) -> Result<Vec<PluginInfo>> {
        let mut out = Vec::new();
        for dir in default_clap_paths() {
            scan_optional_default_dir_into(&dir, &mut out)?;
        }
        Ok(out)
    }

    fn scan_path(&self, path: &Path) -> Result<Vec<PluginInfo>> {
        let mut out = Vec::new();
        let metadata = std::fs::metadata(path)
            .map_err(|error| scan_path_io_error("inspect explicit CLAP scan path", path, &error))?;

        if has_clap_extension(path) && (metadata.is_file() || metadata.is_dir()) {
            scan_bundle_candidate_into(path, &mut out)?;
        } else if metadata.is_dir() {
            scan_dir_into(path, &mut out)?;
        } else {
            return Err(Error::PluginNotFound(format!(
                "explicit CLAP scan path must be a .clap bundle or directory: {}",
                path.display()
            )));
        }
        Ok(out)
    }

    fn load(&self, info: &PluginInfo) -> Result<Self::Plugin> {
        ClapPlugin::load_from(info)
    }
}

/// Standard locations the CLAP spec lists for each OS. Mirrors
/// what the CLAP example host walks.
#[must_use]
pub fn default_clap_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let mut user = PathBuf::from(home);
        #[cfg(target_os = "macos")]
        user.push("Library/Audio/Plug-Ins/CLAP");
        #[cfg(target_os = "linux")]
        user.push(".clap");
        #[cfg(target_os = "windows")]
        user.push("AppData/Local/Programs/Common/CLAP");
        out.push(user);
    }
    #[cfg(target_os = "macos")]
    out.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    #[cfg(target_os = "linux")]
    out.push(PathBuf::from("/usr/lib/clap"));
    #[cfg(target_os = "windows")]
    {
        if let Some(pf) = std::env::var_os("CommonProgramFiles") {
            let mut p = PathBuf::from(pf);
            p.push("CLAP");
            out.push(p);
        }
    }
    out
}

fn scan_optional_default_dir_into(dir: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(scan_path_io_error(
                "read default CLAP directory",
                dir,
                &error,
            ));
        }
    };
    scan_directory_entries(dir, entries, out)
}

fn scan_dir_into(dir: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| scan_path_io_error("read explicit CLAP directory", dir, &error))?;
    scan_directory_entries(dir, entries, out)
}

fn scan_directory_entries(
    dir: &Path,
    entries: std::fs::ReadDir,
    out: &mut Vec<PluginInfo>,
) -> Result<()> {
    for entry in entries {
        let entry = entry
            .map_err(|error| scan_path_io_error("read entry in CLAP directory", dir, &error))?;
        let path = entry.path();
        if has_clap_extension(&path) {
            scan_bundle_candidate_into(&path, out)?;
        }
    }
    Ok(())
}

fn has_clap_extension(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("clap"))
}

fn scan_path_io_error(action: &str, path: &Path, error: &std::io::Error) -> Error {
    Error::Other(format!("failed to {action} {}: {error}", path.display()))
}

fn scan_bundle_candidate_into(bundle_path: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    scan_bundle_into(bundle_path, out).map_err(|error| Error::LoadFailed {
        path: bundle_path.to_path_buf(),
        reason: error.to_string(),
    })
}

fn scan_bundle_into(bundle_path: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let binary = bundle_binary_path(bundle_path);
    let handle = unsafe { LoadedLibrary::open(&binary)? };
    let entry = handle.entry()?;
    unsafe { entry.init(&binary)? };
    let factory = unsafe { entry.factory() };
    if !factory.is_null() {
        let count = unsafe { ((*factory).get_plugin_count.unwrap_or(empty_count))(factory) };
        for idx in 0..count {
            let desc =
                unsafe { ((*factory).get_plugin_descriptor.unwrap_or(empty_desc))(factory, idx) };
            if desc.is_null() {
                continue;
            }
            out.push(unsafe { descriptor_to_info(bundle_path, &*desc) });
        }
    }
    unsafe { entry.deinit() };
    Ok(())
}

unsafe extern "C" fn empty_count(_: *const clap_plugin_factory) -> u32 {
    0
}

unsafe extern "C" fn empty_desc(
    _: *const clap_plugin_factory,
    _: u32,
) -> *const clap_plugin_descriptor {
    ptr::null()
}

/// Per-platform `.clap` bundle layout. macOS uses NSBundle-style
/// `Contents/MacOS/<stem>`; Linux / Windows treat the `.clap`
/// file itself as the dylib.
fn bundle_binary_path(bundle: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let stem = bundle.file_stem().unwrap_or_default();
        if bundle.is_dir() {
            return bundle.join("Contents/MacOS").join(stem);
        }
    }
    bundle.to_path_buf()
}

unsafe fn descriptor_to_info(bundle_path: &Path, desc: &clap_plugin_descriptor) -> PluginInfo {
    let id = unsafe { cstr_to_string(desc.id) };
    let name = unsafe { cstr_to_string(desc.name) };
    let vendor = unsafe { cstr_to_string(desc.vendor) };
    let version_str = unsafe { cstr_to_string(desc.version) };
    let version = parse_version(&version_str);
    let (category, accepts_midi) = unsafe { categorize(desc.features) };
    PluginInfo {
        name,
        vendor,
        version,
        category,
        path: bundle_path.to_path_buf(),
        unique_id: id,
        format: FORMAT,
        has_editor: false, // honest default — set during load via the GUI ext.
        accepts_midi,
    }
}

/// CLAP feature strings live in a NULL-terminated array of C
/// strings. We scan for instrument / note-effect / analyzer
/// markers and the `note-input` / `note-effect` markers used to
/// flag MIDI capability.
unsafe fn categorize(features: *const *const c_char) -> (PluginCategory, bool) {
    if features.is_null() {
        return (PluginCategory::Effect, false);
    }
    let mut category = PluginCategory::Effect;
    let mut accepts_midi = false;
    let mut idx = 0usize;
    loop {
        let p = unsafe { *features.add(idx) };
        if p.is_null() {
            break;
        }
        let bytes = unsafe { CStr::from_ptr(p).to_bytes() };
        match bytes {
            b"instrument" => category = PluginCategory::Instrument,
            b"note-effect" => {
                category = PluginCategory::NoteEffect;
                accepts_midi = true;
            }
            b"analyzer" => category = PluginCategory::Analyzer,
            b"utility" => category = PluginCategory::Tool,
            b"note-input" => accepts_midi = true,
            _ => {}
        }
        idx += 1;
    }
    (category, accepts_midi)
}

/// CLAP versions are `"major.minor.patch"`. We pack the first
/// three numeric components into a `u32` as
/// `(major << 16) | (minor << 8) | patch`, matching what the
/// legacy rack used.
fn parse_version(s: &str) -> u32 {
    let mut parts = s.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    (major << 16) | (minor << 8) | patch
}

fn empty_param_info() -> clap_param_info {
    clap_param_info {
        id: 0,
        flags: 0,
        cookie: ptr::null_mut(),
        name: [0; clap_sys::string_sizes::CLAP_NAME_SIZE],
        module: [0; clap_sys::string_sizes::CLAP_PATH_SIZE],
        min_value: 0.0,
        max_value: 0.0,
        default_value: 0.0,
    }
}

fn clap_param_info_to_rack(info: &clap_param_info) -> ParameterInfo {
    let name = c_buf_to_string(&info.name);
    let mut flags = truce_rack_core::info::ParameterFlags::empty();
    if info.flags & CLAP_PARAM_IS_BYPASS != 0 {
        flags |= truce_rack_core::info::ParameterFlags::BYPASS;
    }
    if info.flags & CLAP_PARAM_IS_AUTOMATABLE != 0 {
        flags |= truce_rack_core::info::ParameterFlags::AUTOMATABLE;
    }
    if info.flags & CLAP_PARAM_IS_HIDDEN != 0 {
        flags |= truce_rack_core::info::ParameterFlags::HIDDEN;
    }
    if info.flags & CLAP_PARAM_IS_READONLY != 0 {
        flags |= truce_rack_core::info::ParameterFlags::READ_ONLY;
    }
    if info.flags & CLAP_PARAM_IS_ENUM != 0 {
        flags |= truce_rack_core::info::ParameterFlags::ENUMERATED;
    }
    let step_count = if info.flags & CLAP_PARAM_IS_STEPPED != 0 {
        // Stepped parameters have integer-valued [min, max]; the
        // step count is the number of distinct values.
        #[allow(clippy::cast_possible_truncation)]
        let span = (info.max_value - info.min_value).round() as i64;
        u32::try_from(span + 1).unwrap_or(0)
    } else {
        0
    };
    ParameterInfo {
        id: info.id,
        name: name.clone(),
        short_name: name,
        unit: String::new(), // CLAP doesn't expose a separate unit field
        min: info.min_value,
        max: info.max_value,
        default: info.default_value,
        step_count,
        flags,
    }
}

fn c_buf_to_string(buf: &[c_char]) -> String {
    // `c_char` is signed on some targets and unsigned on others. Its native
    // representation is one byte, so this preserves the CLAP byte without a cast.
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&byte| byte.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

unsafe fn cstr_to_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// RAII wrapper around a `libloading::Library` plus the
/// `clap_entry` symbol it exports.
struct LoadedLibrary {
    library: libloading::Library,
}

impl LoadedLibrary {
    unsafe fn open(path: &Path) -> Result<Self> {
        // SAFETY: caller guarantees `path` points at a valid
        // CLAP plugin bundle binary; `libloading::Library::new`
        // is itself the OS dlopen call.
        let library = unsafe { libloading::Library::new(path) }.map_err(|e| Error::LoadFailed {
            path: path.to_path_buf(),
            reason: format!("dlopen failed: {e}"),
        })?;
        Ok(Self { library })
    }

    fn entry(&self) -> Result<EntryRef<'_>> {
        let symbol: libloading::Symbol<'_, *const clap_plugin_entry> = unsafe {
            self.library
                .get(ENTRY_SYMBOL)
                .map_err(|e| Error::LoadFailed {
                    path: PathBuf::new(),
                    reason: format!("missing clap_entry symbol: {e}"),
                })?
        };
        let entry = *symbol;
        if entry.is_null() {
            return Err(Error::LoadFailed {
                path: PathBuf::new(),
                reason: "clap_entry symbol resolved to NULL".into(),
            });
        }
        Ok(EntryRef {
            entry,
            _phantom: std::marker::PhantomData,
        })
    }
}

struct EntryRef<'a> {
    entry: *const clap_plugin_entry,
    // The reference lifetime ties the entry pointer to the
    // library handle it was loaded from.
    #[allow(dead_code)]
    _phantom: std::marker::PhantomData<&'a LoadedLibrary>,
}

impl EntryRef<'_> {
    unsafe fn init(&self, path: &Path) -> Result<()> {
        let init = unsafe { (*self.entry).init };
        if let Some(init) = init {
            let c_path =
                CString::new(path.to_string_lossy().as_bytes()).map_err(|e| Error::LoadFailed {
                    path: path.to_path_buf(),
                    reason: format!("plugin path is not a valid C string: {e}"),
                })?;
            if !unsafe { init(c_path.as_ptr()) } {
                return Err(Error::LoadFailed {
                    path: path.to_path_buf(),
                    reason: "clap_entry::init returned false".into(),
                });
            }
        }
        Ok(())
    }

    unsafe fn deinit(&self) {
        if let Some(deinit) = unsafe { (*self.entry).deinit } {
            unsafe { deinit() };
        }
    }

    unsafe fn factory(&self) -> *const clap_plugin_factory {
        let Some(get_factory) = (unsafe { (*self.entry).get_factory }) else {
            return ptr::null();
        };
        let raw = unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) };
        raw.cast::<clap_plugin_factory>()
    }
}

struct EntryInitGuard {
    entry: *const clap_plugin_entry,
    armed: bool,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl EntryInitGuard {
    fn new(entry: *const clap_plugin_entry) -> Self {
        Self {
            entry,
            armed: true,
            _not_send: std::marker::PhantomData,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for EntryInitGuard {
    fn drop(&mut self) {
        if self.armed
            && let Some(deinit) = unsafe { (*self.entry).deinit }
        {
            // SAFETY: the load path creates this guard only after entry.init succeeds and keeps
            // the owning LoadedLibrary declared before the guard until it is dropped.
            unsafe { deinit() };
        }
    }
}

struct PluginInstanceGuard(
    *const clap_plugin,
    std::marker::PhantomData<std::rc::Rc<()>>,
);

impl PluginInstanceGuard {
    fn new() -> Self {
        Self(ptr::null(), std::marker::PhantomData)
    }

    fn set(&mut self, plugin: *const clap_plugin) {
        self.0 = plugin;
    }

    fn take(&mut self) -> *const clap_plugin {
        let plugin = self.0;
        self.0 = ptr::null();
        plugin
    }
}

impl Drop for PluginInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null()
            && let Some(destroy) = unsafe { (*self.0).destroy }
        {
            // SAFETY: the guard owns a plugin returned by the factory and is dropped before its
            // host context, entry state, and library.
            unsafe { destroy(self.0) };
        }
    }
}

/// One loaded CLAP plugin instance.
///
/// Holds the raw `*const clap_plugin` pointer plus the metadata
/// needed for the truce-rack-core trait impls. The pointer is owned —
/// `Drop` calls `clap_plugin::destroy` and `clap_entry::deinit`.
pub struct ClapPlugin {
    info: PluginInfo,
    audio_port_catalog: AudioPortCatalog,
    active_layout: Option<BusLayout>,
    plugin: *const clap_plugin,
    /// Stable host context outlives plugin destroy and entry deinit in `Drop`.
    host_context: Box<HostContext>,
    library: LoadedLibrary,
    bundle_path: PathBuf,
    started_processing: bool,
    activation: Option<ActivationConfig>,
    completed_restart_generation: u64,
    completed_latency_generation: u64,
    parameter_ids: ParameterIds,
    parameters_changed_pending: bool,
    /// Main-bus pointer slots prepared on activation and cleared after each synchronous block.
    process_pointers: Option<ProcessChannelPointers>,
    /// Cached `clap.params` extension. NULL if the plugin doesn't
    /// expose any parameters.
    params_ext: *const clap_plugin_params,
    /// Cached `clap.state` extension. NULL if the plugin doesn't
    /// expose state save/load.
    state_ext: *const clap_plugin_state,
    /// Cached `clap.gui` extension. NULL when the plugin has no
    /// custom editor.
    gui_ext: *const clap_plugin_gui,
    /// Cached `clap.timer-support` extension. NULL when the plugin does not implement it.
    timer_ext: *const clap_plugin_timer_support,
    /// Cached `clap.latency` extension. NULL means the optional extension is absent.
    latency_ext: *const clap_plugin_latency,
    /// Last lifecycle-thread latency snapshot; never calls into the plugin on read.
    latency_frames: Option<u32>,
    /// Whether the editor has been `create`d but not yet
    /// `destroy`ed — `clap.gui` separates the two lifecycles.
    gui_open: bool,
    /// Running `steady_time` counter — CLAP wants a monotonically
    /// increasing sample-count across activations.
    steady_time: i64,
}

#[derive(Clone)]
struct ActivationConfig {
    layout: BusLayout,
    sample_rate: f64,
    max_block_size: usize,
}

struct DiscoveredPluginMetadata {
    info: PluginInfo,
    audio_port_catalog: AudioPortCatalog,
    parameter_ids: ParameterIds,
    params_ext: *const clap_plugin_params,
    state_ext: *const clap_plugin_state,
    gui_ext: *const clap_plugin_gui,
    timer_ext: *const clap_plugin_timer_support,
    latency_ext: *const clap_plugin_latency,
}

struct ActivationSnapshot {
    parameter_ids: ParameterIds,
    latency_frames: Option<u32>,
    latency_generation: u64,
}

/// Per-bus channel pointers allocated for the accepted layout during activation.
/// Values are filled for one synchronous block and reset before that block returns.
struct ProcessChannelPointers {
    input_channels: Vec<Vec<*mut f32>>,
    output_channels: Vec<Vec<*mut f32>>,
    input_buffers: Vec<clap_audio_buffer>,
    output_buffers: Vec<clap_audio_buffer>,
    input_count: u32,
    output_count: u32,
}

impl ProcessChannelPointers {
    fn for_layout(layout: &BusLayout) -> Result<Self> {
        let (input_channels, input_buffers, input_count) = bus_pointer_slots(&layout.inputs)?;
        let (output_channels, output_buffers, output_count) = bus_pointer_slots(&layout.outputs)?;
        Ok(Self {
            input_channels,
            output_channels,
            input_buffers,
            output_buffers,
            input_count,
            output_count,
        })
    }

    fn matches_buffer(&self, buffer: &mut AudioBuffer<'_, f32>) -> bool {
        if self.input_channels.len() != buffer.num_input_buses()
            || self.output_channels.len() != buffer.num_output_buses()
        {
            return false;
        }
        self.input_channels
            .iter()
            .enumerate()
            .all(|(index, channels)| buffer.bus_inputs(index).len() == channels.len())
            && self
                .output_channels
                .iter()
                .enumerate()
                .all(|(index, channels)| buffer.bus_outputs(index).len() == channels.len())
    }

    fn fill_from(&mut self, buffer: &mut AudioBuffer<'_, f32>) -> bool {
        self.clear();
        if !self.matches_buffer(buffer) {
            return false;
        }

        for (bus_index, (pointer_slots, audio_buffer)) in self
            .input_channels
            .iter_mut()
            .zip(&mut self.input_buffers)
            .enumerate()
        {
            for (pointer, channel) in pointer_slots.iter_mut().zip(buffer.bus_inputs(bus_index)) {
                *pointer = channel.as_ptr().cast_mut();
            }
            audio_buffer.data32 = if pointer_slots.is_empty() {
                ptr::null_mut()
            } else {
                pointer_slots.as_mut_ptr()
            };
        }

        for (bus_index, (pointer_slots, audio_buffer)) in self
            .output_channels
            .iter_mut()
            .zip(&mut self.output_buffers)
            .enumerate()
        {
            for (pointer, channel) in pointer_slots
                .iter_mut()
                .zip(buffer.bus_outputs(bus_index).iter_mut())
            {
                *pointer = (*channel).as_mut_ptr();
            }
            audio_buffer.data32 = if pointer_slots.is_empty() {
                ptr::null_mut()
            } else {
                pointer_slots.as_mut_ptr()
            };
        }
        true
    }

    fn clear(&mut self) {
        for (channels, audio_buffer) in self
            .input_channels
            .iter_mut()
            .zip(&mut self.input_buffers)
            .chain(
                self.output_channels
                    .iter_mut()
                    .zip(&mut self.output_buffers),
            )
        {
            channels.fill(ptr::null_mut());
            audio_buffer.data32 = ptr::null_mut();
        }
    }
}

type BusPointerSlots = (Vec<Vec<*mut f32>>, Vec<clap_audio_buffer>, u32);

fn bus_pointer_slots(buses: &[truce_rack_core::bus::Bus]) -> Result<BusPointerSlots> {
    if buses.len() > 64 {
        return Err(Error::Other("CLAP layout exceeds 64 audio buses".into()));
    }
    let bus_count = u32::try_from(buses.len())
        .map_err(|_| Error::Other("CLAP audio bus count exceeds u32".into()))?;
    let mut channels_by_bus = Vec::new();
    channels_by_bus
        .try_reserve_exact(buses.len())
        .map_err(|error| Error::Other(format!("cannot reserve CLAP bus pointers: {error}")))?;
    let mut audio_buffers = Vec::new();
    audio_buffers
        .try_reserve_exact(buses.len())
        .map_err(|error| Error::Other(format!("cannot reserve CLAP audio buffers: {error}")))?;

    let mut total_channels = 0_usize;
    for bus in buses {
        let channel_count = usize::try_from(bus.channels.count())
            .map_err(|_| Error::Other("CLAP bus channel count exceeds usize".into()))?;
        total_channels = total_channels
            .checked_add(channel_count)
            .ok_or_else(|| Error::Other("CLAP total channel count overflow".into()))?;
        if channel_count == 0 || total_channels > 64 {
            return Err(Error::Other(
                "CLAP direction must contain at most 64 total channels".into(),
            ));
        }
        let channel_count_u32 = u32::try_from(channel_count)
            .map_err(|_| Error::Other("CLAP bus channel count exceeds u32".into()))?;
        let mut channel_pointers = Vec::new();
        channel_pointers
            .try_reserve_exact(channel_count)
            .map_err(|error| {
                Error::Other(format!("cannot reserve CLAP channel pointers: {error}"))
            })?;
        channel_pointers.resize(channel_count, ptr::null_mut());
        channels_by_bus.push(channel_pointers);
        audio_buffers.push(clap_audio_buffer {
            data32: ptr::null_mut(),
            data64: ptr::null_mut(),
            channel_count: channel_count_u32,
            latency: 0,
            constant_mask: 0,
        });
    }
    Ok((channels_by_bus, audio_buffers, bus_count))
}

struct ResetProcessPointers<'a> {
    pointers: &'a mut ProcessChannelPointers,
}

impl Drop for ResetProcessPointers<'_> {
    fn drop(&mut self) {
        self.pointers.clear();
    }
}

/// Native endpoints and stable host state for one activation/restart lifecycle operation.
#[derive(Clone, Copy)]
struct ActivationTarget<'a> {
    plugin: *const clap_plugin,
    host_context: &'a HostContext,
    params_ext: *const clap_plugin_params,
    latency_ext: *const clap_plugin_latency,
}

impl ActivationTarget<'_> {
    fn activate(&self, config: &ActivationConfig) -> Result<ActivationSnapshot> {
        let _main_thread = self.host_context.main_thread_scope()?;
        let max_block_size = u32::try_from(config.max_block_size)
            .map_err(|_| Error::Other("CLAP max block size exceeds u32".into()))?;
        let activate = unsafe { (*self.plugin).activate }
            .ok_or_else(|| Error::Other("clap plugin missing activate".into()))?;
        if !unsafe { activate(self.plugin, config.sample_rate, 1, max_block_size) } {
            return Err(Error::Other("clap_plugin::activate returned false".into()));
        }

        let refreshed = (|| {
            let parameter_ids = read_parameter_ids(self.plugin, self.params_ext)?;
            // CLAP permits host.latency.changed while being activated. Query after activation and
            // parameter refresh, then absorb notifications through this snapshot.
            // SAFETY: the live plugin is active and this is its fixed main thread.
            let latency_frames = unsafe { query_latency_frames(self.plugin, self.latency_ext) };
            let latency_generation = self.host_context.latency_changed_generation();
            Ok::<_, Error>(ActivationSnapshot {
                parameter_ids,
                latency_frames,
                latency_generation,
            })
        })();
        match refreshed {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                if let Some(deactivate) = unsafe { (*self.plugin).deactivate } {
                    unsafe { deactivate(self.plugin) };
                }
                Err(error)
            }
        }
    }

    fn activate_with_cache(
        &self,
        config: &ActivationConfig,
        latency_cache: &mut Option<u32>,
    ) -> Result<ActivationSnapshot> {
        *latency_cache = None;
        let snapshot = self.activate(config)?;
        *latency_cache = snapshot.latency_frames;
        Ok(snapshot)
    }
}

fn deactivate_native(
    plugin: *const clap_plugin,
    host_context: &HostContext,
    started_processing: &mut bool,
    active: bool,
) -> Result<()> {
    let _main_thread = host_context.main_thread_scope()?;
    if *started_processing {
        let stop = unsafe { (*plugin).stop_processing }
            .ok_or_else(|| Error::Other("clap plugin missing stop_processing".into()))?;
        {
            // CLAP audio-thread is symbolic; the exclusive owner may stop this instance from
            // its fixed main OS thread while no DSP call can overlap.
            let _audio_thread = host_context.audio_thread_scope();
            unsafe { stop(plugin) };
        }
        *started_processing = false;
    }
    if active {
        let deactivate = unsafe { (*plugin).deactivate }
            .ok_or_else(|| Error::Other("clap plugin missing deactivate".into()))?;
        unsafe { deactivate(plugin) };
    }
    Ok(())
}

fn close_gui_native(
    plugin: *const clap_plugin,
    host_context: &HostContext,
    gui_ext: *const clap_plugin_gui,
    gui_open: &mut bool,
) -> Result<()> {
    let _main_thread = host_context.main_thread_scope()?;
    let was_closed = host_context.take_gui_closed();
    host_context.reset_gui();
    if !*gui_open {
        return Ok(());
    }
    if gui_ext.is_null() {
        return Err(Error::Other(
            "open CLAP GUI lost its extension pointer".into(),
        ));
    }
    if !was_closed && let Some(hide) = unsafe { (*gui_ext).hide } {
        let _ = unsafe { hide(plugin) };
    }
    let Some(destroy) = (unsafe { (*gui_ext).destroy }) else {
        return Err(Error::Other(
            "open CLAP GUI missing destroy callback".into(),
        ));
    };
    unsafe { destroy(plugin) };
    *gui_open = false;
    Ok(())
}

/// Owned references and state needed for ordered CLAP instance teardown.
struct InstanceDestruction<'a> {
    plugin: *const clap_plugin,
    entry: *const clap_plugin_entry,
    host_context: &'a HostContext,
    gui_ext: *const clap_plugin_gui,
    gui_open: &'a mut bool,
    started_processing: &'a mut bool,
    active: bool,
}

/// Destroy an instance and deinitialize its CLAP entry after closing/deactivating it.
/// # Safety
/// The plugin and entry pointers must be null or point to live records from the same library.
/// The caller must retain that library and the host context until this function returns.
unsafe fn destroy_instance(instance: InstanceDestruction<'_>) {
    let InstanceDestruction {
        plugin,
        entry,
        host_context,
        gui_ext,
        gui_open,
        started_processing,
        active,
    } = instance;
    let _main_thread = host_context
        .main_thread_scope()
        .expect("CLAP plugin owner must destroy the instance on its fixed main thread");
    if !plugin.is_null() {
        let _ = close_gui_native(plugin, host_context, gui_ext, gui_open);
        let _ = deactivate_native(plugin, host_context, started_processing, active);
        if let Some(destroy) = unsafe { (*plugin).destroy } {
            // SAFETY: owner holds the plugin and library until this returns.
            unsafe { destroy(plugin) };
        }
    }
    host_context.clear_timers();
    if !entry.is_null()
        && let Some(deinit) = unsafe { (*entry).deinit }
    {
        // SAFETY: caller keeps the dynamic library and host context alive through this callback.
        unsafe { deinit() };
    }
}

// SAFETY: Moving the wrapper does not transfer or destroy native ownership. The host serializes
// access through its instance mutex; lifecycle and host-service calls additionally verify the
// captured creator thread, while DSP calls use the adapter's symbolic audio-thread scope.
unsafe impl Send for ClapPlugin {}

struct NativeProcessTarget<'a> {
    plugin: *const clap_plugin,
    host_context: &'a HostContext,
    steady_time: i64,
    pointers: &'a mut ProcessChannelPointers,
}

/// Build the borrowed CLAP block records and synchronously call the native processor.
fn run_native_process_block(
    target: NativeProcessTarget<'_>,
    buffer: &mut AudioBuffer<'_, f32>,
    transport_event: Option<clap_event_transport>,
    input_events: &clap_input_events,
    output_events: &clap_output_events,
) -> i32 {
    let NativeProcessTarget {
        plugin,
        host_context,
        steady_time,
        pointers,
    } = target;
    if !pointers.fill_from(buffer) {
        return CLAP_PROCESS_ERROR;
    }
    let pointer_reset = ResetProcessPointers { pointers };
    let num_frames = buffer.num_frames();
    let process = clap_process {
        steady_time,
        frames_count: u32::try_from(num_frames).unwrap_or(u32::MAX),
        transport: transport_event
            .as_ref()
            .map_or(ptr::null(), std::ptr::from_ref),
        audio_inputs: if pointer_reset.pointers.input_buffers.is_empty() {
            ptr::null()
        } else {
            pointer_reset.pointers.input_buffers.as_ptr()
        },
        audio_outputs: if pointer_reset.pointers.output_buffers.is_empty() {
            ptr::null_mut()
        } else {
            pointer_reset.pointers.output_buffers.as_mut_ptr()
        },
        audio_inputs_count: pointer_reset.pointers.input_count,
        audio_outputs_count: pointer_reset.pointers.output_count,
        in_events: input_events,
        out_events: output_events,
    };

    let process_ptr = unsafe { (*plugin).process };
    match process_ptr {
        Some(process_fn) => {
            // `request_process` is coalesced until a real transport block reaches this native
            // call. It does not synthesize processing while the graph is idle/OFF.
            let _requested_process = host_context.take_process_request();
            let _audio_thread = host_context.audio_thread_scope();
            run_audio_block_with::<ClapPlugin, i32>(FORMAT, CLAP_PROCESS_ERROR, || unsafe {
                process_fn(plugin, &raw const process)
            })
        }
        None => CLAP_PROCESS_ERROR,
    }
}

impl ClapPlugin {
    fn activate_config(&mut self, config: &ActivationConfig, generation: u64) -> Result<()> {
        let _main_thread = self.host_context.main_thread_scope()?;
        self.process_pointers = None;
        self.audio_port_catalog
            .select_and_validate(&config.layout)?;
        let process_pointers = ProcessChannelPointers::for_layout(&config.layout)?;
        let target = ActivationTarget {
            plugin: self.plugin,
            host_context: &self.host_context,
            params_ext: self.params_ext,
            latency_ext: self.latency_ext,
        };
        let snapshot = target.activate_with_cache(config, &mut self.latency_frames);
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.active_layout = None;
                self.latency_frames = None;
                return Err(error);
            }
        };

        self.active_layout = Some(config.layout.clone());
        self.parameter_ids = snapshot.parameter_ids;
        self.completed_restart_generation = generation;
        self.completed_latency_generation = snapshot.latency_generation;
        self.process_pointers = Some(process_pointers);
        Ok(())
    }

    /// Borrow an engine buffer as CLAP's per-block audio records and make one native process call.
    fn call_process_block(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        transport_event: Option<clap_event_transport>,
        input_events: &clap_input_events,
        output_events: &clap_output_events,
    ) -> i32 {
        let Some(pointers) = self.process_pointers.as_mut() else {
            return CLAP_PROCESS_ERROR;
        };
        run_native_process_block(
            NativeProcessTarget {
                plugin: self.plugin,
                host_context: &self.host_context,
                steady_time: self.steady_time,
                pointers,
            },
            buffer,
            transport_event,
            input_events,
            output_events,
        )
    }

    fn deactivate_native(&mut self) -> Result<()> {
        self.latency_frames = None;
        deactivate_native(
            self.plugin,
            &self.host_context,
            &mut self.started_processing,
            self.active_layout.is_some(),
        )?;
        self.active_layout = None;
        self.process_pointers = None;
        Ok(())
    }

    fn load_from(info: &PluginInfo) -> Result<Self> {
        let bundle_path = info.path.clone();
        let binary = bundle_binary_path(&bundle_path);
        let library = unsafe { LoadedLibrary::open(&binary)? };
        let host_context = HostContext::new();
        let _main_thread = host_context.main_thread_scope()?;
        let entry = library.entry()?;
        unsafe { entry.init(&binary)? };
        let mut entry_guard = EntryInitGuard::new(entry.entry);
        let factory = unsafe { entry.factory() };
        if factory.is_null() {
            return Err(Error::LoadFailed {
                path: bundle_path,
                reason: "plugin has no clap.plugin-factory".into(),
            });
        }
        let Some(create) = (unsafe { (*factory).create_plugin }) else {
            return Err(Error::LoadFailed {
                path: bundle_path,
                reason: "factory missing create_plugin".into(),
            });
        };
        let id_cstring = CString::new(info.unique_id.as_str()).map_err(|e| Error::LoadFailed {
            path: bundle_path.clone(),
            reason: format!("plugin id is not a valid C string: {e}"),
        })?;
        let mut plugin_guard = PluginInstanceGuard::new();
        let plugin = unsafe { create(factory, host_context.host_ptr(), id_cstring.as_ptr()) };
        plugin_guard.set(plugin);
        if plugin.is_null() {
            return Err(Error::LoadFailed {
                path: bundle_path,
                reason: "factory.create_plugin returned NULL".into(),
            });
        }
        let Some(init) = (unsafe { (*plugin).init }) else {
            return Err(Error::LoadFailed {
                path: bundle_path,
                reason: "plugin missing init".into(),
            });
        };
        if !unsafe { init(plugin) } {
            return Err(Error::LoadFailed {
                path: bundle_path,
                reason: "clap_plugin::init returned false".into(),
            });
        }
        let metadata = discover_plugin_metadata(info, plugin)?;
        let plugin = plugin_guard.take();
        entry_guard.disarm();
        Ok(Self::from_loaded_parts(
            metadata,
            plugin,
            host_context,
            library,
            bundle_path,
        ))
    }

    fn from_loaded_parts(
        metadata: DiscoveredPluginMetadata,
        plugin: *const clap_plugin,
        host_context: Box<HostContext>,
        library: LoadedLibrary,
        bundle_path: PathBuf,
    ) -> Self {
        let DiscoveredPluginMetadata {
            info,
            audio_port_catalog,
            parameter_ids,
            params_ext,
            state_ext,
            gui_ext,
            timer_ext,
            latency_ext,
        } = metadata;
        Self {
            info,
            audio_port_catalog,
            active_layout: None,
            plugin,
            host_context,
            library,
            bundle_path,
            started_processing: false,
            activation: None,
            completed_restart_generation: 0,
            completed_latency_generation: 0,
            parameter_ids,
            parameters_changed_pending: false,
            process_pointers: None,
            params_ext,
            state_ext,
            gui_ext,
            timer_ext,
            latency_ext,
            latency_frames: None,
            gui_open: false,
            steady_time: 0,
        }
    }
}

fn discover_plugin_metadata(
    info: &PluginInfo,
    plugin: *const clap_plugin,
) -> Result<DiscoveredPluginMetadata> {
    let params_ext = unsafe { lookup_extension::<clap_plugin_params>(plugin, CLAP_EXT_PARAMS) };
    let audio_ports_ext =
        unsafe { lookup_extension::<clap_plugin_audio_ports>(plugin, CLAP_EXT_AUDIO_PORTS) };
    let audio_ports_config_ext = unsafe {
        lookup_extension::<clap_plugin_audio_ports_config>(plugin, CLAP_EXT_AUDIO_PORTS_CONFIG)
    };
    let audio_ports_config_info_ext = unsafe {
        lookup_extension::<clap_plugin_audio_ports_config_info>(
            plugin,
            CLAP_EXT_AUDIO_PORTS_CONFIG_INFO,
        )
    };
    // SAFETY: extensions belong to this live instance; the caller is on its creator thread and
    // discovery runs before activation.
    let audio_port_catalog = unsafe {
        AudioPortCatalog::discover(
            plugin,
            audio_ports_ext,
            audio_ports_config_ext,
            audio_ports_config_info_ext,
        )
    }?;
    let state_ext = unsafe { lookup_extension::<clap_plugin_state>(plugin, CLAP_EXT_STATE) };
    let gui_ext = unsafe { lookup_extension::<clap_plugin_gui>(plugin, CLAP_EXT_GUI) };
    let timer_ext =
        unsafe { lookup_extension::<clap_plugin_timer_support>(plugin, CLAP_EXT_TIMER_SUPPORT) };
    let latency_ext = unsafe { lookup_extension::<clap_plugin_latency>(plugin, CLAP_EXT_LATENCY) };
    let parameter_ids = read_parameter_ids(plugin, params_ext)?;
    let mut plugin_info = info.clone();
    plugin_info.has_editor |=
        !gui_ext.is_null() && unsafe { gui_supports_current_platform(plugin, gui_ext) };

    Ok(DiscoveredPluginMetadata {
        info: plugin_info,
        audio_port_catalog,
        parameter_ids,
        params_ext,
        state_ext,
        gui_ext,
        timer_ext,
        latency_ext,
    })
}

/// Look up a CLAP extension by id. Returns `null` if the plugin
/// doesn't implement that extension. The returned pointer's
/// lifetime is the plugin instance; callers must not outlive
/// the plugin.
unsafe fn lookup_extension<T>(plugin: *const clap_plugin, id: &CStr) -> *const T {
    let Some(get_extension) = (unsafe { (*plugin).get_extension }) else {
        return ptr::null();
    };
    let raw = unsafe { get_extension(plugin, id.as_ptr()) };
    raw.cast::<T>()
}

fn read_parameter_ids(
    plugin: *const clap_plugin,
    params_ext: *const clap_plugin_params,
) -> Result<ParameterIds> {
    if params_ext.is_null() {
        return Ok(ParameterIds::empty());
    }
    // SAFETY: the extension pointer was obtained from this live plugin instance and callers
    // invoke this helper only on its fixed main thread while holding exclusive instance access.
    let params = unsafe { &*params_ext };
    let count = params
        .count
        .ok_or_else(|| Error::Other("clap.params missing count callback".into()))?;
    let get_info = params
        .get_info
        .ok_or_else(|| Error::Other("clap.params missing get_info callback".into()))?;
    let count = unsafe { count(plugin) };
    ParameterIds::collect(count, |index| {
        let mut info = empty_param_info();
        // SAFETY: the CLAP parameter index is within the count just returned by this instance;
        // caller is the plugin's main thread and the output record has the C ABI layout.
        if !unsafe { get_info(plugin, index, &raw mut info) } {
            return Err(Error::Other(format!(
                "clap.params.get_info failed while caching parameter index {index}"
            )));
        }
        Ok(ParameterMetadata {
            id: info.id,
            requires_process: info.flags & CLAP_PARAM_REQUIRES_PROCESS != 0,
        })
    })
}

fn queue_cached_parameter_change(
    parameter_ids: &mut ParameterIds,
    index: usize,
    value: f64,
) -> Result<()> {
    parameter_ids.queue(index, value)
}

fn flush_pending_parameter_changes(
    plugin: *const clap_plugin,
    params_ext: *const clap_plugin_params,
    host_context: &HostContext,
    parameter_ids: &mut ParameterIds,
) -> Result<bool> {
    let _main_thread = host_context.main_thread_scope()?;
    if !parameter_ids.has_flushable_pending() {
        return Ok(false);
    }
    if params_ext.is_null() {
        return Err(Error::Other(
            "pending CLAP parameter changes lost their params extension".into(),
        ));
    }
    let Some(flush) = (unsafe { (*params_ext).flush }) else {
        return Ok(false);
    };

    let mut converted = ConvertedInputEvents { events: Vec::new() };
    parameter_ids.for_each_flushable(|param_id, value| {
        converted.push_param_value(0, param_id, value);
    });
    let input = converted.as_clap();
    let mut sink = OutputEventsSink::new(None);
    let output = sink.as_clap();
    // The fixed-main broker owns the instance exclusively here; CLAP's audio-thread role is
    // symbolic and may be reported on this thread for the serialized flush call.
    {
        let _audio_thread = host_context.audio_thread_scope();
        unsafe { flush(plugin, &raw const input, &raw const output) };
    }
    parameter_ids.clear_flushed();
    Ok(true)
}

fn commit_processed_parameter_changes(parameter_ids: &mut ParameterIds, status: i32) {
    if status != CLAP_PROCESS_ERROR {
        parameter_ids.clear_all_pending();
    }
}

/// Read a CLAP latency value while the plugin is active on its lifecycle thread.
///
/// CLAP defines the extension as optional; without it the host has no reported intrinsic
/// latency and uses zero. A present but incomplete extension is unknown, not zero.
///
/// # Safety
/// `plugin` and any non-null `latency_ext` must belong to the same live instance, and this must
/// be called on the CLAP main thread while the plugin is being activated or active.
unsafe fn query_latency_frames(
    plugin: *const clap_plugin,
    latency_ext: *const clap_plugin_latency,
) -> Option<u32> {
    if latency_ext.is_null() {
        return Some(0);
    }
    // SAFETY: the extension pointer came from this live CLAP plugin instance and is only
    // queried during activation on the serialized lifecycle thread.
    let get = unsafe { (*latency_ext).get }?;
    // SAFETY: CLAP permits latency.get while being activated or active; caller invokes it only
    // after a successful activate call and keeps the plugin instance alive.
    Some(unsafe { get(plugin) })
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        let entry = self
            .library
            .entry()
            .map_or(ptr::null(), |entry| entry.entry);
        // SAFETY: this owner keeps host and library fields alive for the complete teardown call.
        unsafe {
            destroy_instance(InstanceDestruction {
                plugin: self.plugin,
                entry,
                host_context: &self.host_context,
                gui_ext: self.gui_ext,
                gui_open: &mut self.gui_open,
                started_processing: &mut self.started_processing,
                active: self.active_layout.is_some(),
            });
        }
        // `library` drops here, unloading the dylib.
        let _ = &self.bundle_path; // keep PathBuf alive for diagnostics
    }
}

impl PluginCore for ClapPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn active_layout(&self) -> Option<&BusLayout> {
        self.active_layout.as_ref()
    }

    fn latency_frames(&self) -> Option<u32> {
        (self.host_context.restart_generation() == self.completed_restart_generation
            && self.host_context.latency_changed_generation() == self.completed_latency_generation)
            .then_some(self.latency_frames)
            .flatten()
    }

    fn service_main_thread(&mut self) -> Result<MainThreadUpdate> {
        let _main_thread = self.host_context.main_thread_scope()?;
        // Consume before the native callback. A request raised from on_main_thread remains set
        // for the next broker tick rather than recursively dispatching without a bound.
        // SAFETY: the broker owns the instance mutex and invokes this on the fixed main thread.
        unsafe { self.host_context.service_callback(self.plugin)? };
        if !self.timer_ext.is_null() {
            self.host_context
                .service_timers(self.plugin, self.timer_ext, Instant::now());
        }

        let generation = self.host_context.restart_generation();
        let latency_generation = self.host_context.latency_changed_generation();
        let mut parameters_changed = std::mem::take(&mut self.parameters_changed_pending);
        if generation != self.completed_restart_generation
            || latency_generation != self.completed_latency_generation
        {
            let Some(config) = self.activation.clone() else {
                return Ok(if parameters_changed {
                    MainThreadUpdate::ParametersChanged
                } else {
                    MainThreadUpdate::Unchanged
                });
            };

            // The broker holds exclusive instance ownership, so this is the drain point for any
            // in-flight process call. Pending ID-addressed events must not cross the replacement
            // parameter map.
            self.latency_frames = None;
            self.parameter_ids.clear_all_pending();
            self.deactivate_native()?;
            self.activate_config(&config, generation)?;
            parameters_changed = true;
        }

        Ok(if parameters_changed {
            MainThreadUpdate::ParametersChanged
        } else {
            MainThreadUpdate::Unchanged
        })
    }

    fn has_pending_parameter_changes(&self) -> bool {
        self.parameter_ids.has_pending()
    }

    fn supported_layouts(&self) -> &[BusLayout] {
        self.audio_port_catalog.layouts()
    }

    fn parameter_count(&self) -> usize {
        self.parameter_ids.len()
    }

    fn parameter_info(&self, index: usize) -> Result<ParameterInfo> {
        let _main_thread = self.host_context.main_thread_scope()?;
        self.parameter_ids.id(index)?;
        if self.params_ext.is_null() {
            return Err(Error::InvalidParameter(index));
        }
        let get_info =
            unsafe { (*self.params_ext).get_info }.ok_or(Error::InvalidParameter(index))?;
        let mut info = empty_param_info();
        let idx_u32 = u32::try_from(index).map_err(|_| Error::InvalidParameter(index))?;
        let ok = unsafe { get_info(self.plugin, idx_u32, &raw mut info) };
        if !ok {
            return Err(Error::InvalidParameter(index));
        }
        Ok(clap_param_info_to_rack(&info))
    }

    fn parameter_value(&self, index: usize) -> Result<f64> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if self.params_ext.is_null() {
            return Err(Error::InvalidParameter(index));
        }
        let id = self.parameter_ids.id(index)?;
        let get_value =
            unsafe { (*self.params_ext).get_value }.ok_or(Error::InvalidParameter(index))?;
        let mut out = 0.0f64;
        let ok = unsafe { get_value(self.plugin, id, &raw mut out) };
        if !ok {
            return Err(Error::InvalidParameter(index));
        }
        Ok(out)
    }

    fn parameter_value_string(&self, index: usize, value: f64) -> Result<String> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if self.params_ext.is_null() {
            return Err(Error::InvalidParameter(index));
        }
        let id = self.parameter_ids.id(index)?;
        let value_to_text =
            unsafe { (*self.params_ext).value_to_text }.ok_or(Error::InvalidParameter(index))?;
        // Match the CLAP ABI on signed- and unsigned-c_char targets alike.
        let mut buf = [0 as c_char; clap_sys::string_sizes::CLAP_NAME_SIZE];
        let buf_len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let ok = unsafe { value_to_text(self.plugin, id, value, buf.as_mut_ptr(), buf_len) };
        if !ok {
            return Err(Error::InvalidParameter(index));
        }
        Ok(c_buf_to_string(&buf))
    }

    fn set_parameter(&mut self, index: usize, value: f64) -> Result<()> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if !self.is_active() {
            return Err(Error::NotActivated);
        }
        if self.params_ext.is_null() {
            return Err(Error::InvalidParameter(index));
        }
        queue_cached_parameter_change(&mut self.parameter_ids, index, value)?;
        // Queue the change either way; process() drains the queue
        // every block, and a non-processing host can call flush()
        // to apply it immediately.
        if !self.started_processing {
            let _ = flush_pending_parameter_changes(
                self.plugin,
                self.params_ext,
                &self.host_context,
                &mut self.parameter_ids,
            )?;
        }
        Ok(())
    }

    fn preset_count(&self) -> usize {
        0
    }

    fn preset_info(&self, index: usize) -> Result<PresetInfo> {
        Err(Error::InvalidParameter(index))
    }

    fn load_preset(&mut self, _preset_number: i32) -> Result<()> {
        Err(Error::Other("clap preset loading not yet wired".into()))
    }

    fn save_state(&self) -> Result<Vec<u8>> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if self.state_ext.is_null() {
            return Err(Error::Other("plugin missing clap.state extension".into()));
        }
        let save = unsafe { (*self.state_ext).save }
            .ok_or_else(|| Error::Other("clap.state extension missing save fn".into()))?;
        let mut buffer = WriteBuffer::default();
        let stream = clap_ostream {
            ctx: (&raw mut buffer).cast(),
            write: Some(ostream_write),
        };
        let ok = unsafe { save(self.plugin, &raw const stream) };
        if !ok {
            return Err(Error::Other("clap state save returned false".into()));
        }
        Ok(buffer.bytes)
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if self.state_ext.is_null() {
            return Err(Error::Other("plugin missing clap.state extension".into()));
        }
        let load = unsafe { (*self.state_ext).load }
            .ok_or_else(|| Error::Other("clap.state extension missing load fn".into()))?;
        let mut cursor = ReadCursor { bytes, position: 0 };
        let stream = clap_istream {
            ctx: (&raw mut cursor).cast(),
            read: Some(istream_read),
        };
        self.latency_frames = None;
        let ok = unsafe { load(self.plugin, &raw const stream) };
        if !ok {
            return Err(Error::Other("clap state load returned false".into()));
        }
        self.parameter_ids.clear_all_pending();
        self.parameter_ids = read_parameter_ids(self.plugin, self.params_ext)?;
        self.parameters_changed_pending = true;
        self.latency_frames = if self.active_layout.is_some() {
            // SAFETY: state load ran on the fixed main thread while the instance remains active.
            unsafe { query_latency_frames(self.plugin, self.latency_ext) }
        } else {
            None
        };
        self.completed_latency_generation = self.host_context.latency_changed_generation();
        Ok(())
    }

    fn activate(
        &mut self,
        layout: BusLayout,
        sample_rate: f64,
        max_block_size: usize,
    ) -> Result<()> {
        if self.active_layout.is_some() {
            return Err(Error::Other(
                "CLAP activate requires the prior instance to be deactivated".into(),
            ));
        }
        let config = ActivationConfig {
            layout,
            sample_rate,
            max_block_size,
        };
        self.latency_frames = None;
        let generation = self.host_context.restart_generation();
        self.activate_config(&config, generation)?;
        self.activation = Some(config);
        self.parameters_changed_pending = true;
        Ok(())
    }

    fn deactivate(&mut self) {
        let Ok(_main_thread) = self.host_context.main_thread_scope() else {
            self.latency_frames = None;
            return;
        };
        if self.deactivate_native().is_ok() {
            self.activation = None;
            self.parameter_ids.clear_all_pending();
        }
    }

    fn is_active(&self) -> bool {
        self.active_layout.is_some()
    }

    fn editor(&mut self) -> Option<&mut dyn truce_rack_core::editor::PluginEditor> {
        if self.gui_ext.is_null() {
            return None;
        }
        Some(self)
    }
}

/// Platform API id we'd ask the plugin to support — one per
/// build target.
const fn platform_api() -> &'static CStr {
    #[cfg(target_os = "macos")]
    {
        CLAP_WINDOW_API_COCOA
    }
    #[cfg(target_os = "windows")]
    {
        CLAP_WINDOW_API_WIN32
    }
    #[cfg(target_os = "linux")]
    {
        CLAP_WINDOW_API_X11
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        CLAP_WINDOW_API_COCOA
    }
}

unsafe fn gui_supports_current_platform(
    plugin: *const clap_plugin,
    gui_ext: *const clap_plugin_gui,
) -> bool {
    let Some(is_supported) = (unsafe { (*gui_ext).is_api_supported }) else {
        return false;
    };
    unsafe { is_supported(plugin, platform_api().as_ptr(), false) }
}

fn handle_to_clap_window(handle: truce_rack_core::editor::WindowHandle) -> Result<clap_window> {
    use truce_rack_core::editor::WindowHandle;
    let (api, specific) = match handle {
        WindowHandle::NSView(p) => (CLAP_WINDOW_API_COCOA, clap_window_handle { cocoa: p }),
        WindowHandle::HWND(p) => (CLAP_WINDOW_API_WIN32, clap_window_handle { win32: p }),
        WindowHandle::X11(id) => {
            // CLAP uses C unsigned long: 32 bits on Windows and 32-bit Unix,
            // but 64 bits on the other supported Unix targets.
            #[cfg(any(target_os = "windows", target_pointer_width = "32"))]
            let id = u32::try_from(id)
                .map_err(|_| Error::Other("X11 window handle exceeds the CLAP ABI width".into()))?;
            (CLAP_WINDOW_API_X11, clap_window_handle { x11: id })
        }
    };
    // `create` and `set_parent` must agree on the window-system API.
    if api != platform_api() {
        return Err(Error::Other(
            "Parent window API differs from the CLAP editor API".into(),
        ));
    }
    Ok(clap_window {
        api: api.as_ptr(),
        specific,
    })
}

impl truce_rack_core::editor::PluginEditor for ClapPlugin {
    fn open(
        &mut self,
        parent: truce_rack_core::editor::WindowHandle,
        scale: f64,
    ) -> truce_rack_core::error::Result<()> {
        let _main_thread = self.host_context.main_thread_scope()?;
        if self.gui_open {
            return Ok(());
        }
        if self.gui_ext.is_null() {
            return Err(Error::Other("clap.gui extension absent".into()));
        }
        self.host_context.reset_gui();
        let window = handle_to_clap_window(parent)?;
        let api = platform_api();
        let create = unsafe { (*self.gui_ext).create }
            .ok_or_else(|| Error::Other("clap.gui missing `create`".into()))?;
        if !unsafe { create(self.plugin, api.as_ptr(), false) } {
            return Err(Error::Other("clap.gui::create returned false".into()));
        }
        self.gui_open = true;
        // CLAP Cocoa sizes use logical pixels; AppKit owns the backing scale. Calling
        // set_scale here can also trigger a vendor resize callback before embedding.
        if api != CLAP_WINDOW_API_COCOA
            && let Some(set_scale) = unsafe { (*self.gui_ext).set_scale }
        {
            let _ = unsafe { set_scale(self.plugin, scale) };
        }
        let Some(set_parent) = (unsafe { (*self.gui_ext).set_parent }) else {
            let _ = close_gui_native(
                self.plugin,
                &self.host_context,
                self.gui_ext,
                &mut self.gui_open,
            );
            return Err(Error::Other("clap.gui missing `set_parent`".into()));
        };
        if !unsafe { set_parent(self.plugin, &raw const window) } {
            let _ = close_gui_native(
                self.plugin,
                &self.host_context,
                self.gui_ext,
                &mut self.gui_open,
            );
            return Err(Error::Other("clap.gui::set_parent returned false".into()));
        }
        self.host_context.gui_parent_attached();
        if let Some(show) = unsafe { (*self.gui_ext).show }
            && !unsafe { show(self.plugin) }
        {
            let _ = close_gui_native(
                self.plugin,
                &self.host_context,
                self.gui_ext,
                &mut self.gui_open,
            );
            return Err(Error::Other("clap.gui::show returned false".into()));
        }
        Ok(())
    }

    fn close(&mut self) {
        let _ = close_gui_native(
            self.plugin,
            &self.host_context,
            self.gui_ext,
            &mut self.gui_open,
        );
        // Keep the cached value unless the plugin's latency/restart callbacks say it changed.
    }

    fn is_open(&self) -> bool {
        self.gui_open
    }

    fn size(&self) -> Option<(u32, u32)> {
        let _main_thread = self.host_context.main_thread_scope().ok()?;
        if !self.gui_open {
            return None;
        }
        if let Some(size) = self.host_context.requested_gui_size() {
            return Some(size);
        }
        let get_size = unsafe { (*self.gui_ext).get_size }?;
        let mut w: u32 = 0;
        let mut h: u32 = 0;
        if unsafe { get_size(self.plugin, &raw mut w, &raw mut h) } {
            Some((w, h))
        } else {
            None
        }
    }

    fn is_resizable(&self) -> bool {
        let Ok(_main_thread) = self.host_context.main_thread_scope() else {
            return false;
        };
        if !self.gui_open {
            return false;
        }
        unsafe { (*self.gui_ext).can_resize }.is_some_and(|f| unsafe { f(self.plugin) })
    }

    fn set_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let _main_thread = self.host_context.main_thread_scope().ok()?;
        if !self.gui_open || width == 0 || height == 0 || !self.is_resizable() {
            return None;
        }
        let mut w = width;
        let mut h = height;
        // SAFETY: the open GUI extension and plugin remain owned by this instance, and the
        // main-thread scope serializes GUI calls. Size outputs live through the callback.
        if let Some(adjust) = unsafe { (*self.gui_ext).adjust_size }
            && !unsafe { adjust(self.plugin, &raw mut w, &raw mut h) }
        {
            return None;
        }
        if w == 0 || h == 0 {
            return None;
        }
        let set = unsafe { (*self.gui_ext).set_size }?;
        // If set_size itself requests another size, that newer request must survive this call.
        self.host_context.clear_gui_size_request();
        if unsafe { set(self.plugin, w, h) } {
            Some((w, h))
        } else {
            None
        }
    }

    fn show(&mut self) {
        if !self.gui_open {
            return;
        }
        let Ok(_main_thread) = self.host_context.main_thread_scope() else {
            return;
        };
        if let Some(show) = unsafe { (*self.gui_ext).show } {
            let _ = unsafe { show(self.plugin) };
        }
    }

    fn hide(&mut self) {
        if !self.gui_open {
            return;
        }
        let Ok(_main_thread) = self.host_context.main_thread_scope() else {
            return;
        };
        if let Some(hide) = unsafe { (*self.gui_ext).hide } {
            let _ = unsafe { hide(self.plugin) };
        }
    }

    fn on_idle(&mut self) {
        if self.gui_open && self.host_context.gui_closed_requested() {
            // closed() can arrive on any thread. Acknowledge it only on the plugin's main thread;
            // the next editor snapshot then closes the native parent window.
            let _ = close_gui_native(
                self.plugin,
                &self.host_context,
                self.gui_ext,
                &mut self.gui_open,
            );
        }
    }
}

impl Plugin<f32> for ClapPlugin {
    fn process(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        events: &EventList,
        context: &mut ProcessContext<'_>,
    ) -> Result<ProcessStatus> {
        if !self.is_active() {
            return Err(Error::NotActivated);
        }
        if !self
            .process_pointers
            .as_ref()
            .is_some_and(|pointers| pointers.matches_buffer(buffer))
        {
            // A mismatched host buffer must not reach start_processing or process, and the
            // real-time rejection path deliberately uses no allocated error message.
            return Ok(ProcessStatus::Error);
        }
        if !self.started_processing {
            let start = unsafe { (*self.plugin).start_processing }
                .ok_or_else(|| Error::Other("clap plugin missing start_processing".into()))?;
            let _audio_thread = self.host_context.audio_thread_scope();
            if !unsafe { start(self.plugin) } {
                return Err(Error::Other(
                    "clap_plugin::start_processing returned false".into(),
                ));
            }
            self.started_processing = true;
        }

        // Translate the truce-rack event list into CLAP's typed event
        // unions. Plus any pending parameter changes from
        // set_parameter. The resulting Vec backs the
        // ConvertedInputEvents callbacks for the duration of the
        // process call.
        // Translate the host transport snapshot up front so its
        // backing struct lives for the whole process call.
        let transport_event = context
            .transport
            .map(|t| build_clap_transport(&t, context.sample_rate));

        let converted =
            ConvertedInputEvents::from_pending_and_rack_events(events, &self.parameter_ids);
        let input_events = converted.as_clap();
        let mut sink = OutputEventsSink::new(Some(context.output_events));
        let output_events = sink.as_clap();

        let num_frames = buffer.num_frames();
        let status =
            self.call_process_block(buffer, transport_event, &input_events, &output_events);

        commit_processed_parameter_changes(&mut self.parameter_ids, status);

        self.steady_time = self
            .steady_time
            .saturating_add(i64::try_from(num_frames).unwrap_or(0));

        Ok(map_clap_status(status))
    }
}

/// Translate a host [`TransportInfo`] snapshot into CLAP's
/// `clap_event_transport`. `sample_rate` converts the sample
/// position into the seconds timeline CLAP also wants.
///
/// CLAP beat / second times are 31.32 fixed-point (see
/// `CLAP_BEATTIME_FACTOR`); beats are quarter notes.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
fn build_clap_transport(t: &TransportInfo, sample_rate: f64) -> clap_event_transport {
    let beats_to_fixed = |b: f64| (b * CLAP_BEATTIME_FACTOR as f64).round() as i64;
    let secs_to_fixed = |s: f64| (s * CLAP_SECTIME_FACTOR as f64).round() as i64;

    let mut flags: clap_transport_flags = 0;
    let tempo = t.tempo_bpm.unwrap_or(0.0);
    if t.tempo_bpm.is_some() {
        flags |= CLAP_TRANSPORT_HAS_TEMPO;
    }
    let song_pos_beats = match t.song_position_beats {
        Some(b) => {
            flags |= CLAP_TRANSPORT_HAS_BEATS_TIMELINE;
            beats_to_fixed(b)
        }
        None => 0,
    };
    let song_pos_seconds = match t.song_position_samples {
        Some(s) => {
            flags |= CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
            secs_to_fixed(s as f64 / sample_rate.max(1.0))
        }
        None => 0,
    };
    let (tsig_num, tsig_denom) = match t.time_signature {
        Some((n, d)) => {
            flags |= CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
            (n as u16, d as u16)
        }
        None => (0, 0),
    };
    let bar_start = t.bar_start_beats.map_or(0, beats_to_fixed);
    // Bar index: bar_start (in quarter notes) divided by the bar
    // length the time signature implies.
    let bar_number = match (t.bar_start_beats, t.time_signature) {
        (Some(bsb), Some((n, d))) => {
            let beats_per_bar = f64::from(n) * 4.0 / f64::from(d.max(1));
            (bsb / beats_per_bar.max(f64::EPSILON)).round() as i32
        }
        _ => 0,
    };
    if t.playing {
        flags |= CLAP_TRANSPORT_IS_PLAYING;
    }
    if t.recording {
        flags |= CLAP_TRANSPORT_IS_RECORDING;
    }
    if t.loop_active {
        flags |= CLAP_TRANSPORT_IS_LOOP_ACTIVE;
    }

    clap_event_transport {
        header: clap_event_header {
            size: u32::try_from(std::mem::size_of::<clap_event_transport>()).unwrap_or(0),
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_TRANSPORT,
            flags: 0,
        },
        flags,
        song_pos_beats,
        song_pos_seconds,
        tempo,
        tempo_inc: 0.0,
        loop_start_beats: 0,
        loop_end_beats: 0,
        loop_start_seconds: 0,
        loop_end_seconds: 0,
        bar_start,
        bar_number,
        tsig_num,
        tsig_denom,
    }
}

fn map_clap_status(status: i32) -> ProcessStatus {
    match status {
        CLAP_PROCESS_CONTINUE | CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => ProcessStatus::Continue,
        CLAP_PROCESS_SLEEP => ProcessStatus::Sleep,
        CLAP_PROCESS_TAIL => ProcessStatus::Tail { tail_samples: 0 },
        _ => ProcessStatus::Error,
    }
}

fn make_param_value_event(sample_offset: u32, param_id: u32, value: f64) -> clap_event_param_value {
    clap_event_param_value {
        header: clap_event_header {
            size: u32::try_from(std::mem::size_of::<clap_event_param_value>()).unwrap_or(0),
            time: sample_offset,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_PARAM_VALUE,
            flags: 0,
        },
        param_id,
        cookie: ptr::null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value,
    }
}

fn make_note_event(
    sample_offset: u32,
    event_type: u16,
    channel: u8,
    key: u8,
    velocity: f64,
) -> clap_event_note {
    clap_event_note {
        header: clap_event_header {
            size: u32::try_from(std::mem::size_of::<clap_event_note>()).unwrap_or(0),
            time: sample_offset,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: event_type,
            flags: 0,
        },
        note_id: -1,
        port_index: -1,
        channel: i16::from(channel),
        key: i16::from(key),
        velocity,
    }
}

fn make_midi_event(sample_offset: u32, bytes: [u8; 3]) -> clap_event_midi {
    clap_event_midi {
        header: clap_event_header {
            size: u32::try_from(std::mem::size_of::<clap_event_midi>()).unwrap_or(0),
            time: sample_offset,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_MIDI,
            flags: 0,
        },
        port_index: 0,
        data: bytes,
    }
}

/// Sink for `clap_plugin_state::save` — the plugin pushes bytes
/// via the C `write` callback, we accumulate into a Vec.
#[derive(Default)]
struct WriteBuffer {
    bytes: Vec<u8>,
}

unsafe extern "C" fn ostream_write(
    stream: *const clap_ostream,
    buffer: *const std::ffi::c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let ctx = unsafe { (*stream).ctx.cast::<WriteBuffer>() };
    if ctx.is_null() {
        return -1;
    }
    let Ok(size_usize) = usize::try_from(size) else {
        return -1;
    };
    let slice = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), size_usize) };
    unsafe { (*ctx).bytes.extend_from_slice(slice) };
    i64::try_from(size_usize).unwrap_or(i64::MAX)
}

/// Source for `clap_plugin_state::load` — the plugin pulls bytes
/// via the C `read` callback, we hand back from a `&[u8]`.
struct ReadCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

unsafe extern "C" fn istream_read(
    stream: *const clap_istream,
    buffer: *mut std::ffi::c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let ctx = unsafe { (*stream).ctx.cast::<ReadCursor<'_>>() };
    if ctx.is_null() {
        return -1;
    }
    let cursor = unsafe { &mut *ctx };
    let Ok(want) = usize::try_from(size) else {
        return -1;
    };
    let available = cursor.bytes.len().saturating_sub(cursor.position);
    let take = want.min(available);
    if take > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                cursor.bytes.as_ptr().add(cursor.position),
                buffer.cast::<u8>(),
                take,
            );
        }
        cursor.position += take;
    }
    i64::try_from(take).unwrap_or(i64::MAX)
}

/// Owned storage for one block's CLAP-formatted input events.
///
/// We can't ship `*const clap_event_header` directly off our
/// `EventList` because the truce-rack event payloads have different
/// layouts than CLAP's. The fix is to translate up front and keep
/// the result alive for the `process` call.
struct ConvertedInputEvents {
    /// Owned event memory — every entry is a CLAP event struct
    /// the input-events vtable hands out pointers into. Boxed so
    /// addresses survive `Vec` reallocation.
    events: Vec<EventStorage>,
}

#[allow(dead_code)]
enum EventStorage {
    Param(clap_event_param_value),
    Note(clap_event_note),
    Midi(clap_event_midi),
}

impl EventStorage {
    fn header(&self) -> *const clap_event_header {
        match self {
            Self::Param(e) => &raw const e.header,
            Self::Note(e) => &raw const e.header,
            Self::Midi(e) => &raw const e.header,
        }
    }
}

fn convert_midi_event(
    offset: u32,
    midi: truce_rack_core::events::MidiData,
) -> Option<EventStorage> {
    use truce_rack_core::events::MidiData;

    Some(match midi {
        MidiData::NoteOn {
            channel,
            note,
            velocity,
        } => EventStorage::Note(make_note_event(
            offset,
            CLAP_EVENT_NOTE_ON,
            channel,
            note,
            f64::from(velocity) / 127.0,
        )),
        MidiData::NoteOff {
            channel,
            note,
            velocity,
        } => EventStorage::Note(make_note_event(
            offset,
            CLAP_EVENT_NOTE_OFF,
            channel,
            note,
            f64::from(velocity) / 127.0,
        )),
        MidiData::ControlChange {
            channel,
            controller,
            value,
        } => {
            let status = 0xB0 | (channel & 0x0F);
            EventStorage::Midi(make_midi_event(
                offset,
                [status, controller & 0x7F, value & 0x7F],
            ))
        }
        MidiData::ProgramChange { channel, program } => {
            let status = 0xC0 | (channel & 0x0F);
            EventStorage::Midi(make_midi_event(offset, [status, program & 0x7F, 0]))
        }
        MidiData::PolyAftertouch {
            channel,
            note,
            pressure,
        } => {
            let status = 0xA0 | (channel & 0x0F);
            EventStorage::Midi(make_midi_event(
                offset,
                [status, note & 0x7F, pressure & 0x7F],
            ))
        }
        MidiData::ChannelAftertouch { channel, pressure } => {
            let status = 0xD0 | (channel & 0x0F);
            EventStorage::Midi(make_midi_event(offset, [status, pressure & 0x7F, 0]))
        }
        MidiData::PitchBend { channel, value } => {
            let status = 0xE0 | (channel & 0x0F);
            let lsb = u8::try_from(value & 0x7F).unwrap_or(0);
            let msb = u8::try_from((value >> 7) & 0x7F).unwrap_or(0);
            EventStorage::Midi(make_midi_event(offset, [status, lsb, msb]))
        }
        MidiData::Raw { len, data } if len >= 3 => {
            EventStorage::Midi(make_midi_event(offset, [data[0], data[1], data[2]]))
        }
        MidiData::Raw { .. } => return None,
    })
}

impl ConvertedInputEvents {
    fn from_pending_and_rack_events(list: &EventList, parameter_ids: &ParameterIds) -> Self {
        let mut out = Self { events: Vec::new() };
        parameter_ids.for_each_pending(|param_id, value| out.push_param_value(0, param_id, value));
        for event in list {
            out.push_rack(event);
        }
        out
    }

    fn push_param_value(&mut self, sample_offset: u32, param_id: u32, value: f64) {
        self.events.push(EventStorage::Param(make_param_value_event(
            sample_offset,
            param_id,
            value,
        )));
    }

    fn push_rack(&mut self, event: &truce_rack_core::events::Event) {
        use truce_rack_core::events::EventBody;
        let offset = event.sample_offset;
        match event.body {
            EventBody::Midi(midi) => self.push_midi(offset, midi),
            EventBody::ParamValue { param_id, value } => {
                self.push_param_value(offset, param_id, value);
            }
            EventBody::ParamGesture { .. } | EventBody::TransportFlag(_) => {
                // ParamGesture: CLAP's begin/end events have
                // separate types; covered in a follow-on once
                // hosts start emitting them.
                // TransportFlag: routed through clap_event_transport
                // on the process struct, not via input events.
            }
        }
    }

    fn push_midi(&mut self, offset: u32, midi: truce_rack_core::events::MidiData) {
        if let Some(event) = convert_midi_event(offset, midi) {
            self.events.push(event);
        }
    }

    fn as_clap(&self) -> clap_input_events {
        clap_input_events {
            ctx: std::ptr::from_ref::<Self>(self)
                .cast::<std::ffi::c_void>()
                .cast_mut(),
            size: Some(input_events_size),
            get: Some(input_events_get),
        }
    }
}

unsafe extern "C" fn input_events_size(list: *const clap_input_events) -> u32 {
    let ctx = unsafe { (*list).ctx.cast::<ConvertedInputEvents>() };
    if ctx.is_null() {
        return 0;
    }
    u32::try_from(unsafe { (*ctx).events.len() }).unwrap_or(u32::MAX)
}

unsafe extern "C" fn input_events_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let ctx = unsafe { (*list).ctx.cast::<ConvertedInputEvents>() };
    if ctx.is_null() {
        return ptr::null();
    }
    let idx = index as usize;
    let events = unsafe { &(*ctx).events };
    events.get(idx).map_or(ptr::null(), EventStorage::header)
}

/// Sink for events the plugin emits during `process`.
struct OutputEventsSink<'a> {
    target: Option<&'a mut EventList>,
}

impl<'a> OutputEventsSink<'a> {
    fn new(target: Option<&'a mut EventList>) -> Self {
        Self { target }
    }

    fn as_clap(&mut self) -> clap_output_events {
        clap_output_events {
            ctx: std::ptr::from_mut::<Self>(self).cast::<std::ffi::c_void>(),
            try_push: Some(output_events_try_push),
        }
    }
}

unsafe extern "C" fn output_events_try_push(
    list: *const clap_output_events,
    event: *const clap_event_header,
) -> bool {
    if event.is_null() {
        return false;
    }
    let header = unsafe { &*event };
    let ctx = unsafe { (*list).ctx.cast::<OutputEventsSink<'_>>() };
    if ctx.is_null() {
        return true;
    }
    let target = unsafe { (*ctx).target.as_deref_mut() };
    let Some(target) = target else {
        // Plugin wanted to send an event; the host doesn't care
        // (Sink was constructed with `target: None`). Returning
        // true tells the plugin we accepted it.
        return true;
    };
    if let Some(rack_event) = unsafe { clap_event_to_rack(header) } {
        target.push(rack_event);
    }
    true
}

unsafe fn clap_event_to_rack(header: &clap_event_header) -> Option<truce_rack_core::events::Event> {
    use truce_rack_core::events::{Event, EventBody, MidiData};
    if header.space_id != CLAP_CORE_EVENT_SPACE_ID {
        return None;
    }
    let offset = header.time;
    match header.type_ {
        t if t == CLAP_EVENT_PARAM_VALUE => {
            let e: &clap_event_param_value =
                unsafe { &*std::ptr::from_ref::<clap_event_header>(header).cast() };
            Some(Event {
                sample_offset: offset,
                body: EventBody::ParamValue {
                    param_id: e.param_id,
                    value: e.value,
                },
            })
        }
        t if t == CLAP_EVENT_NOTE_ON => {
            let e: &clap_event_note =
                unsafe { &*std::ptr::from_ref::<clap_event_header>(header).cast() };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let velocity = (e.velocity * 127.0).round().clamp(0.0, 127.0) as u8;
            Some(Event {
                sample_offset: offset,
                body: EventBody::Midi(MidiData::NoteOn {
                    channel: u8::try_from(e.channel.max(0)).unwrap_or(0),
                    note: u8::try_from(e.key.max(0)).unwrap_or(0),
                    velocity,
                }),
            })
        }
        t if t == CLAP_EVENT_NOTE_OFF => {
            let e: &clap_event_note =
                unsafe { &*std::ptr::from_ref::<clap_event_header>(header).cast() };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let velocity = (e.velocity * 127.0).round().clamp(0.0, 127.0) as u8;
            Some(Event {
                sample_offset: offset,
                body: EventBody::Midi(MidiData::NoteOff {
                    channel: u8::try_from(e.channel.max(0)).unwrap_or(0),
                    note: u8::try_from(e.key.max(0)).unwrap_or(0),
                    velocity,
                }),
            })
        }
        t if t == CLAP_EVENT_MIDI => {
            let e: &clap_event_midi =
                unsafe { &*std::ptr::from_ref::<clap_event_header>(header).cast() };
            Some(Event {
                sample_offset: offset,
                body: EventBody::Midi(MidiData::Raw {
                    len: 3,
                    data: [e.data[0], e.data[1], e.data[2], 0, 0, 0, 0, 0],
                }),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_sys::entry::clap_plugin_entry;
    use clap_sys::ext::gui::clap_plugin_gui;
    use clap_sys::ext::thread_check::{CLAP_EXT_THREAD_CHECK, clap_host_thread_check};
    use clap_sys::ext::timer_support::clap_host_timer_support;
    use clap_sys::host::clap_host;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
    use truce_rack_core::buffer::BusRange;

    const PROCESS_TEST_FRAMES: usize = 64;
    const PROCESS_BASELINE_BLOCKS: usize = 10_000;

    #[test]
    fn editor_parent_requires_the_platform_api() {
        use truce_rack_core::editor::WindowHandle;

        let pointer = std::ptr::NonNull::<u8>::dangling().as_ptr().cast();
        for (handle, expected_api) in [
            (WindowHandle::NSView(pointer), CLAP_WINDOW_API_COCOA),
            (WindowHandle::HWND(pointer), CLAP_WINDOW_API_WIN32),
            (WindowHandle::X11(u64::from(u32::MAX)), CLAP_WINDOW_API_X11),
        ] {
            let result = handle_to_clap_window(handle);
            if expected_api != platform_api() {
                assert!(result.is_err());
                continue;
            }
            let window = result.expect("matching native parent API");
            // SAFETY: conversion initializes exactly the union member selected
            // by this handle; pointer values are compared, never dereferenced.
            unsafe {
                match handle {
                    WindowHandle::NSView(_) => assert_eq!(window.specific.cocoa, pointer),
                    WindowHandle::HWND(_) => assert_eq!(window.specific.win32, pointer),
                    WindowHandle::X11(id) => {
                        assert_eq!(u128::from(window.specific.x11), u128::from(id));
                    }
                }
            }
        }
    }

    #[cfg(any(target_os = "windows", target_pointer_width = "32"))]
    #[test]
    fn oversized_x11_parent_is_rejected_without_truncation() {
        let result = handle_to_clap_window(truce_rack_core::editor::WindowHandle::X11(
            u64::from(u32::MAX) + 1,
        ));

        assert!(matches!(result, Err(Error::Other(message)) if message.contains("ABI width")));
    }

    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn x11_parent_preserves_the_native_unsigned_long_width() {
        let window = handle_to_clap_window(truce_rack_core::editor::WindowHandle::X11(u64::MAX))
            .expect("64-bit native X11 handle");

        // SAFETY: the X11 input initializes the x11 union member.
        assert_eq!(unsafe { window.specific.x11 }, u64::MAX);
    }

    #[test]
    fn c_buffer_conversion_preserves_bytes_and_stops_at_nul() {
        let buffer = [
            c_char::from_ne_bytes([0xC3]),
            c_char::from_ne_bytes([0xA9]),
            c_char::from_ne_bytes([0]),
            c_char::from_ne_bytes([0xFF]),
        ];

        assert_eq!(c_buf_to_string(&buffer), "é");
    }

    struct ProcessPointerState {
        expected_inputs: Vec<Vec<*mut f32>>,
        expected_outputs: Vec<Vec<*mut f32>>,
        process_status: i32,
        calls: AtomicUsize,
        valid: AtomicBool,
        last_status: AtomicI32,
    }

    impl ProcessPointerState {
        fn validate(&self, block: &clap_process) -> bool {
            let expected_steady_time = i64::try_from(self.calls.load(Ordering::Relaxed))
                .unwrap_or(i64::MAX)
                .saturating_mul(i64::try_from(PROCESS_TEST_FRAMES).unwrap_or(i64::MAX));
            if block.steady_time != expected_steady_time
                || block.frames_count != u32::try_from(PROCESS_TEST_FRAMES).unwrap_or(u32::MAX)
                || !block.transport.is_null()
                || usize::try_from(block.audio_inputs_count).unwrap_or(usize::MAX)
                    != self.expected_inputs.len()
                || usize::try_from(block.audio_outputs_count).unwrap_or(usize::MAX)
                    != self.expected_outputs.len()
                || block.in_events.is_null()
                || block.out_events.is_null()
            {
                return false;
            }

            // SAFETY: this boundary fixture keeps both event records alive around each native
            // call; the input callback is configured to expose no events in the baseline.
            let input_event_count = unsafe {
                let events = &*block.in_events;
                events.size.map_or(u32::MAX, |size| size(block.in_events))
            };
            if input_event_count != 0 {
                return false;
            }

            // SAFETY: this synchronous fixture keeps the records and pointed-to slices live
            // through the native callback.
            unsafe {
                audio_bus_channel_pointers_match(block.audio_inputs, &self.expected_inputs)
                    && audio_bus_channel_pointers_match(
                        block.audio_outputs.cast_const(),
                        &self.expected_outputs,
                    )
            }
        }
    }

    unsafe fn audio_bus_channel_pointers_match(
        audio: *const clap_audio_buffer,
        expected_buses: &[Vec<*mut f32>],
    ) -> bool {
        if expected_buses.is_empty() {
            return audio.is_null();
        }
        if audio.is_null() {
            return false;
        }
        for (bus_index, expected_channels) in expected_buses.iter().enumerate() {
            // SAFETY: the synchronous fixture supplies one live record per expected bus.
            let bus = unsafe { &*audio.add(bus_index) };
            if usize::try_from(bus.channel_count).unwrap_or(usize::MAX) != expected_channels.len()
                || bus.data32.is_null()
                || !bus.data64.is_null()
                || bus.latency != 0
                || bus.constant_mask != 0
            {
                return false;
            }
            // SAFETY: count validation established one live slot per fixture channel.
            for (channel_index, expected) in expected_channels.iter().enumerate() {
                if unsafe { *bus.data32.add(channel_index) } != *expected {
                    return false;
                }
            }
        }
        true
    }

    struct ProcessPointerFixture {
        inputs: Vec<Vec<f32>>,
        outputs: Vec<Vec<f32>>,
        input_bus_channels: Vec<usize>,
        output_bus_channels: Vec<usize>,
        process_status: i32,
    }

    struct ProcessPointerMeasurement {
        allocations: allocation_counter::AllocationInfo,
        calls: usize,
        valid: bool,
        scratch_cleared: bool,
        last_status: i32,
    }

    impl ProcessPointerFixture {
        fn measure_blocks(
            &mut self,
            layout: &BusLayout,
            block_count: usize,
        ) -> ProcessPointerMeasurement {
            let expected_inputs =
                channel_pointer_groups(&mut self.inputs, &self.input_bus_channels);
            let expected_outputs =
                channel_pointer_groups(&mut self.outputs, &self.output_bus_channels);
            let mut state = ProcessPointerState {
                expected_inputs,
                expected_outputs,
                process_status: self.process_status,
                calls: AtomicUsize::new(0),
                valid: AtomicBool::new(true),
                last_status: AtomicI32::new(CLAP_PROCESS_ERROR),
            };
            // SAFETY: zero is valid for this inert CLAP callback table; plugin_data and the
            // process callback are set before this local plugin is passed to the native helper.
            let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
            plugin.plugin_data = (&raw mut state).cast();
            plugin.process = Some(process_pointer_fixture_process);
            let host_context = HostContext::new();

            let input_channels: Vec<&[f32]> = self.inputs.iter().map(Vec::as_slice).collect();
            let mut output_channels: Vec<&mut [f32]> =
                self.outputs.iter_mut().map(Vec::as_mut_slice).collect();
            let input_ranges = bus_ranges(&self.input_bus_channels);
            let output_ranges = bus_ranges(&self.output_bus_channels);
            let mut pointers = ProcessChannelPointers::for_layout(layout).unwrap();
            let mut buffer = AudioBuffer::new(
                &input_channels,
                &mut output_channels,
                PROCESS_TEST_FRAMES,
                &input_ranges,
                &output_ranges,
            );
            let (input_events, output_events) = empty_process_event_records();
            let state = &state;
            let host_context = &host_context;
            let plugin = &raw const plugin;

            let warmup = run_native_process_block(
                NativeProcessTarget {
                    plugin,
                    host_context,
                    steady_time: 0,
                    pointers: &mut pointers,
                },
                &mut buffer,
                None,
                &input_events,
                &output_events,
            );
            state
                .valid
                .store(warmup == CLAP_PROCESS_CONTINUE, Ordering::Relaxed);
            state.last_status.store(warmup, Ordering::Relaxed);

            let allocations = allocation_counter::measure(|| {
                for block_index in 0..block_count {
                    let steady_time = i64::try_from(block_index + 1)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(i64::try_from(PROCESS_TEST_FRAMES).unwrap_or(i64::MAX));
                    let status = run_native_process_block(
                        NativeProcessTarget {
                            plugin,
                            host_context,
                            steady_time,
                            pointers: &mut pointers,
                        },
                        &mut buffer,
                        None,
                        &input_events,
                        &output_events,
                    );
                    state
                        .valid
                        .fetch_and(status == CLAP_PROCESS_CONTINUE, Ordering::Relaxed);
                    state.last_status.store(status, Ordering::Relaxed);
                }
            });
            ProcessPointerMeasurement {
                allocations,
                calls: state.calls.load(Ordering::Relaxed),
                valid: state.valid.load(Ordering::Relaxed),
                scratch_cleared: pointers_are_clear(&pointers),
                last_status: state.last_status.load(Ordering::Relaxed),
            }
        }

        fn outputs_match_inputs(&self) -> bool {
            self.inputs == self.outputs
        }
    }

    fn pointers_are_clear(pointers: &ProcessChannelPointers) -> bool {
        pointers
            .input_channels
            .iter()
            .chain(&pointers.output_channels)
            .flatten()
            .all(|pointer| pointer.is_null())
            && pointers
                .input_buffers
                .iter()
                .chain(&pointers.output_buffers)
                .all(|buffer| buffer.data32.is_null())
    }

    fn channel_pointer_groups(
        channels: &mut [Vec<f32>],
        bus_channel_counts: &[usize],
    ) -> Vec<Vec<*mut f32>> {
        let mut channel_index = 0;
        bus_channel_counts
            .iter()
            .map(|&bus_channel_count| {
                let start = channel_index;
                channel_index += bus_channel_count;
                channels[start..channel_index]
                    .iter_mut()
                    .map(Vec::as_mut_ptr)
                    .collect()
            })
            .collect()
    }

    fn bus_ranges(bus_channel_counts: &[usize]) -> Vec<BusRange> {
        let mut start = 0;
        bus_channel_counts
            .iter()
            .map(|&channel_count| {
                let range = BusRange::new(start, channel_count);
                start += channel_count;
                range
            })
            .collect()
    }

    unsafe extern "C" fn empty_process_input_size(_events: *const clap_input_events) -> u32 {
        0
    }

    unsafe extern "C" fn empty_process_input_get(
        _events: *const clap_input_events,
        _index: u32,
    ) -> *const clap_event_header {
        ptr::null()
    }

    fn empty_process_event_records() -> (clap_input_events, clap_output_events) {
        (
            clap_input_events {
                ctx: ptr::null_mut(),
                size: Some(empty_process_input_size),
                get: Some(empty_process_input_get),
            },
            clap_output_events {
                ctx: ptr::null_mut(),
                try_push: None,
            },
        )
    }

    unsafe extern "C" fn process_pointer_fixture_process(
        plugin: *const clap_plugin,
        process: *const clap_process,
    ) -> i32 {
        // SAFETY: plugin_data is set to the live state and the process record is supplied by
        // the synchronous production boundary call measured by this test.
        let state = unsafe { &*(*plugin).plugin_data.cast::<ProcessPointerState>() };
        if process.is_null() {
            state.valid.store(false, Ordering::Relaxed);
            return CLAP_PROCESS_ERROR;
        }
        // SAFETY: process is checked non-null and remains live until this callback returns.
        let block = unsafe { &*process };
        if !state.validate(block) {
            state.valid.store(false, Ordering::Relaxed);
            return CLAP_PROCESS_ERROR;
        }
        if !block.audio_outputs.is_null() {
            // SAFETY: validate proved each pointer matches the live test buffers; the helper
            // retains those buffers for this complete callback.
            unsafe {
                for output_bus_index in 0..usize::try_from(block.audio_outputs_count).unwrap_or(0) {
                    let output_bus = &mut *block.audio_outputs.add(output_bus_index);
                    let input_bus = if output_bus_index
                        < usize::try_from(block.audio_inputs_count).unwrap_or(0)
                    {
                        Some(&*block.audio_inputs.add(output_bus_index))
                    } else {
                        None
                    };
                    for channel_index in 0..usize::try_from(output_bus.channel_count).unwrap_or(0) {
                        let output = *output_bus.data32.add(channel_index);
                        let input = input_bus
                            .filter(|bus| {
                                channel_index < usize::try_from(bus.channel_count).unwrap_or(0)
                            })
                            .map_or(ptr::null_mut(), |bus| *bus.data32.add(channel_index));
                        for frame in 0..PROCESS_TEST_FRAMES {
                            if input.is_null() {
                                *output.add(frame) = 0.0;
                            } else {
                                *output.add(frame) = *input.add(frame);
                            }
                        }
                    }
                }
            }
        }
        state.calls.fetch_add(1, Ordering::Relaxed);
        state.process_status
    }

    fn process_pointer_fixture(
        input_channels: usize,
        output_channels: usize,
    ) -> ProcessPointerFixture {
        ProcessPointerFixture {
            inputs: (0..input_channels)
                .map(|_| vec![0.25_f32; PROCESS_TEST_FRAMES])
                .collect(),
            outputs: (0..output_channels)
                .map(|_| vec![0.0_f32; PROCESS_TEST_FRAMES])
                .collect(),
            input_bus_channels: if input_channels == 0 {
                Vec::new()
            } else {
                vec![input_channels]
            },
            output_bus_channels: if output_channels == 0 {
                Vec::new()
            } else {
                vec![output_channels]
            },
            process_status: CLAP_PROCESS_CONTINUE,
        }
    }

    fn process_layout(input_channels: usize, output_channels: usize) -> BusLayout {
        use truce_rack_core::bus::{Bus, ChannelConfig};

        let channel_config = |channels| match channels {
            1 => ChannelConfig::Mono,
            2 => ChannelConfig::Stereo,
            channels => ChannelConfig::Discrete(u32::try_from(channels).unwrap()),
        };
        let mut layout = BusLayout::new();
        if input_channels > 0 {
            layout
                .inputs
                .push(Bus::main("Input", channel_config(input_channels)));
        }
        if output_channels > 0 {
            layout
                .outputs
                .push(Bus::main("Output", channel_config(output_channels)));
        }
        layout
    }

    fn stereo_sidechain_layout() -> BusLayout {
        use truce_rack_core::bus::{Bus, ChannelConfig};

        let mut layout = BusLayout::new();
        layout.inputs.push(Bus::main("Main", ChannelConfig::Stereo));
        layout
            .inputs
            .push(Bus::sidechain("Sidechain", ChannelConfig::Stereo));
        layout
            .outputs
            .push(Bus::main("Output", ChannelConfig::Stereo));
        layout
    }

    struct ProcessCallCounters {
        starts: AtomicUsize,
        processes: AtomicUsize,
    }

    unsafe extern "C" fn counted_start_processing(plugin: *const clap_plugin) -> bool {
        // SAFETY: plugin_data points to the live counter fixture used by this test.
        let counters = unsafe { &*(*plugin).plugin_data.cast::<ProcessCallCounters>() };
        counters.starts.fetch_add(1, Ordering::Relaxed);
        true
    }

    unsafe extern "C" fn counted_process(
        plugin: *const clap_plugin,
        _process: *const clap_process,
    ) -> i32 {
        // SAFETY: plugin_data points to the live counter fixture used by this test.
        let counters = unsafe { &*(*plugin).plugin_data.cast::<ProcessCallCounters>() };
        counters.processes.fetch_add(1, Ordering::Relaxed);
        CLAP_PROCESS_CONTINUE
    }

    fn current_process_library() -> LoadedLibrary {
        #[cfg(unix)]
        let library = libloading::os::unix::Library::this();
        #[cfg(windows)]
        let library = libloading::os::windows::Library::this().unwrap();
        LoadedLibrary {
            library: library.into(),
        }
    }

    fn active_process_test_instance(plugin: &clap_plugin, layout: BusLayout) -> ClapPlugin {
        let process_pointers = ProcessChannelPointers::for_layout(&layout).unwrap();
        ClapPlugin {
            info: PluginInfo {
                name: "process test".into(),
                vendor: "test".into(),
                version: 1,
                category: PluginCategory::Effect,
                path: PathBuf::new(),
                unique_id: "test.process".into(),
                format: FORMAT,
                has_editor: false,
                accepts_midi: false,
            },
            audio_port_catalog: AudioPortCatalog::test_catalog(vec![layout.clone()]),
            active_layout: Some(layout),
            plugin: std::ptr::from_ref(plugin),
            host_context: HostContext::new(),
            library: current_process_library(),
            bundle_path: PathBuf::new(),
            started_processing: false,
            activation: None,
            completed_restart_generation: 0,
            completed_latency_generation: 0,
            parameter_ids: ParameterIds::empty(),
            parameters_changed_pending: false,
            process_pointers: Some(process_pointers),
            params_ext: ptr::null(),
            state_ext: ptr::null(),
            gui_ext: ptr::null(),
            timer_ext: ptr::null(),
            latency_ext: ptr::null(),
            latency_frames: Some(0),
            gui_open: false,
            steady_time: 0,
        }
    }

    #[test]
    fn main_thread_service_delivers_timers_without_an_open_editor() {
        struct TimerCalls {
            count: Cell<usize>,
            last_id: Cell<clap_sys::id::clap_id>,
        }
        unsafe extern "C" fn on_timer(plugin: *const clap_plugin, timer_id: clap_sys::id::clap_id) {
            // SAFETY: plugin_data points to TimerCalls retained by this test through servicing.
            let calls = unsafe { &*(*plugin).plugin_data.cast::<TimerCalls>() };
            calls.count.set(calls.count.get() + 1);
            calls.last_id.set(timer_id);
        }

        let calls = TimerCalls {
            count: Cell::new(0),
            last_id: Cell::new(clap_sys::id::CLAP_INVALID_ID),
        };
        // SAFETY: all-zero initializes the nullable pointers and optional callback fields.
        let mut native: clap_plugin = unsafe { std::mem::zeroed() };
        native.plugin_data = std::ptr::from_ref(&calls).cast_mut().cast();
        let mut instance = active_process_test_instance(&native, process_layout(2, 2));
        assert!(!instance.gui_open);

        let host = instance.host_context.host_ptr();
        // SAFETY: host extension lookup uses the stable CLAP timer-support identifier.
        let timer_host =
            unsafe { ((*host).get_extension.unwrap())(host, CLAP_EXT_TIMER_SUPPORT.as_ptr()) }
                .cast::<clap_host_timer_support>();
        let mut timer_id = clap_sys::id::CLAP_INVALID_ID;
        // SAFETY: the host remains live on its fixed main thread for this registration.
        assert!(unsafe { ((*timer_host).register_timer.unwrap())(host, 33, &raw mut timer_id) });
        let timer_extension = clap_plugin_timer_support {
            on_timer: Some(on_timer),
        };
        instance.timer_ext = &raw const timer_extension;

        // Timer support has a 33 ms minimum; allow one period to elapse before the owner tick.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            instance.service_main_thread().unwrap(),
            MainThreadUpdate::Unchanged
        );
        assert_eq!(calls.count.get(), 1);
        assert_eq!(calls.last_id.get(), timer_id);
    }

    #[test]
    fn editor_scale_respects_logical_pixels_and_closed_gui_ignores_visibility_calls() {
        use truce_rack_core::{PluginEditor, WindowHandle};

        struct Calls {
            scale: Cell<usize>,
            visibility: Cell<usize>,
        }
        unsafe extern "C" fn create(_: *const clap_plugin, _: *const c_char, _: bool) -> bool {
            true
        }
        unsafe extern "C" fn destroy(_: *const clap_plugin) {}
        unsafe extern "C" fn parent(_: *const clap_plugin, _: *const clap_window) -> bool {
            true
        }
        unsafe extern "C" fn scale(plugin: *const clap_plugin, _: f64) -> bool {
            // SAFETY: this test's live plugin_data points to Calls; all calls are serialized.
            let calls = unsafe { &*(*plugin).plugin_data.cast::<Calls>() };
            calls.scale.set(calls.scale.get() + 1);
            true
        }
        unsafe extern "C" fn visibility(plugin: *const clap_plugin) -> bool {
            // SAFETY: this test's live plugin_data points to Calls; all calls are serialized.
            let calls = unsafe { &*(*plugin).plugin_data.cast::<Calls>() };
            calls.visibility.set(calls.visibility.get() + 1);
            true
        }
        let calls = Calls {
            scale: Cell::new(0),
            visibility: Cell::new(0),
        };
        // SAFETY: CLAP callback fields are optional and all other fields accept null pointers.
        let mut native: clap_plugin = unsafe { std::mem::zeroed() };
        native.plugin_data = std::ptr::from_ref(&calls).cast_mut().cast();
        // SAFETY: GUI fields are optional function pointers; absent callbacks are valid.
        let mut gui: clap_plugin_gui = unsafe { std::mem::zeroed() };
        gui.create = Some(create);
        gui.destroy = Some(destroy);
        gui.set_parent = Some(parent);
        gui.set_scale = Some(scale);
        gui.show = Some(visibility);
        gui.hide = Some(visibility);
        let mut instance = active_process_test_instance(&native, process_layout(2, 2));
        instance.gui_ext = &raw const gui;
        instance.show();
        instance.hide();
        assert_eq!(calls.visibility.get(), 0);
        let pointer = std::ptr::NonNull::<u8>::dangling().as_ptr().cast();
        let parent = if platform_api() == CLAP_WINDOW_API_COCOA {
            WindowHandle::NSView(pointer)
        } else if platform_api() == CLAP_WINDOW_API_WIN32 {
            WindowHandle::HWND(pointer)
        } else {
            WindowHandle::X11(1)
        };
        instance.open(parent, 2.0).unwrap();
        let host = instance.host_context.host_ptr();
        let timer_host =
            unsafe { ((*host).get_extension.unwrap())(host, CLAP_EXT_TIMER_SUPPORT.as_ptr()) }
                .cast::<clap_host_timer_support>();
        let register_timer = unsafe { (*timer_host).register_timer.unwrap() };
        let unregister_timer = unsafe { (*timer_host).unregister_timer.unwrap() };
        let mut timer_id = clap_sys::id::CLAP_INVALID_ID;
        assert!(unsafe { register_timer(host, 33, &raw mut timer_id) });
        assert_eq!(
            calls.scale.get(),
            usize::from(platform_api() != CLAP_WINDOW_API_COCOA)
        );
        instance.hide();
        instance.show();
        instance.close();
        assert!(unsafe { unregister_timer(host, timer_id) });
        let closed_count = calls.visibility.get();
        assert!(closed_count >= 3);
        instance.hide();
        instance.show();
        assert_eq!(calls.visibility.get(), closed_count);
    }

    #[test]
    fn editor_resize_honors_capability_and_adjustment_rejection() {
        use truce_rack_core::PluginEditor;

        struct Resize {
            resizable: bool,
            accepted: bool,
            adjusted: (u32, u32),
            calls: Cell<usize>,
            applied: Cell<Option<(u32, u32)>>,
        }
        unsafe extern "C" fn can_resize(plugin: *const clap_plugin) -> bool {
            // SAFETY: the fixture retains Resize in plugin_data through every GUI call.
            unsafe { &*(*plugin).plugin_data.cast::<Resize>() }.resizable
        }
        unsafe extern "C" fn adjust(
            plugin: *const clap_plugin,
            width: *mut u32,
            height: *mut u32,
        ) -> bool {
            // SAFETY: the fixture retains Resize and the adapter supplies live size outputs.
            unsafe {
                let state = &*(*plugin).plugin_data.cast::<Resize>();
                *width = state.adjusted.0;
                *height = state.adjusted.1;
                state.accepted
            }
        }
        unsafe extern "C" fn set(plugin: *const clap_plugin, width: u32, height: u32) -> bool {
            // SAFETY: the fixture retains Resize; GUI calls are serialized on this thread.
            let state = unsafe { &*(*plugin).plugin_data.cast::<Resize>() };
            state.calls.set(state.calls.get() + 1);
            state.applied.set(Some((width, height)));
            true
        }
        for (resizable, accepted, adjusted, expected) in [
            (false, true, (800, 600), None),
            (true, false, (800, 600), None),
            (true, true, (0, 600), None),
            (true, true, (800, 0), None),
            (true, true, (800, 600), Some((800, 600))),
        ] {
            let state = Resize {
                resizable,
                accepted,
                adjusted,
                calls: Cell::new(0),
                applied: Cell::new(None),
            };
            // SAFETY: CLAP callbacks are nullable; this fixture installs the callbacks it uses.
            let mut native: clap_plugin = unsafe { std::mem::zeroed() };
            native.plugin_data = std::ptr::from_ref(&state).cast_mut().cast();
            // SAFETY: absent GUI callbacks have a valid null representation.
            let mut gui: clap_plugin_gui = unsafe { std::mem::zeroed() };
            gui.can_resize = Some(can_resize);
            gui.adjust_size = Some(adjust);
            gui.set_size = Some(set);
            let mut instance = active_process_test_instance(&native, process_layout(2, 2));
            instance.gui_ext = &raw const gui;
            assert_eq!(instance.set_size(900, 700), None);
            instance.gui_open = true;
            for (width, height) in [(0, 700), (900, 0)] {
                assert_eq!(instance.set_size(width, height), None);
            }
            assert_eq!(state.calls.get(), 0);
            assert_eq!(instance.set_size(900, 700), expected);
            assert_eq!(state.calls.get(), usize::from(expected.is_some()));
            assert_eq!(state.applied.get(), expected);
            instance.gui_open = false;
        }
    }

    mod gui_request_fixture {
        use super::*;

        pub(super) struct GuiCalls {
            pub host: Cell<*const clap_host>,
            pub size: Cell<(u32, u32)>,
            pub resize_accepted: Cell<bool>,
            pub destroy_count: Cell<usize>,
            pub hide_count: Cell<usize>,
        }

        unsafe fn calls<'a>(plugin: *const clap_plugin) -> &'a GuiCalls {
            // SAFETY: the test keeps GuiCalls alive until after the instance is destroyed.
            unsafe { &*(*plugin).plugin_data.cast::<GuiCalls>() }
        }
        unsafe extern "C" fn create(_: *const clap_plugin, _: *const c_char, _: bool) -> bool {
            true
        }
        unsafe extern "C" fn parent(_: *const clap_plugin, _: *const clap_window) -> bool {
            true
        }
        unsafe extern "C" fn show(plugin: *const clap_plugin) -> bool {
            // SAFETY: the live fixture owns its host context through this synchronous callback.
            let calls = unsafe { calls(plugin) };
            let host = calls.host.get();
            let gui = unsafe { (*host).get_extension.unwrap()(host, CLAP_EXT_GUI.as_ptr()) }
                .cast::<clap_sys::ext::gui::clap_host_gui>();
            let accepted = unsafe { (*gui).request_resize.unwrap()(host, 1000, 600) };
            calls.resize_accepted.set(accepted);
            true
        }
        unsafe extern "C" fn get_size(
            plugin: *const clap_plugin,
            width: *mut u32,
            height: *mut u32,
        ) -> bool {
            // SAFETY: the adapter supplies live output pointers and retains the fixture.
            let size = unsafe { calls(plugin) }.size.get();
            unsafe {
                *width = size.0;
                *height = size.1;
            }
            true
        }
        unsafe extern "C" fn set_size(plugin: *const clap_plugin, width: u32, height: u32) -> bool {
            // SAFETY: the fixture remains alive for this serialized GUI call.
            unsafe { calls(plugin) }.size.set((width, height));
            true
        }
        unsafe extern "C" fn can_resize(_: *const clap_plugin) -> bool {
            true
        }
        unsafe extern "C" fn hide(plugin: *const clap_plugin) -> bool {
            // SAFETY: the fixture remains alive for this serialized GUI call.
            let calls = unsafe { calls(plugin) };
            calls.hide_count.set(calls.hide_count.get() + 1);
            true
        }
        unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
            // SAFETY: the fixture remains alive for this serialized GUI call.
            let calls = unsafe { calls(plugin) };
            calls.destroy_count.set(calls.destroy_count.get() + 1);
        }

        pub(super) fn gui() -> clap_plugin_gui {
            // SAFETY: absent optional GUI callbacks have a valid null representation.
            let mut gui: clap_plugin_gui = unsafe { std::mem::zeroed() };
            gui.create = Some(create);
            gui.set_parent = Some(parent);
            gui.show = Some(show);
            gui.get_size = Some(get_size);
            gui.can_resize = Some(can_resize);
            gui.set_size = Some(set_size);
            gui.hide = Some(hide);
            gui.destroy = Some(destroy);
            gui
        }
    }

    #[test]
    fn plugin_resize_request_reaches_editor_size_without_reentering_plugin_gui() {
        use gui_request_fixture::GuiCalls;
        use truce_rack_core::{PluginEditor, WindowHandle};

        let calls = GuiCalls {
            host: Cell::new(ptr::null()),
            size: Cell::new((640, 480)),
            resize_accepted: Cell::new(false),
            destroy_count: Cell::new(0),
            hide_count: Cell::new(0),
        };
        // SAFETY: nullable callbacks and pointers in this fixture may be zero-initialized.
        let mut native: clap_plugin = unsafe { std::mem::zeroed() };
        native.plugin_data = ptr::from_ref(&calls).cast_mut().cast();
        let gui = gui_request_fixture::gui();
        let mut instance = active_process_test_instance(&native, process_layout(2, 2));
        instance.gui_ext = &raw const gui;
        calls.host.set(instance.host_context.host_ptr());
        let pointer = std::ptr::NonNull::<u8>::dangling().as_ptr().cast();
        let parent = if platform_api() == CLAP_WINDOW_API_COCOA {
            WindowHandle::NSView(pointer)
        } else if platform_api() == CLAP_WINDOW_API_WIN32 {
            WindowHandle::HWND(pointer)
        } else {
            WindowHandle::X11(1)
        };

        instance.open(parent, 1.0).unwrap();
        assert!(calls.resize_accepted.get());
        assert_eq!(instance.size(), Some((1000, 600)));
        assert_eq!(calls.size.get(), (640, 480));
        assert_eq!(instance.set_size(700, 500), Some((700, 500)));
        assert_eq!(instance.size(), Some((700, 500)));

        let host = calls.host.get();
        let host_gui = unsafe { (*host).get_extension.unwrap()(host, CLAP_EXT_GUI.as_ptr()) }
            .cast::<clap_sys::ext::gui::clap_host_gui>();
        unsafe { (*host_gui).closed.unwrap()(host, true) };
        instance.on_idle();
        assert!(!instance.is_open());
        assert_eq!(instance.size(), None);
        assert_eq!(calls.hide_count.get(), 0);
        assert_eq!(calls.destroy_count.get(), 1);
        assert!(!unsafe { (*host_gui).request_resize.unwrap()(host, 800, 500) });
    }

    #[test]
    fn production_process_boundary_reuses_prepared_scratch_without_allocating() {
        let layout = process_layout(2, 2);
        let mut fixture = process_pointer_fixture(2, 2);
        let measured = fixture.measure_blocks(&layout, PROCESS_BASELINE_BLOCKS);
        let allocations = measured.allocations;

        assert_eq!(allocations.count_total, 0);
        assert_eq!(allocations.bytes_total, 0);
        assert_eq!(allocations.count_current, 0);
        assert_eq!(allocations.bytes_current, 0);
        assert_eq!(measured.calls, PROCESS_BASELINE_BLOCKS + 1);
        assert_eq!(measured.last_status, CLAP_PROCESS_CONTINUE);
        assert!(measured.valid);
        assert!(measured.scratch_cleared);
        assert!(fixture.outputs_match_inputs());
    }

    #[test]
    fn prepared_pointer_scratch_handles_mono_stereo_and_wide_main_buses() {
        for channel_count in [1, 2, 32, 64] {
            let layout = process_layout(channel_count, channel_count);
            let mut fixture = process_pointer_fixture(channel_count, channel_count);
            let measured = fixture.measure_blocks(&layout, 1);

            assert_eq!(
                measured.allocations.count_total, 0,
                "{channel_count} channels"
            );
            assert_eq!(
                measured.allocations.bytes_total, 0,
                "{channel_count} channels"
            );
            assert_eq!(measured.calls, 2, "{channel_count} channels");
            assert_eq!(measured.last_status, CLAP_PROCESS_CONTINUE);
            assert!(measured.valid, "{channel_count} channels");
            assert!(measured.scratch_cleared, "{channel_count} channels");
            assert!(fixture.outputs_match_inputs(), "{channel_count} channels");
        }
    }

    #[test]
    fn prepared_pointer_scratch_keeps_main_and_sidechain_as_separate_buses() {
        let layout = stereo_sidechain_layout();
        let mut fixture = process_pointer_fixture(4, 2);
        fixture.input_bus_channels = vec![2, 2];
        let measured = fixture.measure_blocks(&layout, 1);

        assert_eq!(measured.allocations.count_total, 0);
        assert_eq!(measured.allocations.bytes_total, 0);
        assert_eq!(measured.calls, 2);
        assert_eq!(measured.last_status, CLAP_PROCESS_CONTINUE);
        assert!(measured.valid);
        assert!(measured.scratch_cleared);
        assert_eq!(&fixture.outputs, &fixture.inputs[..2]);
    }

    #[test]
    fn mismatched_main_bus_channels_fail_without_native_process_or_allocation() {
        let layout = process_layout(2, 2);
        for (input_channels, output_channels) in [(1, 2), (2, 1)] {
            let mut fixture = process_pointer_fixture(input_channels, output_channels);
            let measured = fixture.measure_blocks(&layout, 1);

            assert_eq!(measured.allocations.count_total, 0);
            assert_eq!(measured.calls, 0);
            assert!(!measured.valid);
            assert!(measured.scratch_cleared);
            assert_eq!(measured.last_status, CLAP_PROCESS_ERROR);
        }
    }

    #[test]
    fn actual_process_rejects_layout_mismatch_before_starting_native_processing() {
        let counters = ProcessCallCounters {
            starts: AtomicUsize::new(0),
            processes: AtomicUsize::new(0),
        };
        // SAFETY: nullable CLAP fields are valid as zero; this test installs its live callbacks.
        let mut native_plugin: clap_plugin = unsafe { std::mem::zeroed() };
        native_plugin.plugin_data = (&raw const counters).cast_mut().cast();
        native_plugin.start_processing = Some(counted_start_processing);
        native_plugin.process = Some(counted_process);
        let layout = process_layout(2, 2);
        let mut instance = active_process_test_instance(&native_plugin, layout);

        let input = [0.25_f32; PROCESS_TEST_FRAMES];
        let mut output_left = [0.0_f32; PROCESS_TEST_FRAMES];
        let mut output_right = [0.0_f32; PROCESS_TEST_FRAMES];
        let inputs = [&input[..]];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        let input_ranges = [BusRange::new(0, 1)];
        let output_ranges = [BusRange::new(0, 2)];
        let mut buffer = AudioBuffer::new(
            &inputs,
            &mut outputs,
            PROCESS_TEST_FRAMES,
            &input_ranges,
            &output_ranges,
        );
        let events = EventList::new();
        let mut output_events = EventList::new();
        let mut context = ProcessContext {
            sample_rate: 48_000.0,
            max_block_size: PROCESS_TEST_FRAMES,
            transport: None,
            output_events: &mut output_events,
        };

        let status =
            <ClapPlugin as Plugin<f32>>::process(&mut instance, &mut buffer, &events, &mut context)
                .unwrap();

        assert_eq!(status, ProcessStatus::Error);
        assert_eq!(counters.starts.load(Ordering::Relaxed), 0);
        assert_eq!(counters.processes.load(Ordering::Relaxed), 0);
        instance.active_layout = None;
    }

    #[test]
    fn native_process_error_still_clears_every_pointer_slot() {
        let layout = process_layout(2, 2);
        let mut fixture = process_pointer_fixture(2, 2);
        fixture.process_status = CLAP_PROCESS_ERROR;
        let measured = fixture.measure_blocks(&layout, 1);

        assert_eq!(measured.calls, 2);
        assert_eq!(measured.last_status, CLAP_PROCESS_ERROR);
        assert!(!measured.valid);
        assert!(measured.scratch_cleared);
    }

    #[test]
    fn generator_sink_and_asymmetric_main_buses_forward_only_live_channel_pointers() {
        for (input_channels, output_channels) in [(0, 2), (2, 0), (1, 2), (2, 1)] {
            let layout = process_layout(input_channels, output_channels);
            let mut fixture = process_pointer_fixture(input_channels, output_channels);
            let measured = fixture.measure_blocks(&layout, 1);

            assert_eq!(
                measured.calls, 2,
                "{input_channels} in, {output_channels} out"
            );
            assert_eq!(measured.last_status, CLAP_PROCESS_CONTINUE);
            assert!(measured.valid, "{input_channels} in, {output_channels} out");
            assert!(measured.scratch_cleared);
            for (input, output) in fixture.inputs.iter().zip(&fixture.outputs) {
                assert_eq!(input, output);
            }
            for output in fixture.outputs.iter().skip(fixture.inputs.len()) {
                assert!(output.iter().all(|sample| *sample == 0.0));
            }
        }
    }

    #[test]
    fn pending_parameter_values_precede_ordered_rack_events_without_deduplication() {
        use truce_rack_core::events::{Event, EventBody, MidiData};

        let mut parameter_ids = ParameterIds::collect(1, |_| {
            Ok(ParameterMetadata {
                id: 41,
                requires_process: false,
            })
        })
        .unwrap();
        parameter_ids.queue(0, 0.75).unwrap();

        let mut rack_events = EventList::new();
        rack_events.push(Event {
            sample_offset: 0,
            body: EventBody::ParamValue {
                param_id: 41,
                value: 0.25,
            },
        });
        rack_events.push(Event {
            sample_offset: 8,
            body: EventBody::ParamValue {
                param_id: 42,
                value: 0.5,
            },
        });
        rack_events.push(Event {
            sample_offset: 24,
            body: EventBody::Midi(MidiData::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            }),
        });

        let converted =
            ConvertedInputEvents::from_pending_and_rack_events(&rack_events, &parameter_ids);

        assert_eq!(converted.events.len(), 4);
        assert!(matches!(
            &converted.events[0],
            EventStorage::Param(event)
                if event.header.time == 0 && event.param_id == 41
                    && event.value.to_bits() == 0.75_f64.to_bits()
        ));
        assert!(matches!(
            &converted.events[1],
            EventStorage::Param(event)
                if event.header.time == 0 && event.param_id == 41
                    && event.value.to_bits() == 0.25_f64.to_bits()
        ));
        assert!(matches!(
            &converted.events[2],
            EventStorage::Param(event)
                if event.header.time == 8 && event.param_id == 42
                    && event.value.to_bits() == 0.5_f64.to_bits()
        ));
        assert!(matches!(
            &converted.events[3],
            EventStorage::Note(event)
                if event.header.time == 24
                    && event.header.type_ == CLAP_EVENT_NOTE_ON
                    && event.key == 60
        ));
    }

    struct FakePluginState {
        host: *const clap_host,
        activate_result: AtomicBool,
        latency_changed_on_activate: AtomicBool,
        request_restart_on_activate: AtomicBool,
        active: AtomicBool,
        activate_calls: AtomicUsize,
        latency_reads: AtomicUsize,
        activation_roles_valid: AtomicBool,
        teardown_stage: AtomicUsize,
        teardown_order_valid: AtomicBool,
        host_alive_during_destroy: AtomicBool,
        host_alive_during_deinit: AtomicBool,
    }

    impl FakePluginState {
        fn new(host: *const clap_host) -> Self {
            Self {
                host,
                activate_result: AtomicBool::new(true),
                latency_changed_on_activate: AtomicBool::new(false),
                request_restart_on_activate: AtomicBool::new(false),
                active: AtomicBool::new(false),
                activate_calls: AtomicUsize::new(0),
                latency_reads: AtomicUsize::new(0),
                activation_roles_valid: AtomicBool::new(true),
                teardown_stage: AtomicUsize::new(0),
                teardown_order_valid: AtomicBool::new(true),
                host_alive_during_destroy: AtomicBool::new(false),
                host_alive_during_deinit: AtomicBool::new(false),
            }
        }

        fn roles_are(&self, main: bool, audio: bool) -> bool {
            if self.host.is_null() {
                return false;
            }
            // SAFETY: test state retains the live boxed host context.
            let host = unsafe { &*self.host };
            let Some(get_extension) = host.get_extension else {
                return false;
            };
            // SAFETY: the requested extension pointer is stable in HostContext.
            let raw = unsafe { get_extension(self.host, CLAP_EXT_THREAD_CHECK.as_ptr()) };
            if raw.is_null() {
                return false;
            }
            // SAFETY: get_extension returned a clap_host_thread_check by its registered id.
            let check = unsafe { &*raw.cast::<clap_host_thread_check>() };
            check
                .is_main_thread
                .is_some_and(|f| unsafe { f(self.host) })
                == main
                && check
                    .is_audio_thread
                    .is_some_and(|f| unsafe { f(self.host) })
                    == audio
        }

        fn record_stage(&self, expected: usize) {
            let actual = self.teardown_stage.fetch_add(1, Ordering::SeqCst) + 1;
            if actual != expected {
                self.teardown_order_valid.store(false, Ordering::Relaxed);
            }
        }
    }

    thread_local! {
        static ENTRY_DEINIT_STATE: Cell<*const FakePluginState> = const { Cell::new(ptr::null()) };
    }

    /// # Safety
    /// The pointer must reference a live test plugin whose `plugin_data` points to live state.
    unsafe fn fake_plugin_state<'a>(plugin: *const clap_plugin) -> Option<&'a FakePluginState> {
        if plugin.is_null() {
            return None;
        }
        // SAFETY: the test owns this pointer and keeps its state alive around all fake callbacks.
        unsafe { (*plugin).plugin_data.cast::<FakePluginState>().as_ref() }
    }

    unsafe extern "C" fn fake_activate(
        plugin: *const clap_plugin,
        _sample_rate: f64,
        _min_frames: u32,
        _max_frames: u32,
    ) -> bool {
        // SAFETY: test fixture's plugin_data points to its live state.
        let Some(state) = (unsafe { fake_plugin_state(plugin) }) else {
            return false;
        };
        state.activate_calls.fetch_add(1, Ordering::Relaxed);
        state
            .activation_roles_valid
            .store(state.roles_are(true, false), Ordering::Relaxed);
        // SAFETY: host pointer and callbacks are owned by HostContext for this test.
        let Some(host) = (unsafe { state.host.as_ref() }) else {
            return false;
        };
        if state.latency_changed_on_activate.load(Ordering::Relaxed)
            && let Some(get_extension) = host.get_extension
        {
            // SAFETY: host extension lookup uses a CLAP constant id and stable host storage.
            let raw = unsafe { get_extension(state.host, CLAP_EXT_LATENCY.as_ptr()) };
            if !raw.is_null() {
                // SAFETY: the returned extension matches clap.latency.
                let latency = unsafe { &*raw.cast::<clap_sys::ext::latency::clap_host_latency>() };
                if let Some(changed) = latency.changed {
                    unsafe { changed(state.host) };
                }
            }
        }
        if state.request_restart_on_activate.load(Ordering::Relaxed)
            && let Some(request_restart) = host.request_restart
        {
            unsafe { request_restart(state.host) };
        }
        let activated = state.activate_result.load(Ordering::Relaxed);
        state.active.store(activated, Ordering::Relaxed);
        activated
    }

    unsafe extern "C" fn fake_latency_get(plugin: *const clap_plugin) -> u32 {
        // SAFETY: test fixture's plugin_data points to its live state.
        let Some(state) = (unsafe { fake_plugin_state(plugin) }) else {
            return 0;
        };
        if !state.active.load(Ordering::Relaxed) || !state.roles_are(true, false) {
            state.activation_roles_valid.store(false, Ordering::Relaxed);
        }
        u32::try_from(state.latency_reads.fetch_add(1, Ordering::Relaxed)).unwrap() + 64
    }

    unsafe extern "C" fn fake_stop_processing(plugin: *const clap_plugin) {
        // SAFETY: test fixture's plugin_data points to its live state.
        if let Some(state) = unsafe { fake_plugin_state(plugin) } {
            if !state.roles_are(true, true) {
                state.teardown_order_valid.store(false, Ordering::Relaxed);
            }
            state.record_stage(3);
        }
    }

    unsafe extern "C" fn fake_deactivate(plugin: *const clap_plugin) {
        // SAFETY: test fixture's plugin_data points to its live state.
        if let Some(state) = unsafe { fake_plugin_state(plugin) } {
            if !state.roles_are(true, false) {
                state.teardown_order_valid.store(false, Ordering::Relaxed);
            }
            state.active.store(false, Ordering::Relaxed);
            state.record_stage(4);
        }
    }

    unsafe extern "C" fn fake_hide(plugin: *const clap_plugin) -> bool {
        // SAFETY: test fixture's plugin_data points to its live state.
        if let Some(state) = unsafe { fake_plugin_state(plugin) } {
            if !state.roles_are(true, false) {
                state.teardown_order_valid.store(false, Ordering::Relaxed);
            }
            state.record_stage(1);
        }
        true
    }

    unsafe extern "C" fn fake_gui_destroy(plugin: *const clap_plugin) {
        // SAFETY: test fixture's plugin_data points to its live state.
        if let Some(state) = unsafe { fake_plugin_state(plugin) } {
            if !state.roles_are(true, false) {
                state.teardown_order_valid.store(false, Ordering::Relaxed);
            }
            state.record_stage(2);
        }
    }

    unsafe extern "C" fn fake_plugin_destroy(plugin: *const clap_plugin) {
        // SAFETY: test fixture's plugin_data points to its live state.
        if let Some(state) = unsafe { fake_plugin_state(plugin) } {
            state
                .host_alive_during_destroy
                .store(state.roles_are(true, false), Ordering::Relaxed);
            state.record_stage(5);
        }
    }

    unsafe extern "C" fn fake_entry_deinit() {
        ENTRY_DEINIT_STATE.with(|slot| {
            let state = slot.get();
            // SAFETY: test installs this pointer until after destroy_instance calls deinit.
            if let Some(state) = unsafe { state.as_ref() } {
                state
                    .host_alive_during_deinit
                    .store(state.roles_are(true, false), Ordering::Relaxed);
                state.record_stage(6);
            }
        });
    }

    fn fake_plugin(state: &mut FakePluginState) -> clap_plugin {
        // SAFETY: zero is valid for the nullable pointers and optional callbacks in this C record.
        let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
        plugin.plugin_data = (&raw mut *state).cast();
        plugin.activate = Some(fake_activate);
        plugin.stop_processing = Some(fake_stop_processing);
        plugin.deactivate = Some(fake_deactivate);
        plugin.destroy = Some(fake_plugin_destroy);
        plugin
    }

    fn fake_activation_config() -> ActivationConfig {
        ActivationConfig {
            layout: BusLayout::stereo(),
            sample_rate: 48_000.0,
            max_block_size: 512,
        }
    }

    fn fake_activation_target(
        plugin: *const clap_plugin,
        host_context: &HostContext,
        latency_ext: *const clap_plugin_latency,
    ) -> ActivationTarget<'_> {
        ActivationTarget {
            plugin,
            host_context,
            params_ext: ptr::null(),
            latency_ext,
        }
    }

    #[test]
    fn activation_changed_notification_is_absorbed_but_restart_during_activation_is_preserved() {
        let host = HostContext::new();
        let mut state = FakePluginState::new(host.host_ptr());
        state
            .latency_changed_on_activate
            .store(true, Ordering::Relaxed);
        state
            .request_restart_on_activate
            .store(true, Ordering::Relaxed);
        let plugin = fake_plugin(&mut state);
        let latency = clap_plugin_latency {
            get: Some(fake_latency_get),
        };
        let start_generation = host.restart_generation();
        let mut visible_latency = Some(999);

        let snapshot = fake_activation_target(&raw const plugin, &host, &raw const latency)
            .activate_with_cache(&fake_activation_config(), &mut visible_latency)
            .unwrap();

        assert_eq!(state.activate_calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.latency_reads.load(Ordering::Relaxed), 1);
        assert!(state.activation_roles_valid.load(Ordering::Relaxed));
        assert_eq!(visible_latency, Some(64));
        assert_eq!(
            snapshot.latency_generation,
            host.latency_changed_generation()
        );
        assert_eq!(host.latency_changed_generation(), 1);
        assert_eq!(host.restart_generation(), start_generation + 1);
        assert_eq!(snapshot.parameter_ids.len(), 0);
    }

    #[test]
    fn failed_activation_keeps_latency_unknown_and_skips_native_latency_getter() {
        let host = HostContext::new();
        let mut state = FakePluginState::new(host.host_ptr());
        state.activate_result.store(false, Ordering::Relaxed);
        let plugin = fake_plugin(&mut state);
        let latency = clap_plugin_latency {
            get: Some(fake_latency_get),
        };
        let mut visible_latency = Some(77);

        let result = fake_activation_target(&raw const plugin, &host, &raw const latency)
            .activate_with_cache(&fake_activation_config(), &mut visible_latency);

        assert!(result.is_err());
        assert_eq!(visible_latency, None);
        assert_eq!(state.latency_reads.load(Ordering::Relaxed), 0);
        assert!(!state.active.load(Ordering::Relaxed));
        assert!(!(&raw const plugin).is_null());
    }

    #[test]
    fn failed_activation_with_open_gui_still_allows_ordered_host_and_plugin_cleanup() {
        let host = HostContext::new();
        let host_ptr = host.host_ptr();
        // SAFETY: the host extension lookup uses its stable CLAP timer-support identifier.
        let timer_host = unsafe {
            ((*host_ptr).get_extension.unwrap())(host_ptr, CLAP_EXT_TIMER_SUPPORT.as_ptr())
        }
        .cast::<clap_host_timer_support>();
        let mut timer_id = clap_sys::id::CLAP_INVALID_ID;
        // SAFETY: the host context remains live on its owner thread through ordered teardown.
        assert!(unsafe {
            ((*timer_host).register_timer.unwrap())(host_ptr, 33, &raw mut timer_id)
        });
        let mut state = FakePluginState::new(host.host_ptr());
        state.activate_result.store(false, Ordering::Relaxed);
        let plugin = fake_plugin(&mut state);
        let latency = clap_plugin_latency {
            get: Some(fake_latency_get),
        };
        let mut visible_latency = Some(77);
        assert!(
            fake_activation_target(&raw const plugin, &host, &raw const latency,)
                .activate_with_cache(&fake_activation_config(), &mut visible_latency,)
                .is_err()
        );

        let mut gui: clap_plugin_gui = unsafe { std::mem::zeroed() };
        gui.hide = Some(fake_hide);
        gui.destroy = Some(fake_gui_destroy);
        let entry = clap_plugin_entry {
            clap_version: clap_sys::version::CLAP_VERSION,
            init: None,
            deinit: Some(fake_entry_deinit),
            get_factory: None,
        };
        let mut gui_open = true;
        let mut started_processing = true;
        state.active.store(true, Ordering::Relaxed);
        ENTRY_DEINIT_STATE.with(|slot| slot.set(&raw const state));
        // SAFETY: fixture records, host, and fake callbacks all outlive this ordered teardown.
        unsafe {
            destroy_instance(InstanceDestruction {
                plugin: &raw const plugin,
                entry: &raw const entry,
                host_context: &host,
                gui_ext: &raw const gui,
                gui_open: &mut gui_open,
                started_processing: &mut started_processing,
                active: true,
            });
        }
        ENTRY_DEINIT_STATE.with(|slot| slot.set(ptr::null()));

        assert!(!gui_open);
        assert!(!started_processing);
        assert!(!state.active.load(Ordering::Relaxed));
        assert!(state.teardown_order_valid.load(Ordering::Relaxed));
        assert_eq!(state.teardown_stage.load(Ordering::Relaxed), 6);
        assert!(state.host_alive_during_destroy.load(Ordering::Relaxed));
        assert!(state.host_alive_during_deinit.load(Ordering::Relaxed));
        // SAFETY: the host extension and context remain live after plugin-instance destruction.
        assert!(!unsafe { ((*timer_host).unregister_timer.unwrap())(host_ptr, timer_id) });
        assert!(!(&raw const plugin).is_null());
    }

    struct ParameterFixture {
        get_info_calls: AtomicUsize,
        flush_calls: AtomicUsize,
        flush_event_count: AtomicUsize,
        count: u32,
        fail_index: Option<u32>,
        requires_process_index: Option<u32>,
    }

    unsafe extern "C" fn fake_parameter_count(plugin: *const clap_plugin) -> u32 {
        // SAFETY: test fixture's plugin_data points to the live parameter state.
        unsafe { &*(*plugin).plugin_data.cast::<ParameterFixture>() }.count
    }

    unsafe extern "C" fn fake_parameter_info(
        plugin: *const clap_plugin,
        index: u32,
        info: *mut clap_param_info,
    ) -> bool {
        // SAFETY: test fixture's plugin_data and output record remain live for the call.
        let state = unsafe { &*(*plugin).plugin_data.cast::<ParameterFixture>() };
        state.get_info_calls.fetch_add(1, Ordering::Relaxed);
        if state.fail_index == Some(index) || index >= state.count || info.is_null() {
            return false;
        }
        unsafe {
            (*info).id = 1000 + index;
            if state.requires_process_index == Some(index) {
                (*info).flags |= CLAP_PARAM_REQUIRES_PROCESS;
            }
        }
        true
    }

    unsafe extern "C" fn fake_parameter_flush(
        plugin: *const clap_plugin,
        inputs: *const clap_input_events,
        _outputs: *const clap_output_events,
    ) {
        // SAFETY: the plugin fixture and input event list remain live for this synchronous call.
        let state = unsafe { &*(*plugin).plugin_data.cast::<ParameterFixture>() };
        let input_events = unsafe { &*inputs };
        let count = input_events.size.map_or(0, |size| unsafe { size(inputs) });
        state.flush_calls.fetch_add(1, Ordering::Relaxed);
        state
            .flush_event_count
            .store(count as usize, Ordering::Relaxed);
    }

    #[test]
    fn parameter_id_cache_uses_native_get_info_only_during_main_thread_refresh() {
        let host = HostContext::new();
        let mut fixture = ParameterFixture {
            get_info_calls: AtomicUsize::new(0),
            flush_calls: AtomicUsize::new(0),
            flush_event_count: AtomicUsize::new(0),
            count: 2,
            fail_index: None,
            requires_process_index: Some(1),
        };
        let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
        plugin.plugin_data = (&raw mut fixture).cast();
        let params = clap_plugin_params {
            count: Some(fake_parameter_count),
            get_info: Some(fake_parameter_info),
            get_value: None,
            value_to_text: None,
            text_to_value: None,
            flush: Some(fake_parameter_flush),
        };
        let _main_thread = host.main_thread_scope().unwrap();
        let ids = read_parameter_ids(&raw const plugin, &raw const params).unwrap();
        assert_eq!(ids.id(0).unwrap(), 1000);
        assert_eq!(ids.id(1).unwrap(), 1001);
        assert_eq!(fixture.get_info_calls.load(Ordering::Relaxed), 2);

        let mut ids = ids;
        queue_cached_parameter_change(&mut ids, 0, 0.25).unwrap();
        queue_cached_parameter_change(&mut ids, 1, 0.625).unwrap();
        assert!(ids.has_pending());
        assert!(ids.has_flushable_pending());

        assert!(
            flush_pending_parameter_changes(&raw const plugin, &raw const params, &host, &mut ids,)
                .unwrap()
        );
        assert_eq!(fixture.flush_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fixture.flush_event_count.load(Ordering::Relaxed), 1);

        let mut pending = Vec::new();
        ids.for_each_pending(|id, value| pending.push((id, value)));
        assert_eq!(pending, [(1001, 0.625)]);
        assert_eq!(ids.pending_count(), 1);
        assert_eq!(fixture.get_info_calls.load(Ordering::Relaxed), 2);

        let mut process_delivery = Vec::new();
        ids.for_each_pending(|id, value| process_delivery.push((id, value)));
        assert_eq!(process_delivery, [(1001, 0.625)]);
        commit_processed_parameter_changes(&mut ids, CLAP_PROCESS_ERROR);
        assert!(ids.has_pending());
        commit_processed_parameter_changes(&mut ids, CLAP_PROCESS_CONTINUE);
        assert!(!ids.has_pending());
    }

    #[test]
    fn parameter_id_refresh_failure_is_explicit() {
        let host = HostContext::new();
        let mut fixture = ParameterFixture {
            get_info_calls: AtomicUsize::new(0),
            flush_calls: AtomicUsize::new(0),
            flush_event_count: AtomicUsize::new(0),
            count: 2,
            fail_index: Some(1),
            requires_process_index: None,
        };
        let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
        plugin.plugin_data = (&raw mut fixture).cast();
        let params = clap_plugin_params {
            count: Some(fake_parameter_count),
            get_info: Some(fake_parameter_info),
            get_value: None,
            value_to_text: None,
            text_to_value: None,
            flush: None,
        };
        let _main_thread = host.main_thread_scope().unwrap();
        assert!(matches!(
            read_parameter_ids(&raw const plugin, &raw const params),
            Err(Error::Other(message)) if message.contains("index 1")
        ));
    }

    unsafe extern "C" fn zero_latency(_plugin: *const clap_plugin) -> u32 {
        0
    }

    #[test]
    fn latency_snapshot_distinguishes_absent_extension_from_incomplete_extension() {
        // SAFETY: every field in this C ABI test value is a nullable pointer, integer, or optional
        // function pointer; all-zero is a valid inert plugin record used only as a callback token.
        let plugin: clap_plugin = unsafe { std::mem::zeroed() };
        let plugin_ptr = &raw const plugin;
        // SAFETY: the absent-extension branch does not dereference the valid inert plugin token.
        assert_eq!(
            unsafe { query_latency_frames(plugin_ptr, ptr::null()) },
            Some(0)
        );

        let extension = clap_plugin_latency { get: None };
        assert_eq!(
            // SAFETY: the inert plugin token and stack extension remain live for this call.
            unsafe { query_latency_frames(plugin_ptr, &raw const extension) },
            None
        );

        let extension = clap_plugin_latency {
            get: Some(zero_latency),
        };
        assert_eq!(
            // SAFETY: the callback ignores the valid inert plugin token and returns a scalar.
            unsafe { query_latency_frames(plugin_ptr, &raw const extension) },
            Some(0)
        );
    }

    #[test]
    fn parse_version_components() {
        assert_eq!(parse_version("1.2.3"), (1 << 16) | (2 << 8) | 3);
        assert_eq!(parse_version("0.5"), 5 << 8);
        assert_eq!(parse_version(""), 0);
    }

    #[test]
    fn bundle_binary_macos() {
        let p = bundle_binary_path(Path::new("/tmp/MyPlugin.clap"));
        #[cfg(target_os = "macos")]
        assert!(!p.exists() || p.starts_with("/tmp/MyPlugin.clap"));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(p, Path::new("/tmp/MyPlugin.clap"));
    }
}
