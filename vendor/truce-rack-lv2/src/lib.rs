//! LV2 host implementation for the truce-rack framework.
//!
//! Built on `lilv-sys` — the Rust FFI for **lilv**, the standard
//! LV2 host library. lilv handles URI resolution, TTL parsing, and
//! the Turtle world load that LV2 demands; truce-rack-lv2 is the layer
//! that turns those C-API queries into [`PluginInfo`] entries and
//! drives `lilv_plugin_instantiate` / `lilv_instance_run` from the
//! truce-rack-core trait surface.
//!
//! # Status
//!
//! - **Scan** via `lilv_world_load_all`, or one exact bundle via `lilv_world_load_bundle`.
//! - **Load** instantiates the plugin into a `LilvInstance` with
//!   the LV2 `urid#map` feature. Each plugin owns its own world
//!   (instance pointers reference world-owned data).
//! - **Process** connects audio / control / atom-sequence ports
//!   per block and calls `lilv_instance_run`. Audio ports point
//!   straight at the host's planes (zero-copy); control ports
//!   carry their declared default value; the MIDI input atom port
//!   is rebuilt each block from the truce-rack [`EventList`].
//! - **MIDI** in via atom sequence ports tagged `midi:MidiEvent`.
//!   MIDI out is not yet drained back to the truce-rack host.
//!
//! # Build dependency
//!
//! `lilv-sys` links against the system `lilv-0` library. On macOS,
//! `brew install lilv`. On Debian/Ubuntu, `apt install liblilv-dev`.
//! Without those, `cargo build -p truce-rack-lv2` will fail at link
//! time.

#![allow(
    // Atom struct sizes are well under u32::MAX; the casts are for
    // FFI fields that are themselves u32.
    clippy::cast_possible_truncation,
    // Vec<u8> allocations come from the global allocator with at
    // least 8-byte alignment; safe to cast to atom-header pointers.
    clippy::cast_ptr_alignment,
    // `&x as *const _` reads cleaner here than `&raw const x`.
    clippy::borrow_as_ptr,
    clippy::ref_as_ptr,
    clippy::ptr_as_ptr
)]

use truce_rack_core::buffer::AudioBuffer;
use truce_rack_core::bus::BusLayout;
use truce_rack_core::editor::{PluginEditor, WindowHandle};
use truce_rack_core::error::{Error, Result};
use truce_rack_core::events::{Event, EventBody, EventList, MidiData};
use truce_rack_core::info::{ParameterInfo, PluginCategory, PluginInfo, PresetInfo};
use truce_rack_core::plugin::{Plugin, PluginCore, ProcessContext, ProcessStatus};
use truce_rack_core::scanner::PluginScanner;
use truce_rack_core::transport::TransportInfo;

use lv2_raw::atom::{
    LV2Atom, LV2AtomEvent, LV2AtomObjectBody, LV2AtomPropertyBody, LV2AtomSequence,
    LV2AtomSequenceBody,
};
use lv2_raw::core::LV2Feature;
use lv2_raw::ui::{LV2UIControllerRaw, LV2UIDescriptorRaw, LV2UIHandle, LV2UIWidget};
use lv2_raw::urid::{LV2Urid, LV2UridMap, LV2UridMapHandle};

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod options;
mod parameters;
mod pending_controls;
mod port_groups;
mod state;

/// Format identifier used on returned [`PluginInfo`].
pub const FORMAT: &str = "lv2";

const LV2_URID_MAP_URI: &[u8] = b"http://lv2plug.in/ns/ext/urid#map\0";
const LV2_BOUNDED_BLOCK_URI: &[u8] = b"http://lv2plug.in/ns/ext/buf-size#boundedBlockLength\0";
const LV2_CORE_DESIGNATION_URI: &[u8] = b"http://lv2plug.in/ns/lv2core#designation\0";
const LV2_CORE_LATENCY_URI: &[u8] = b"http://lv2plug.in/ns/lv2core#latency\0";
const LV2_CORE_REPORTS_LATENCY_URI: &[u8] = b"http://lv2plug.in/ns/lv2core#reportsLatency\0";
const LV2_ATOM_SEQUENCE_URI: &[u8] = b"http://lv2plug.in/ns/ext/atom#Sequence\0";
const LV2_UI_PARENT_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#parent\0";
const LV2_UI_RESIZE_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#resize\0";
// Keep activation-time MIDI scratch storage under the processor host's existing 16 MiB bound.
const MAX_LV2_EVENT_BUFFER_BYTES: usize = 16 * 1024 * 1024;
const MAX_LV2_AUDIO_CHANNELS: usize = 64;
const LV2_UI_IDLE_INTERFACE_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#idleInterface\0";
const LV2_UI_RESIZE_INTERFACE_URI: &[u8] = LV2_UI_RESIZE_URI;

#[cfg(target_os = "macos")]
const NATIVE_UI_CLASS_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#CocoaUI\0";
#[cfg(target_os = "windows")]
const NATIVE_UI_CLASS_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#WindowsUI\0";
#[cfg(all(unix, not(target_os = "macos")))]
const NATIVE_UI_CLASS_URI: &[u8] = b"http://lv2plug.in/ns/extensions/ui#X11UI\0";

/// LV2 scanner.
#[derive(Debug, Default)]
pub struct Lv2Scanner;

impl Lv2Scanner {
    /// Construct a default scanner.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PluginScanner for Lv2Scanner {
    type Plugin = Lv2Plugin;

    fn scan(&self) -> Result<Vec<PluginInfo>> {
        let world = unsafe { World::new() }
            .ok_or_else(|| Error::Other("lilv_world_new returned NULL".into()))?;
        unsafe { lilv_sys::lilv_world_load_all(world.ptr) };
        Ok(unsafe { world.collect_plugin_infos() })
    }

    /// Discovers URI-identified plugins in exactly one `.lv2` bundle.
    ///
    /// Returns an error for an inaccessible bundle, missing manifest, or native world failure.
    fn scan_path(&self, path: &Path) -> Result<Vec<PluginInfo>> {
        let path = exact_bundle_path(path)?;
        // SAFETY: this new world is used only by this synchronous scan and retained until all
        // borrowed metadata has been copied into owned PluginInfo values.
        let world = unsafe { World::new() }
            .ok_or_else(|| Error::Other("lilv_world_new returned NULL".into()))?;
        world.load_bundle(&path)?;
        // SAFETY: the live world owns all metadata read during this call.
        let mut infos = unsafe { world.collect_plugin_infos() };
        for info in &mut infos {
            info.path.clone_from(&path);
        }
        Ok(infos)
    }

    fn load(&self, info: &PluginInfo) -> Result<Self::Plugin> {
        Lv2Plugin::load_from(info)
    }
}

// ---------------------------------------------------------------------------
// World wrapper
// ---------------------------------------------------------------------------

/// RAII wrapper around `*mut LilvWorld`. Holds the world for the
/// lifetime of either a scan or a loaded plugin (instance pointers
/// reference world-owned data, so the world must outlive them).
struct World {
    ptr: *mut lilv_sys::LilvWorld,
}

impl World {
    unsafe fn new() -> Option<Self> {
        let ptr = unsafe { lilv_sys::lilv_world_new() };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    fn load_bundle(&self, path: &Path) -> Result<()> {
        let path = path_to_cstring_with_trailing_sep(path)?;
        // SAFETY: self retains the live world; path remains NUL-terminated through URI creation.
        // The temporary URI is freed after loading, before querying the world's owned metadata.
        unsafe {
            let uri = lilv_sys::lilv_new_file_uri(self.ptr, std::ptr::null(), path.as_ptr());
            if uri.is_null() {
                return Err(Error::Other("LV2 bundle URI creation failed".into()));
            }
            lilv_sys::lilv_world_load_bundle(self.ptr, uri);
            lilv_sys::lilv_node_free(uri);
            lilv_sys::lilv_world_load_specifications(self.ptr);
            lilv_sys::lilv_world_load_plugin_classes(self.ptr);
        }
        Ok(())
    }

    /// Resolve one requested plugin while retaining the world that owns its native pointer.
    fn load_plugin(info: &PluginInfo) -> Result<(Box<Self>, *const lilv_sys::LilvPlugin)> {
        // SAFETY: the new native world is immediately boxed and retained with its selected
        // plugin pointer until the owning Lv2Plugin has freed its native instance.
        let world = Box::new(unsafe { Self::new() }.ok_or_else(|| Error::LoadFailed {
            path: info.path.clone(),
            reason: "lilv_world_new returned NULL".into(),
        })?);
        if info.path.as_os_str().is_empty() {
            // SAFETY: the returned box retains this world for every borrowed native object.
            unsafe { lilv_sys::lilv_world_load_all(world.ptr) };
        } else {
            world.load_bundle(&exact_bundle_path(&info.path)?)?;
        }

        let uri = CString::new(info.unique_id.clone()).map_err(|_| Error::LoadFailed {
            path: info.path.clone(),
            reason: format!("lv2 unique_id contains NUL: {:?}", info.unique_id),
        })?;
        // SAFETY: the world and NUL-terminated URI remain live through lookup; this node is freed.
        let uri_node = unsafe { lilv_sys::lilv_new_uri(world.ptr, uri.as_ptr()) };
        if uri_node.is_null() {
            return Err(Error::LoadFailed {
                path: info.path.clone(),
                reason: format!("lilv_new_uri failed for {:?}", info.unique_id),
            });
        }
        // SAFETY: Lilv returns world-owned plugin metadata. The returned plugin pointer is
        // retained only alongside the boxed world and its temporary URI node is freed below.
        let plugins = unsafe { lilv_sys::lilv_world_get_all_plugins(world.ptr) };
        let plugin = unsafe { lilv_sys::lilv_plugins_get_by_uri(plugins, uri_node) };
        unsafe { lilv_sys::lilv_node_free(uri_node) };
        if plugin.is_null() {
            return Err(Error::LoadFailed {
                path: info.path.clone(),
                reason: format!("no LV2 plugin matching URI {:?}", info.unique_id),
            });
        }
        Ok((world, plugin))
    }

    unsafe fn collect_plugin_infos(&self) -> Vec<PluginInfo> {
        // Stash this world for the per-plugin classifiers to reach
        // (lilv exposes no `plugin → world` accessor). Cleared
        // before we return so the TLS pointer doesn't outlive
        // the borrow.
        SCAN_WORLD.with(|w| w.set(self.ptr));
        let mut out = Vec::new();
        let plugins = unsafe { lilv_sys::lilv_world_get_all_plugins(self.ptr) };
        if plugins.is_null() {
            SCAN_WORLD.with(|w| w.set(std::ptr::null_mut()));
            return out;
        }
        let audio_uri = unsafe { self.new_uri(lilv_sys::LILV_URI_AUDIO_PORT.as_ptr().cast()) };
        let input_uri = unsafe { self.new_uri(lilv_sys::LILV_URI_INPUT_PORT.as_ptr().cast()) };
        let output_uri = unsafe { self.new_uri(lilv_sys::LILV_URI_OUTPUT_PORT.as_ptr().cast()) };
        let atom_uri = unsafe { self.new_uri(lilv_sys::LILV_URI_ATOM_PORT.as_ptr().cast()) };
        let mut it = unsafe { lilv_sys::lilv_plugins_begin(plugins) };
        loop {
            if unsafe { lilv_sys::lilv_plugins_is_end(plugins, it) } {
                break;
            }
            let plugin = unsafe { lilv_sys::lilv_plugins_get(plugins, it) };
            if !plugin.is_null() {
                out.push(unsafe {
                    plugin_to_info(plugin, audio_uri, input_uri, output_uri, atom_uri)
                });
            }
            it = unsafe { lilv_sys::lilv_plugins_next(plugins, it) };
        }
        unsafe {
            lilv_sys::lilv_node_free(audio_uri);
            lilv_sys::lilv_node_free(input_uri);
            lilv_sys::lilv_node_free(output_uri);
            lilv_sys::lilv_node_free(atom_uri);
        }
        SCAN_WORLD.with(|w| w.set(std::ptr::null_mut()));
        out
    }

    unsafe fn new_uri(&self, uri: *const c_char) -> *mut lilv_sys::LilvNode {
        unsafe { lilv_sys::lilv_new_uri(self.ptr, uri.cast()) }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        unsafe { lilv_sys::lilv_world_free(self.ptr) };
    }
}

// SAFETY: lilv's world is meant to be single-threaded for mutation
// but we hand the wrapper between activate/process on the same
// thread the host owns. We never share it.
unsafe impl Send for World {}

unsafe fn plugin_to_info(
    plugin: *const lilv_sys::LilvPlugin,
    audio_uri: *mut lilv_sys::LilvNode,
    input_uri: *mut lilv_sys::LilvNode,
    output_uri: *mut lilv_sys::LilvNode,
    atom_uri: *mut lilv_sys::LilvNode,
) -> PluginInfo {
    let uri_node = unsafe { lilv_sys::lilv_plugin_get_uri(plugin) };
    let uri = unsafe { node_to_uri_string(uri_node) };

    let name_node = unsafe { lilv_sys::lilv_plugin_get_name(plugin) };
    let name = unsafe { node_to_string_owned(name_node) };
    unsafe { lilv_sys::lilv_node_free(name_node) };

    let author_node = unsafe { lilv_sys::lilv_plugin_get_author_name(plugin) };
    let vendor = if author_node.is_null() {
        String::new()
    } else {
        let v = unsafe { node_to_string_owned(author_node) };
        unsafe { lilv_sys::lilv_node_free(author_node) };
        v
    };

    let mut audio_in = 0u32;
    let mut audio_out = 0u32;
    let mut accepts_midi = false;
    let count = unsafe { lilv_sys::lilv_plugin_get_num_ports(plugin) };
    for idx in 0..count {
        let port = unsafe { lilv_sys::lilv_plugin_get_port_by_index(plugin, idx) };
        if port.is_null() {
            continue;
        }
        let is_input = unsafe { lilv_sys::lilv_port_is_a(plugin, port, input_uri) };
        if unsafe { lilv_sys::lilv_port_is_a(plugin, port, audio_uri) } {
            if is_input {
                audio_in += 1;
            } else if unsafe { lilv_sys::lilv_port_is_a(plugin, port, output_uri) } {
                audio_out += 1;
            }
        } else if is_input && unsafe { lilv_sys::lilv_port_is_a(plugin, port, atom_uri) } {
            // Best-effort: any atom-port input is treated as a MIDI
            // sink for the catalog. The actual MIDI-vs-not check
            // requires walking the port's `atom:supports` triples,
            // which we do at load time.
            accepts_midi = true;
        }
    }
    let category = if audio_in == 0 && audio_out > 0 {
        PluginCategory::Instrument
    } else {
        PluginCategory::Effect
    };

    let has_editor = unsafe { has_native_ui(plugin) };

    PluginInfo {
        name,
        vendor,
        version: 0,
        category,
        path: std::path::PathBuf::new(),
        unique_id: uri,
        format: FORMAT,
        has_editor,
        accepts_midi,
    }
}

/// True if the plugin advertises a UI of this platform's native class
/// (`CocoaUI` / `WindowsUI` / `X11UI`).
unsafe fn has_native_ui(plugin: *const lilv_sys::LilvPlugin) -> bool {
    let uis = unsafe { lilv_sys::lilv_plugin_get_uis(plugin) };
    if uis.is_null() {
        return false;
    }
    let world_ptr = unsafe { lilv_plugin_world(plugin) };
    if world_ptr.is_null() {
        unsafe { lilv_sys::lilv_uis_free(uis) };
        return false;
    }
    let class_node =
        unsafe { lilv_sys::lilv_new_uri(world_ptr, NATIVE_UI_CLASS_URI.as_ptr().cast()) };
    let mut found = false;
    let mut it = unsafe { lilv_sys::lilv_uis_begin(uis) };
    while !unsafe { lilv_sys::lilv_uis_is_end(uis, it) } {
        let ui = unsafe { lilv_sys::lilv_uis_get(uis, it) };
        if !ui.is_null() && unsafe { lilv_sys::lilv_ui_is_a(ui, class_node) } {
            found = true;
            break;
        }
        it = unsafe { lilv_sys::lilv_uis_next(uis, it) };
    }
    unsafe {
        lilv_sys::lilv_node_free(class_node);
        lilv_sys::lilv_uis_free(uis);
    }
    found
}

/// Recover the world pointer from a plugin pointer. lilv stores it
/// internally but doesn't expose it directly; we cheat by stashing
/// it into a thread-local during `collect_plugin_infos`.
unsafe fn lilv_plugin_world(_plugin: *const lilv_sys::LilvPlugin) -> *mut lilv_sys::LilvWorld {
    SCAN_WORLD.with(std::cell::Cell::get)
}

thread_local! {
    /// Set by `collect_plugin_infos` for the duration of one scan
    /// pass so the per-plugin classifiers can `lilv_new_uri`
    /// against the same world. Cleared on Drop of the World guard.
    static SCAN_WORLD: std::cell::Cell<*mut lilv_sys::LilvWorld> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

unsafe fn node_to_uri_string(node: *const lilv_sys::LilvNode) -> String {
    if node.is_null() {
        return String::new();
    }
    let p = unsafe { lilv_sys::lilv_node_as_uri(node) };
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

unsafe fn node_to_string_owned(node: *mut lilv_sys::LilvNode) -> String {
    if node.is_null() {
        return String::new();
    }
    let p = unsafe { lilv_sys::lilv_node_as_string(node) };
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// URID map + features
// ---------------------------------------------------------------------------

/// Bidirectional URI ↔ u32 map handed to LV2 plugins as the
/// standard `urid#map` feature.
struct UridMap {
    entries: Mutex<HashMap<CString, LV2Urid>>,
    next_id: Mutex<LV2Urid>,
}

impl UridMap {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            // Reserve 0 as the LV2-spec "couldn't map" sentinel.
            next_id: Mutex::new(1),
        }
    }

    fn map_uri(&self, uri: &CStr) -> LV2Urid {
        let mut entries = self.entries.lock().expect("urid map mutex");
        if let Some(&id) = entries.get(uri) {
            return id;
        }
        let mut next = self.next_id.lock().expect("urid next mutex");
        let id = *next;
        *next += 1;
        entries.insert(uri.to_owned(), id);
        id
    }
}

extern "C" fn map_callback(handle: LV2UridMapHandle, uri: *const c_char) -> LV2Urid {
    if handle.is_null() || uri.is_null() {
        return 0;
    }
    // SAFETY: `handle` is the `&UridMap` we set into the feature
    // struct at instantiate time; lilv passes it back unchanged.
    let map = unsafe { &*(handle as *const UridMap) };
    let cstr = unsafe { CStr::from_ptr(uri) };
    map.map_uri(cstr)
}

/// Self-contained LV2 feature list. Heap-allocated so the pointers
/// fed to `lilv_plugin_instantiate` stay valid for the instance's
/// lifetime even if `Lv2Plugin` is moved.
struct Features {
    urid_map: UridMap,
    options: Box<options::HostOptions>,
    // `LV2UridMap.handle` points at `urid_map` — the box's pinned
    // address. `LV2UridMap.map` is the static callback. Held inline
    // so its address can be taken for `feature.data`.
    map_struct: LV2UridMap,
    // The features array proper. Each `data` field points at a
    // sibling field of the same `Features` allocation.
    feature_storage: Vec<LV2Feature>,
    // Null-terminated pointer array — what `lilv_plugin_instantiate`
    // actually consumes.
    feature_ptrs: Vec<*const LV2Feature>,
}

impl Features {
    fn new() -> Box<Self> {
        // Build in two passes so the `Box`'s heap address is stable
        // before we record interior pointers.
        let urid_map = UridMap::new();
        let options = options::HostOptions::new(&urid_map);
        let mut boxed = Box::new(Features {
            urid_map,
            options,
            map_struct: LV2UridMap {
                handle: std::ptr::null_mut(),
                map: map_callback,
            },
            feature_storage: Vec::with_capacity(3),
            feature_ptrs: Vec::with_capacity(4),
        });
        let urid_map_handle: *mut UridMap = &mut boxed.urid_map as *mut UridMap;
        boxed.map_struct.handle = urid_map_handle.cast::<c_void>();
        boxed.feature_storage.push(LV2Feature {
            uri: LV2_URID_MAP_URI.as_ptr().cast::<c_char>(),
            data: (&boxed.map_struct) as *const LV2UridMap as *mut c_void,
        });
        boxed.feature_storage.push(LV2Feature {
            uri: options::FEATURE_URI.as_ptr().cast(),
            data: boxed.options.data(),
        });
        boxed.feature_storage.push(LV2Feature {
            uri: LV2_BOUNDED_BLOCK_URI.as_ptr().cast(),
            data: std::ptr::null_mut(),
        });
        for feature in &boxed.feature_storage {
            boxed.feature_ptrs.push(feature);
        }
        boxed.feature_ptrs.push(std::ptr::null());
        boxed
    }
}

// ---------------------------------------------------------------------------
// UI plumbing
// ---------------------------------------------------------------------------

/// Metadata about a plugin's native UI bundle, captured at load
/// time. `binary_path` is the absolute filesystem path to the
/// `.so`/`.dylib`/`.dll` that exports `lv2ui_descriptor`.
#[derive(Debug, Clone)]
struct UiBundle {
    ui_uri: CString,
    bundle_path: PathBuf,
    binary_path: PathBuf,
}

/// Keep the UI library handle with the selected descriptor until editor cleanup.
fn open_ui_library(bundle: &UiBundle) -> Result<(libloading::Library, *const LV2UIDescriptorRaw)> {
    type DescFn = unsafe extern "C" fn(u32) -> *const LV2UIDescriptorRaw;
    // SAFETY: this handle stays owned by EditorState until the descriptor's cleanup returns.
    let library = unsafe { libloading::Library::new(&bundle.binary_path) }.map_err(|error| {
        Error::Other(format!(
            "lv2 ui dlopen {}: {}",
            bundle.binary_path.display(),
            error
        ))
    })?;
    // SAFETY: the symbol and every returned descriptor/URI are only inspected while their
    // library is open; the matching raw pointer is returned with that same library handle.
    let descriptor = unsafe {
        let symbol: libloading::Symbol<DescFn> = library
            .get(b"lv2ui_descriptor")
            .map_err(|error| Error::Other(format!("lv2 ui_descriptor symbol: {error}")))?;
        let mut chosen = std::ptr::null();
        for index in 0u32..64 {
            let candidate = symbol(index);
            if candidate.is_null() {
                break;
            }
            let uri = (*candidate).uri;
            if !uri.is_null() && CStr::from_ptr(uri) == bundle.ui_uri.as_c_str() {
                chosen = candidate;
                break;
            }
        }
        chosen
    };
    if descriptor.is_null() {
        return Err(Error::Other(format!(
            "no lv2ui_descriptor matching {:?}",
            bundle.ui_uri
        )));
    }
    Ok((library, descriptor))
}

/// Live editor state. Created on `PluginEditor::open`; torn down on
/// `close` or `Drop`. Kept on `Lv2Plugin` so the dropped instance's
/// `lv2ui_descriptor.cleanup` runs before the audio instance is
/// freed.
struct EditorState {
    /// The dlopen'd UI bundle. Held so the descriptor / function
    /// pointers stay valid; dropped after `cleanup` returns.
    _library: libloading::Library,
    descriptor: *const LV2UIDescriptorRaw,
    handle: LV2UIHandle,
    widget: LV2UIWidget,
    /// Self-contained features array passed to `instantiate`.
    /// Boxed so the pointers we recorded into the LV2 plugin stay
    /// valid even if `Lv2Plugin` itself is moved. Held for ownership
    /// only — the UI side reaches into it through the raw feature
    /// pointers it was given at instantiate time.
    #[allow(dead_code)]
    ui_features: Box<UiFeatureStorage>,
    /// Optional `LV2_UI__idleInterface` — non-null if the UI exports
    /// it via `extension_data`. Driven by `on_idle` once per host
    /// frame.
    idle_iface: *const lv2_raw::ui::LV2UIIdleInterface,
    /// Optional `LV2_UI__resize` interface — non-null if the UI
    /// exports it via `extension_data`. Used by `set_size` to push
    /// host-driven resizes to the UI.
    resize_iface: *const Lv2UiResizeInterface,
    /// Snapshot of `control_values` taken at open time and refreshed
    /// every `on_idle` so we can fire `port_event` for any value the
    /// audio thread (or another UI write) has changed since the last
    /// idle tick.
    last_pushed_values: Vec<f32>,
}

/// LV2 host-side `ui:resize` interface — the same shape both the
/// host implements (passed via the feature) and the UI implements
/// (returned from `extension_data`). Non-zero `ui_resize` returns
/// indicate failure.
#[repr(C)]
struct Lv2UiResizeInterface {
    /// Opaque pointer the UI passes back. For host → UI, this is
    /// the UI's own handle.
    handle: *mut c_void,
    ui_resize: extern "C" fn(handle: *mut c_void, width: i32, height: i32) -> i32,
}

/// Heap-stable storage for the LV2 UI feature array. Mirrors the
/// shape of [`Features`] for the audio side.
struct UiFeatureStorage {
    /// Same URID map the audio instance uses — the UI talks to the
    /// host through the same map.
    map_struct: LV2UridMap,
    /// `ui#parent` feature. `data` is a raw pointer to the parent
    /// widget (`NSView`* / HWND / X11 Window).
    parent_feature_data: *mut c_void,
    /// Host-side `ui:resize` callback. The UI calls
    /// `resize_struct.ui_resize(resize_struct.handle, w, h)` to ask
    /// the host to resize. The handle is a pointer to a
    /// `HostResizeNotifier` heap-allocated alongside us; the host
    /// reads the latest requested size out of it via `take_request`.
    resize_struct: Lv2UiResizeInterface,
    resize_notifier: Box<HostResizeNotifier>,
    feature_storage: Vec<LV2Feature>,
    feature_ptrs: Vec<*const LV2Feature>,
}

/// Heap-stable cell the UI's `ui_resize` callback writes its
/// requested dimensions into. The standalone polls this every
/// `on_idle` and applies any pending request.
struct HostResizeNotifier {
    /// `(width, height)` requested by the UI; `None` means no
    /// pending request. Written by the UI thread, read by the host
    /// — both run on the same main thread, but the field is
    /// touched from C code outside Rust's borrow tracking, so we
    /// use `Cell` to make the interior mutability explicit.
    pending: std::cell::Cell<Option<(i32, i32)>>,
}

extern "C" fn host_resize_callback(handle: *mut c_void, width: i32, height: i32) -> i32 {
    if handle.is_null() {
        return 1;
    }
    // SAFETY: `handle` is the boxed `HostResizeNotifier` we stored
    // in the resize_struct at construction. lilv passes it back
    // unchanged.
    let notifier = unsafe { &*(handle as *const HostResizeNotifier) };
    notifier.pending.set(Some((width, height)));
    0
}

/// Heap-stable callback context. The UI may publish while DSP reads control slots;
/// only serialized plugin access transfers mailbox values to those slots.
struct Controller {
    pending: pending_controls::PendingControls,
}

impl Controller {
    /// Fix the input-port map and mailbox size before any UI callback can be registered.
    fn for_ports(ports: &[PortInfo], offsets: &[usize], control_count: usize) -> Self {
        let input_offsets = ports
            .iter()
            .zip(offsets)
            .map(|(port, &offset)| if port.is_input { offset } else { usize::MAX })
            .collect();
        Self {
            pending: pending_controls::PendingControls::new(input_offsets, control_count),
        }
    }
}

extern "C" fn ui_write_callback(
    controller: LV2UIControllerRaw,
    port_index: libc::c_uint,
    buffer_size: libc::c_uint,
    port_protocol: libc::c_uint,
    buffer: *const c_void,
) {
    if controller.is_null() || buffer.is_null() {
        return;
    }
    // Only the implicit float protocol (port_protocol == 0) updates
    // a control port; richer protocols (atom event transfer, etc.)
    // are not yet plumbed back into the audio side.
    if port_protocol != 0 || buffer_size != 4 {
        return;
    }
    // SAFETY: `controller` is the boxed Controller retained until UI cleanup returns;
    // the UI returns that opaque pointer unchanged and its only mutable fields are atomic.
    let ctrl = unsafe { &*(controller as *const Controller) };
    // SAFETY: implicit floatProtocol requires a readable buffer_size == sizeof(float)
    // payload. `read_unaligned` also accepts a UI-provided buffer without f32 alignment.
    let value = unsafe { (buffer as *const f32).read_unaligned() };
    ctrl.pending.publish(port_index as usize, value);
}

#[cfg(test)]
mod ui_write_tests {
    use super::*;

    #[test]
    fn float_callback_accepts_only_finite_input_control_writes() {
        let ports = [
            PortInfo {
                index: 0,
                kind: PortKind::Control { default: 0.0 },
                is_input: true,
                audio_plane: None,
            },
            PortInfo {
                index: 1,
                kind: PortKind::Control { default: 0.0 },
                is_input: false,
                audio_plane: None,
            },
        ];
        let controller = Box::new(Controller::for_ports(&ports, &[0, 0], 1));
        let opaque = (controller.as_ref() as *const Controller)
            .cast_mut()
            .cast::<c_void>();
        let value = 0.625_f32;
        ui_write_callback(opaque, 1, 4, 0, (&raw const value).cast());
        ui_write_callback(opaque, 0, 4, 1, (&raw const value).cast());
        ui_write_callback(opaque, 0, 3, 0, (&raw const value).cast());
        assert_eq!(controller.pending.peek(0), None);

        ui_write_callback(opaque, 0, 4, 0, (&raw const value).cast());
        assert_eq!(controller.pending.take(0), Some(value));
        let mut unaligned = [0_u8; 5];
        unaligned[1..].copy_from_slice(&0.75_f32.to_ne_bytes());
        ui_write_callback(opaque, 0, 4, 0, unaligned[1..].as_ptr().cast());
        assert_eq!(controller.pending.take(0), Some(0.75));
        let invalid = f32::INFINITY;
        ui_write_callback(opaque, 0, 4, 0, (&raw const invalid).cast());
        assert_eq!(controller.pending.peek(0), None);
    }
}

impl UiFeatureStorage {
    // The `Box` return is load-bearing: the caller stores raw
    // pointers into this struct's interior fields, so the heap
    // allocation must outlive `Lv2Plugin` moves.
    #[allow(clippy::unnecessary_box_returns)]
    fn new(map_handle: *mut UridMap, parent: *mut c_void) -> Box<Self> {
        let mut boxed = Box::new(UiFeatureStorage {
            map_struct: LV2UridMap {
                handle: std::ptr::null_mut(),
                map: map_callback,
            },
            parent_feature_data: parent,
            resize_struct: Lv2UiResizeInterface {
                handle: std::ptr::null_mut(),
                ui_resize: host_resize_callback,
            },
            resize_notifier: Box::new(HostResizeNotifier {
                pending: std::cell::Cell::new(None),
            }),
            feature_storage: Vec::with_capacity(3),
            feature_ptrs: Vec::with_capacity(4),
        });
        boxed.map_struct.handle = map_handle.cast::<c_void>();
        boxed.resize_struct.handle =
            (&*boxed.resize_notifier) as *const HostResizeNotifier as *mut c_void;
        boxed.feature_storage.push(LV2Feature {
            uri: LV2_URID_MAP_URI.as_ptr().cast::<c_char>(),
            data: (&boxed.map_struct) as *const LV2UridMap as *mut c_void,
        });
        boxed.feature_storage.push(LV2Feature {
            uri: LV2_UI_PARENT_URI.as_ptr().cast::<c_char>(),
            data: boxed.parent_feature_data,
        });
        boxed.feature_storage.push(LV2Feature {
            uri: LV2_UI_RESIZE_URI.as_ptr().cast::<c_char>(),
            data: (&boxed.resize_struct) as *const Lv2UiResizeInterface as *mut c_void,
        });
        for i in 0..boxed.feature_storage.len() {
            boxed.feature_ptrs.push(&boxed.feature_storage[i]);
        }
        boxed.feature_ptrs.push(std::ptr::null());
        boxed
    }
}

/// Walk `plugin`'s UIs for one matching the host's native UI class.
/// Returns the first match's metadata or `None`.
unsafe fn discover_ui(
    world: *mut lilv_sys::LilvWorld,
    plugin: *const lilv_sys::LilvPlugin,
) -> Option<UiBundle> {
    let uis = unsafe { lilv_sys::lilv_plugin_get_uis(plugin) };
    if uis.is_null() {
        return None;
    }
    let class_node = unsafe { lilv_sys::lilv_new_uri(world, NATIVE_UI_CLASS_URI.as_ptr().cast()) };
    let mut chosen: Option<UiBundle> = None;
    let mut it = unsafe { lilv_sys::lilv_uis_begin(uis) };
    while !unsafe { lilv_sys::lilv_uis_is_end(uis, it) } {
        let ui = unsafe { lilv_sys::lilv_uis_get(uis, it) };
        if !ui.is_null() && unsafe { lilv_sys::lilv_ui_is_a(ui, class_node) } {
            let uri_str = unsafe {
                let n = lilv_sys::lilv_ui_get_uri(ui);
                node_to_uri_string(n)
            };
            let bundle_path = unsafe { node_uri_to_path(lilv_sys::lilv_ui_get_bundle_uri(ui)) };
            let binary_path = unsafe { node_uri_to_path(lilv_sys::lilv_ui_get_binary_uri(ui)) };
            if let (Ok(uri_c), Some(bundle), Some(binary)) =
                (CString::new(uri_str), bundle_path, binary_path)
            {
                chosen = Some(UiBundle {
                    ui_uri: uri_c,
                    bundle_path: bundle,
                    binary_path: binary,
                });
                break;
            }
        }
        it = unsafe { lilv_sys::lilv_uis_next(uis, it) };
    }
    unsafe {
        lilv_sys::lilv_node_free(class_node);
        lilv_sys::lilv_uis_free(uis);
    }
    chosen
}

unsafe fn node_uri_to_path(node: *const lilv_sys::LilvNode) -> Option<PathBuf> {
    if node.is_null() {
        return None;
    }
    let uri = unsafe { lilv_sys::lilv_node_as_uri(node) };
    if uri.is_null() {
        return None;
    }
    let raw = unsafe { lilv_sys::lilv_file_uri_parse(uri, std::ptr::null_mut()) };
    if raw.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { lilv_sys::lilv_free(raw.cast()) };
    Some(PathBuf::from(s))
}

// ---------------------------------------------------------------------------
// Lv2Plugin
// ---------------------------------------------------------------------------

/// Cached metadata about one LV2 port.
#[derive(Debug, Clone, Copy)]
struct PortInfo {
    index: u32,
    kind: PortKind,
    is_input: bool,
    /// Flattened bus/channel slot for native audio ports, computed at load.
    audio_plane: Option<usize>,
}

/// Stack-only view of every active bus plane in flattened host order.
struct AudioPlanes {
    inputs: [*mut f32; MAX_LV2_AUDIO_CHANNELS],
    outputs: [*mut f32; MAX_LV2_AUDIO_CHANNELS],
    input_count: usize,
    output_count: usize,
}

impl AudioPlanes {
    fn collect(
        buffer: &mut AudioBuffer<'_, f32>,
        layout: &BusLayout,
        max_block_size: usize,
    ) -> Result<Self> {
        let frames = buffer.num_frames();
        if buffer.num_input_buses() != layout.inputs.len()
            || buffer.num_output_buses() != layout.outputs.len()
            || buffer.total_input_channels() != layout.total_input_channels() as usize
            || buffer.total_output_channels() != layout.total_output_channels() as usize
            || frames > max_block_size
            || u32::try_from(frames).is_err()
        {
            return Err(Error::Other(
                "LV2 process buffer topology does not match active layout".into(),
            ));
        }
        let mut planes = Self {
            inputs: [std::ptr::null_mut(); MAX_LV2_AUDIO_CHANNELS],
            outputs: [std::ptr::null_mut(); MAX_LV2_AUDIO_CHANNELS],
            input_count: 0,
            output_count: 0,
        };
        for (bus_index, bus) in layout.inputs.iter().enumerate() {
            let channels = buffer.bus_inputs(bus_index);
            if channels.len() != bus.channels.count() as usize {
                return Err(Error::Other("LV2 input bus channel count changed".into()));
            }
            for channel in channels {
                if channel.len() < frames || planes.input_count >= planes.inputs.len() {
                    return Err(Error::Other("LV2 input plane is invalid".into()));
                }
                planes.inputs[planes.input_count] = channel.as_ptr().cast_mut();
                planes.input_count += 1;
            }
        }
        for (bus_index, bus) in layout.outputs.iter().enumerate() {
            let channels = buffer.bus_outputs(bus_index);
            if channels.len() != bus.channels.count() as usize {
                return Err(Error::Other("LV2 output bus channel count changed".into()));
            }
            for channel in channels {
                if channel.len() < frames || planes.output_count >= planes.outputs.len() {
                    return Err(Error::Other("LV2 output plane is invalid".into()));
                }
                planes.outputs[planes.output_count] = channel.as_mut_ptr();
                planes.output_count += 1;
            }
        }
        Ok(planes)
    }

    fn validate_ports(&self, ports: &[PortInfo]) -> Result<()> {
        for port in ports {
            if matches!(port.kind, PortKind::Audio) {
                let Some(plane) = port.audio_plane else {
                    return Err(Error::Other("LV2 native audio port lacks a plane".into()));
                };
                if (port.is_input && plane >= self.input_count)
                    || (!port.is_input && plane >= self.output_count)
                {
                    return Err(Error::Other(
                        "LV2 native audio plane is out of range".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum PortKind {
    Audio,
    Control { default: f32 },
    AtomSequence,
    Other,
}

/// Metadata source for the plugin-reported LV2 latency scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatencyPort {
    /// No latency port is reported by the plugin; LV2 defines this as zero latency.
    NotReported,
    /// Connected output control-port offset, read after `lilv_instance_run`.
    OutputControl { offset: usize },
    /// Metadata is malformed, ambiguous, or could not be inspected reliably.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatencyCandidate {
    NotReported,
    Reported(usize),
    Malformed,
}

/// URIDs needed to build a `time:Position` object atom, mapped once
/// at load. The `atom_*` entries are the value types each property
/// carries; the rest are the `time:` properties themselves.
#[derive(Clone, Copy)]
struct TimeUrids {
    position: LV2Urid,
    frame: LV2Urid,
    speed: LV2Urid,
    bar: LV2Urid,
    bar_beat: LV2Urid,
    beats_per_bar: LV2Urid,
    beat_unit: LV2Urid,
    bpm: LV2Urid,
    atom_long: LV2Urid,
    atom_float: LV2Urid,
    atom_int: LV2Urid,
    atom_object: LV2Urid,
}

impl TimeUrids {
    /// Map transport-property URIs once, before the realtime path can use them.
    fn new(map: &UridMap) -> Self {
        // SAFETY: only process-lifetime NUL-terminated LV2 URI constants are passed below.
        let map_uri = |uri: &[u8]| map.map_uri(unsafe { CStr::from_ptr(uri.as_ptr().cast()) });
        Self {
            position: map_uri(lv2_raw::time::LV2_TIME__POSITION),
            frame: map_uri(lv2_raw::time::LV2_TIME__FRAME),
            speed: map_uri(lv2_raw::time::LV2_TIME__SPEED),
            bar: map_uri(lv2_raw::time::LV2_TIME__BAR),
            bar_beat: map_uri(lv2_raw::time::LV2_TIME__BARBEAT),
            beats_per_bar: map_uri(lv2_raw::time::LV2_TIME__BEATSPERBAR),
            beat_unit: map_uri(lv2_raw::time::LV2_TIME__BEATUNIT),
            bpm: map_uri(lv2_raw::time::LV2_TIME__BEATSPERMINUTE),
            atom_long: map_uri(lv2_raw::atom::LV2_ATOM__LONG),
            atom_float: map_uri(lv2_raw::atom::LV2_ATOM__FLOAT),
            atom_int: map_uri(lv2_raw::atom::LV2_ATOM__INT),
            atom_object: map_uri(lv2_raw::atom::LV2_ATOM__OBJECT),
        }
    }
}

/// One loaded LV2 plugin.
///
/// Owns its `World` (lilv data is referenced by pointer from the
/// instance), its features, its per-port metadata, and the per-port
/// scratch buffers connected on every block.
pub struct Lv2Plugin {
    info: PluginInfo,
    layouts: Vec<BusLayout>,
    active_layout: Option<BusLayout>,
    latency_port: LatencyPort,
    /// Cached scalar value; native output is copied after run, never queried on callback read.
    latency_frames: Option<u32>,

    /// Held for ownership: the `LilvPlugin` pointer and the instance
    /// both reference world-owned data, so the world must outlive
    /// them. Read directly only at load.
    #[allow(dead_code)]
    world: Box<World>,
    plugin: *const lilv_sys::LilvPlugin,
    instance: *mut lilv_sys::LilvInstance,

    sample_rate: f64,

    ports: Vec<PortInfo>,
    /// Backing storage for control ports. `connect_port` points the
    /// plugin at one f32 per control port; the index here matches
    /// the position in `ports` filtered to controls.
    control_values: Vec<f32>,
    /// Map from `ports` index to the offset in `control_values`.
    control_value_offset: Vec<usize>,
    /// Load-time metadata indexes the same scalar storage used by DSP, state, and the editor.
    parameters: parameters::Parameters,

    /// Backing storage for the (single) MIDI input atom-sequence
    /// port, if the plugin has one. Rebuilt each block from the
    /// truce-rack `EventList`.
    midi_in_buf: Vec<u8>,
    /// Backing storage for the MIDI output atom-sequence port. We
    /// connect a chunk-typed buffer so the plugin has somewhere to
    /// write; we don't currently drain it back into the host.
    midi_out_buf: Vec<u8>,

    features: Box<Features>,
    /// Lazily created private namespace retained across saves for state:makePath results.
    asset_dir: Mutex<Option<tempfile::TempDir>>,
    /// Files materialized from a checkpoint stay alive until native instance teardown.
    restored_assets: Option<tempfile::TempDir>,
    midi_event_urid: LV2Urid,
    atom_sequence_urid: LV2Urid,
    time_urids: TimeUrids,

    /// UI metadata captured at load if the plugin advertises a
    /// native UI for this platform. `None` means no editor — the
    /// `PluginEditor` impl will refuse `open`.
    ui_bundle: Option<UiBundle>,
    /// Live editor — `Some` between `open` and `close`.
    editor: Option<EditorState>,
    /// Heap-stable, load-time bounded UI callback context. The UI is cleaned up before drop.
    controller: Box<Controller>,
}

// SAFETY: We hand the plugin between the audio and main threads
// behind an `Arc<Mutex<_>>` exactly like every other truce-rack format.
unsafe impl Send for Lv2Plugin {}

impl Lv2Plugin {
    fn load_from(info: &PluginInfo) -> Result<Self> {
        let (world, plugin) = World::load_plugin(info)?;

        // Walk the ports once, classify each, and stash
        // direction + control defaults.
        let ClassifiedPorts {
            mut ports,
            audio_in_count,
            audio_out_count,
            control_value_offset,
            control_values,
            latency_port,
        } = unsafe { classify_ports(world.ptr, plugin) };
        // SAFETY: both native pointers are owned by the retained world; port/offset metadata
        // was classified from this exact plugin immediately above. Collection is load-time only.
        let parameters = unsafe {
            parameters::Parameters::collect(world.ptr, plugin, &ports, &control_value_offset)
        }?;

        // Pre-map the URIs we'll need at process time.
        let features = Features::new();
        let midi_event_urid = features
            .urid_map
            .map_uri(unsafe { CStr::from_ptr(lilv_sys::LILV_URI_MIDI_EVENT.as_ptr().cast()) });
        let atom_sequence_urid = features
            .urid_map
            .map_uri(unsafe { CStr::from_ptr(LV2_ATOM_SEQUENCE_URI.as_ptr().cast()) });

        let time_urids = TimeUrids::new(&features.urid_map);

        // LV2 port-group declarations, when present, are the native bus contract.
        // Resolve every raw port to a flattened host plane before instantiation.
        let layout =
            unsafe { port_groups::classify(world.ptr, plugin, &mut ports) }.map_err(|reason| {
                Error::LoadFailed {
                    path: info.path.clone(),
                    reason,
                }
            })?;
        debug_assert_eq!(layout.total_input_channels() as usize, audio_in_count);
        debug_assert_eq!(layout.total_output_channels() as usize, audio_out_count);

        let ui_bundle = unsafe { discover_ui(world.ptr, plugin) };
        let mut info = info.clone();
        info.has_editor = ui_bundle.is_some();

        // The box fixes the callback context address before registering it with any native UI.
        let controller = Box::new(Controller::for_ports(
            &ports,
            &control_value_offset,
            control_values.len(),
        ));

        Ok(Self {
            info,
            layouts: vec![layout],
            active_layout: None,
            latency_port,
            latency_frames: match latency_port {
                LatencyPort::NotReported => Some(0),
                LatencyPort::OutputControl { .. } | LatencyPort::Unknown => None,
            },
            world,
            plugin,
            instance: std::ptr::null_mut(),
            sample_rate: 0.0,
            ports,
            control_values,
            control_value_offset,
            parameters,
            midi_in_buf: Vec::new(),
            midi_out_buf: Vec::new(),
            features,
            asset_dir: Mutex::new(None),
            restored_assets: None,
            midi_event_urid,
            atom_sequence_urid,
            time_urids,
            ui_bundle,
            editor: None,
            controller,
        })
    }

    fn drain_pending_controls(&mut self) {
        for (offset, value) in self.control_values.iter_mut().enumerate() {
            if let Some(pending) = self.controller.pending.take(offset) {
                *value = pending;
            }
        }
    }

    fn effective_control_value(&self, offset: usize) -> f32 {
        self.controller
            .pending
            .peek(offset)
            .unwrap_or(self.control_values[offset])
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<()> {
        // A prior UI edit precedes this restore. A publish during restore remains pending.
        self.drain_pending_controls();
        state::restore(self, bytes)
    }

    fn build_midi_sequence(&mut self, events: &EventList, transport: Option<TransportInfo>) {
        let header_size = std::mem::size_of::<LV2AtomSequence>();
        let cap = self.midi_in_buf.len();
        if cap < header_size {
            return;
        }
        let buf = self.midi_in_buf.as_mut_ptr();

        // Header: capacity-bounded sequence whose `atom.size` only
        // counts the body + events (the LV2 atom convention).
        let body_size = std::mem::size_of::<LV2AtomSequenceBody>();
        let seq = unsafe { &mut *buf.cast::<LV2AtomSequence>() };
        seq.atom = LV2Atom {
            size: body_size as u32,
            mytype: self.atom_sequence_urid,
        };
        seq.body = LV2AtomSequenceBody { unit: 0, pad: 0 };

        let mut write_off = header_size;

        // Inject a `time:Position` object as the first event (frame
        // 0) so transport-aware plugins see tempo / grid before any
        // MIDI. MIDI events follow, also at frame >= 0.
        if let Some(t) = transport {
            write_off = unsafe { write_time_position(&self.time_urids, buf, write_off, cap, &t) };
        }

        let event_header = std::mem::size_of::<LV2AtomEvent>();
        for ev in events {
            let Some((status, d1, d2, len)) = midi_bytes(&ev.body) else {
                continue;
            };
            let total = event_header + len;
            let padded = (total + 7) & !7; // 8-byte alignment per atom spec
            if write_off + padded > cap {
                break;
            }
            let event_ptr = unsafe { buf.add(write_off) }.cast::<LV2AtomEvent>();
            unsafe {
                (*event_ptr).time_in_frames = i64::from(ev.sample_offset);
                (*event_ptr).body = LV2Atom {
                    size: len as u32,
                    mytype: self.midi_event_urid,
                };
                let data_ptr = (event_ptr as *mut u8).add(event_header);
                if len >= 1 {
                    *data_ptr = status;
                }
                if len >= 2 {
                    *data_ptr.add(1) = d1;
                }
                if len >= 3 {
                    *data_ptr.add(2) = d2;
                }
            }
            write_off += padded;
        }
        // sequence body size is everything past the atom header
        seq.atom.size = (write_off - std::mem::size_of::<LV2Atom>()) as u32;
    }

    fn prep_midi_out(&mut self) {
        // Reset to an empty Chunk-typed atom so the plugin has a
        // defined buffer to write into. We don't drain output yet.
        if self.midi_out_buf.len() < std::mem::size_of::<LV2AtomSequence>() {
            return;
        }
        let buf = self.midi_out_buf.as_mut_ptr();
        let seq = unsafe { &mut *buf.cast::<LV2AtomSequence>() };
        seq.atom = LV2Atom {
            size: std::mem::size_of::<LV2AtomSequenceBody>() as u32,
            mytype: self.atom_sequence_urid,
        };
        seq.body = LV2AtomSequenceBody { unit: 0, pad: 0 };
    }
}

/// Upper bound on the bytes one `time:Position` event consumes:
/// event + object header plus eight 8-byte-aligned properties. Used
/// to bounds-check once instead of per-property.
const TIME_POSITION_MAX_BYTES: usize =
    std::mem::size_of::<LV2AtomEvent>() + std::mem::size_of::<LV2AtomObjectBody>() + 8 * 32;

/// Write one `time:` property (key + typed scalar value) into an
/// object body at `off`, returning the next 8-byte-aligned offset.
///
/// # Safety
/// `buf + off` must have room for the property header plus `value`
/// rounded up to 8 bytes; callers guarantee this via a single
/// up-front [`TIME_POSITION_MAX_BYTES`] check.
unsafe fn write_property(
    buf: *mut u8,
    off: usize,
    key: LV2Urid,
    value_type: LV2Urid,
    value: &[u8],
) -> usize {
    let prop = unsafe { &mut *buf.add(off).cast::<LV2AtomPropertyBody>() };
    prop.key = key;
    prop.context = 0;
    prop.value = LV2Atom {
        size: u32::try_from(value.len()).unwrap_or(0),
        mytype: value_type,
    };
    let data = unsafe { buf.add(off + std::mem::size_of::<LV2AtomPropertyBody>()) };
    unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), data, value.len()) };
    let total = std::mem::size_of::<LV2AtomPropertyBody>() + value.len();
    off + ((total + 7) & !7)
}

/// Write a `time:Position` object event (at frame 0) into the atom
/// sequence buffer starting at `base`, returning the offset just
/// past it. Returns `base` unchanged if there isn't room.
///
/// # Safety
/// `buf` must point at a buffer of at least `cap` bytes that is
/// valid for writes in `[base, cap)` and aligned for atom structs.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
unsafe fn write_time_position(
    urids: &TimeUrids,
    buf: *mut u8,
    base: usize,
    cap: usize,
    t: &TransportInfo,
) -> usize {
    if base + TIME_POSITION_MAX_BYTES > cap {
        return base;
    }
    let event_header = std::mem::size_of::<LV2AtomEvent>();
    let obj_body_off = base + event_header;

    // Object body header (id = 0 blank, otype = time:Position).
    let obj_body = unsafe { &mut *buf.add(obj_body_off).cast::<LV2AtomObjectBody>() };
    obj_body.id = 0;
    obj_body.otype = urids.position;

    let mut off = obj_body_off + std::mem::size_of::<LV2AtomObjectBody>();

    // speed: 1.0 while rolling, 0.0 when stopped.
    let speed: f32 = if t.playing { 1.0 } else { 0.0 };
    off = unsafe {
        write_property(
            buf,
            off,
            urids.speed,
            urids.atom_float,
            &speed.to_ne_bytes(),
        )
    };

    if let Some(samples) = t.song_position_samples {
        off = unsafe {
            write_property(
                buf,
                off,
                urids.frame,
                urids.atom_long,
                &samples.to_ne_bytes(),
            )
        };
    }
    if let Some(tempo) = t.tempo_bpm {
        let bpm = tempo as f32;
        off = unsafe { write_property(buf, off, urids.bpm, urids.atom_float, &bpm.to_ne_bytes()) };
    }
    off = unsafe { write_time_signature(urids, buf, off, t) };

    // Back-patch the event header now that the object size is known.
    let object_body_size = off - obj_body_off;
    let event = unsafe { &mut *buf.add(base).cast::<LV2AtomEvent>() };
    event.time_in_frames = 0;
    event.body = LV2Atom {
        size: object_body_size as u32,
        mytype: urids.atom_object,
    };

    // Pad the whole event to the sequence's 8-byte event alignment.
    (off + 7) & !7
}

/// Append the meter and bar-position properties to a bounded time:Position atom.
///
/// # Safety
/// `buf` must retain the writable capacity preflighted by `write_time_position`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
unsafe fn write_time_signature(
    urids: &TimeUrids,
    buf: *mut u8,
    mut off: usize,
    t: &TransportInfo,
) -> usize {
    let Some((num, den)) = t.time_signature else {
        return off;
    };
    let beats_per_bar = num as f32;
    off = unsafe {
        write_property(
            buf,
            off,
            urids.beats_per_bar,
            urids.atom_float,
            &beats_per_bar.to_ne_bytes(),
        )
    };
    let beat_unit = den as i32;
    off = unsafe {
        write_property(
            buf,
            off,
            urids.beat_unit,
            urids.atom_int,
            &beat_unit.to_ne_bytes(),
        )
    };

    // Bar / barBeat need the musical position too.
    let beats_per_bar_qn = f64::from(num) * 4.0 / f64::from(den.max(1));
    if let Some(bar_start) = t.bar_start_beats {
        let bar = (bar_start / beats_per_bar_qn.max(f64::EPSILON)).round() as i64;
        off = unsafe { write_property(buf, off, urids.bar, urids.atom_long, &bar.to_ne_bytes()) };

        if let Some(beats) = t.song_position_beats {
            // Quarter notes since the bar, expressed in this signature's beats.
            let bar_beat = ((beats - bar_start) * f64::from(den) / 4.0) as f32;
            off = unsafe {
                write_property(
                    buf,
                    off,
                    urids.bar_beat,
                    urids.atom_float,
                    &bar_beat.to_ne_bytes(),
                )
            };
        }
    }
    off
}

/// One load-time classification result; port metadata and connected scalar offsets travel
/// together so activation cannot accidentally pair different plugin layouts.
struct ClassifiedPorts {
    ports: Vec<PortInfo>,
    audio_in_count: usize,
    audio_out_count: usize,
    control_value_offset: Vec<usize>,
    control_values: Vec<f32>,
    latency_port: LatencyPort,
}

/// Temporary Lilv class nodes used only while classifying one plugin's ports.
struct PortClassUris {
    audio: *mut lilv_sys::LilvNode,
    control: *mut lilv_sys::LilvNode,
    atom: *mut lilv_sys::LilvNode,
    input: *mut lilv_sys::LilvNode,
    output: *mut lilv_sys::LilvNode,
}

impl PortClassUris {
    /// # Safety
    /// `world` must remain live until this owner is dropped, which `classify_ports` ensures.
    unsafe fn new(world: *mut lilv_sys::LilvWorld) -> Self {
        Self {
            audio: unsafe {
                lilv_sys::lilv_new_uri(world, lilv_sys::LILV_URI_AUDIO_PORT.as_ptr().cast())
            },
            control: unsafe {
                lilv_sys::lilv_new_uri(world, lilv_sys::LILV_URI_CONTROL_PORT.as_ptr().cast())
            },
            atom: unsafe {
                lilv_sys::lilv_new_uri(world, lilv_sys::LILV_URI_ATOM_PORT.as_ptr().cast())
            },
            input: unsafe {
                lilv_sys::lilv_new_uri(world, lilv_sys::LILV_URI_INPUT_PORT.as_ptr().cast())
            },
            output: unsafe {
                lilv_sys::lilv_new_uri(world, lilv_sys::LILV_URI_OUTPUT_PORT.as_ptr().cast())
            },
        }
    }

    /// Classify one native port with the same class nodes used for the whole plugin.
    ///
    /// # Safety
    /// `plugin`, `port`, and every class node must remain owned by the live Lilv world.
    unsafe fn kind(
        &self,
        plugin: *const lilv_sys::LilvPlugin,
        port: *const lilv_sys::LilvPort,
        default: f32,
    ) -> PortKind {
        if unsafe { lilv_sys::lilv_port_is_a(plugin, port, self.audio) } {
            PortKind::Audio
        } else if unsafe { lilv_sys::lilv_port_is_a(plugin, port, self.control) } {
            PortKind::Control {
                default: if default.is_finite() { default } else { 0.0 },
            }
        } else if unsafe { lilv_sys::lilv_port_is_a(plugin, port, self.atom) } {
            // `atom:supports` is not inspected; unexpected MIDI atoms are ignored by the plugin.
            PortKind::AtomSequence
        } else {
            PortKind::Other
        }
    }
}

impl Drop for PortClassUris {
    fn drop(&mut self) {
        // SAFETY: each node was created once by Lilv in `new` and is owned only here.
        unsafe {
            lilv_sys::lilv_node_free(self.audio);
            lilv_sys::lilv_node_free(self.control);
            lilv_sys::lilv_node_free(self.atom);
            lilv_sys::lilv_node_free(self.input);
            lilv_sys::lilv_node_free(self.output);
        }
    }
}

unsafe fn classify_ports(
    world: *mut lilv_sys::LilvWorld,
    plugin: *const lilv_sys::LilvPlugin,
) -> ClassifiedPorts {
    let count = unsafe { lilv_sys::lilv_plugin_get_num_ports(plugin) };
    // SAFETY: both pointers refer to the retained world and its selected plugin; the temporary
    // class nodes are released before this function returns.
    let uris = unsafe { PortClassUris::new(world) };

    let mut ports = Vec::with_capacity(count as usize);
    let mut audio_in_count: usize = 0;
    let mut audio_out_count: usize = 0;
    let mut control_value_offset = Vec::with_capacity(count as usize);
    let mut control_values = Vec::new();

    let mut defaults: Vec<f32> = vec![0.0; count as usize];
    unsafe {
        // Pull every control port's declared default in one call.
        let defaults_ptr = defaults.as_mut_ptr();
        lilv_sys::lilv_plugin_get_port_ranges_float(
            plugin,
            std::ptr::null_mut(), // mins (don't care)
            std::ptr::null_mut(), // maxes (don't care)
            defaults_ptr,
        );
    }

    for idx in 0..count {
        let port = unsafe { lilv_sys::lilv_plugin_get_port_by_index(plugin, idx) };
        if port.is_null() {
            ports.push(PortInfo {
                index: idx,
                kind: PortKind::Other,
                is_input: false,
                audio_plane: None,
            });
            control_value_offset.push(usize::MAX);
            continue;
        }
        let is_input = unsafe { lilv_sys::lilv_port_is_a(plugin, port, uris.input) };
        let is_output = unsafe { lilv_sys::lilv_port_is_a(plugin, port, uris.output) };
        let kind = unsafe { uris.kind(plugin, port, defaults[idx as usize]) };
        if matches!(kind, PortKind::Audio) {
            if is_input {
                audio_in_count += 1;
            } else if is_output {
                audio_out_count += 1;
            }
        }
        if let PortKind::Control { default } = kind {
            control_value_offset.push(control_values.len());
            control_values.push(default);
        } else {
            control_value_offset.push(usize::MAX);
        }
        ports.push(PortInfo {
            index: idx,
            kind,
            is_input,
            audio_plane: None,
        });
    }

    // SAFETY: `world` and `plugin` were created together above, and the classified slices are
    // derived from this plugin's live port metadata.
    let latency_port =
        unsafe { classify_latency_port(world, plugin, &ports, &control_value_offset) };

    ClassifiedPorts {
        ports,
        audio_in_count,
        audio_out_count,
        control_value_offset,
        control_values,
        latency_port,
    }
}

struct LatencyMetadataUris {
    designation: *mut lilv_sys::LilvNode,
    latency: *mut lilv_sys::LilvNode,
    reports_latency: *mut lilv_sys::LilvNode,
    control_port: *mut lilv_sys::LilvNode,
    output_port: *mut lilv_sys::LilvNode,
}

impl LatencyMetadataUris {
    /// Create cached nodes for the LV2 latency metadata terms.
    ///
    /// # Safety
    /// `world` must be a live Lilv world that remains alive until the returned guard is dropped.
    unsafe fn new(world: *mut lilv_sys::LilvWorld) -> Self {
        // SAFETY: `world` is the live world that owns the plugin being classified, and every URI
        // byte string is NUL-terminated as required by `lilv_new_uri`.
        unsafe {
            Self {
                designation: lilv_sys::lilv_new_uri(
                    world,
                    LV2_CORE_DESIGNATION_URI.as_ptr().cast(),
                ),
                latency: lilv_sys::lilv_new_uri(world, LV2_CORE_LATENCY_URI.as_ptr().cast()),
                reports_latency: lilv_sys::lilv_new_uri(
                    world,
                    LV2_CORE_REPORTS_LATENCY_URI.as_ptr().cast(),
                ),
                control_port: lilv_sys::lilv_new_uri(
                    world,
                    lilv_sys::LILV_URI_CONTROL_PORT.as_ptr().cast(),
                ),
                output_port: lilv_sys::lilv_new_uri(
                    world,
                    lilv_sys::LILV_URI_OUTPUT_PORT.as_ptr().cast(),
                ),
            }
        }
    }

    fn is_complete(&self) -> bool {
        !self.designation.is_null()
            && !self.latency.is_null()
            && !self.reports_latency.is_null()
            && !self.control_port.is_null()
            && !self.output_port.is_null()
    }
}

impl Drop for LatencyMetadataUris {
    fn drop(&mut self) {
        for uri in [
            self.designation,
            self.latency,
            self.reports_latency,
            self.control_port,
            self.output_port,
        ] {
            if !uri.is_null() {
                // SAFETY: each non-null node is owned by this guard and was returned by
                // `lilv_new_uri`; no node is freed more than once.
                unsafe { lilv_sys::lilv_node_free(uri) };
            }
        }
    }
}

/// Resolve modern `lv2:designation lv2:latency` and deprecated `lv2:reportsLatency` metadata.
///
/// The deprecated property remains in use by third-party plugins, so it is recognized as the
/// same output-port contract. Multiple distinct or malformed reports are unknown rather than
/// silently becoming zero latency.
///
/// # Safety
/// `world` and `plugin` must be live Lilv objects from the same world, and `ports` must be the
/// metadata previously classified from that plugin.
unsafe fn classify_latency_port(
    world: *mut lilv_sys::LilvWorld,
    plugin: *const lilv_sys::LilvPlugin,
    ports: &[PortInfo],
    control_value_offset: &[usize],
) -> LatencyPort {
    // SAFETY: the caller guarantees this is the live world for `plugin`.
    let uris = unsafe { LatencyMetadataUris::new(world) };
    if !uris.is_complete() {
        return LatencyPort::Unknown;
    }

    let mut candidate_offset = None;
    let mut malformed = false;
    for info in ports {
        // SAFETY: plugin, port metadata, and URI nodes remain live for this classification call.
        match unsafe { latency_port_candidate(plugin, info, control_value_offset, &uris) } {
            LatencyCandidate::Reported(offset)
                if candidate_offset.is_some_and(|old| old != offset) =>
            {
                malformed = true;
            }
            LatencyCandidate::Reported(offset) => candidate_offset = Some(offset),
            LatencyCandidate::NotReported => {}
            LatencyCandidate::Malformed => malformed = true,
        }
    }
    if malformed {
        LatencyPort::Unknown
    } else if let Some(offset) = candidate_offset {
        LatencyPort::OutputControl { offset }
    } else {
        LatencyPort::NotReported
    }
}

/// Return the connected control offset if this port reports latency, or `Malformed` if invalid.
///
/// # Safety
/// `plugin`, its indexed port, and all nodes in `uris` must remain live for this call.
unsafe fn latency_port_candidate(
    plugin: *const lilv_sys::LilvPlugin,
    info: &PortInfo,
    control_value_offset: &[usize],
    uris: &LatencyMetadataUris,
) -> LatencyCandidate {
    // SAFETY: `info.index` belongs to the live plugin described by the caller.
    let port = unsafe { lilv_sys::lilv_plugin_get_port_by_index(plugin, info.index) };
    if port.is_null() {
        return LatencyCandidate::Malformed;
    }
    // SAFETY: `port` and the predicate/value nodes are valid objects in the same Lilv world.
    let designation = unsafe { lilv_sys::lilv_port_get(plugin, port, uris.designation) };
    let modern_report =
        !designation.is_null() && unsafe { lilv_sys::lilv_node_equals(designation, uris.latency) };
    if !designation.is_null() {
        // SAFETY: `lilv_port_get` returns an owned node that this function frees once.
        unsafe { lilv_sys::lilv_node_free(designation) };
    }
    // SAFETY: `port` and the property node are valid objects in the same Lilv world.
    let legacy_report =
        unsafe { lilv_sys::lilv_port_has_property(plugin, port, uris.reports_latency) };
    if !modern_report && !legacy_report {
        return LatencyCandidate::NotReported;
    }
    // SAFETY: the port and class nodes are valid objects in the same Lilv world.
    let valid_output = !info.is_input
        && matches!(info.kind, PortKind::Control { .. })
        && unsafe { lilv_sys::lilv_port_is_a(plugin, port, uris.control_port) }
        && unsafe { lilv_sys::lilv_port_is_a(plugin, port, uris.output_port) };
    if !valid_output {
        return LatencyCandidate::Malformed;
    }
    let Some(offset) = control_value_offset
        .get(info.index as usize)
        .copied()
        .filter(|offset| *offset != usize::MAX)
    else {
        return LatencyCandidate::Malformed;
    };
    LatencyCandidate::Reported(offset)
}

fn latency_frames_from_lv2_value(value: f32) -> Option<u32> {
    let frames = f64::from(value);
    if !frames.is_finite() || frames < 0.0 || frames.fract() != 0.0 || frames > f64::from(u32::MAX)
    {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(frames as u32)
}

fn latency_snapshot(port: LatencyPort, value: Option<f32>) -> Option<u32> {
    match port {
        LatencyPort::NotReported => Some(0),
        LatencyPort::OutputControl { .. } => value.and_then(latency_frames_from_lv2_value),
        LatencyPort::Unknown => None,
    }
}

/// Encode one truce-rack MIDI event into the (status, d1, d2, len)
/// triple LV2 atom MIDI events expect. Returns `None` for
/// non-MIDI events or unsupported MIDI variants.
fn midi_bytes(body: &EventBody) -> Option<(u8, u8, u8, usize)> {
    match body {
        EventBody::Midi(MidiData::NoteOn {
            channel,
            note,
            velocity,
        }) => Some((0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F, 3)),
        EventBody::Midi(MidiData::NoteOff {
            channel,
            note,
            velocity,
        }) => Some((0x80 | (channel & 0x0F), note & 0x7F, velocity & 0x7F, 3)),
        EventBody::Midi(MidiData::PolyAftertouch {
            channel,
            note,
            pressure,
        }) => Some((0xA0 | (channel & 0x0F), note & 0x7F, pressure & 0x7F, 3)),
        EventBody::Midi(MidiData::ControlChange {
            channel,
            controller,
            value,
        }) => Some((0xB0 | (channel & 0x0F), controller & 0x7F, value & 0x7F, 3)),
        EventBody::Midi(MidiData::ProgramChange { channel, program }) => {
            Some((0xC0 | (channel & 0x0F), program & 0x7F, 0, 2))
        }
        EventBody::Midi(MidiData::ChannelAftertouch { channel, pressure }) => {
            Some((0xD0 | (channel & 0x0F), pressure & 0x7F, 0, 2))
        }
        EventBody::Midi(MidiData::PitchBend { channel, value }) => Some((
            0xE0 | (channel & 0x0F),
            (value & 0x7F) as u8,
            ((value >> 7) & 0x7F) as u8,
            3,
        )),
        EventBody::Midi(MidiData::Raw { len, data }) if *len >= 1 && *len <= 3 => Some((
            data[0],
            *data.get(1).unwrap_or(&0),
            *data.get(2).unwrap_or(&0),
            *len as usize,
        )),
        _ => None,
    }
}

/// Suppress unused-import warning for `Event`.
#[allow(dead_code)]
fn _event_marker(_: &Event) {}

impl Drop for Lv2Plugin {
    fn drop(&mut self) {
        // Tear down the editor first so the UI's `cleanup` runs
        // while the audio instance is still alive (some UIs talk
        // to the instance via the `instance-access` extension).
        self.close_editor();
        self.free_instance();
        // `world` and `features` drop in struct order after this.
    }
}

impl Lv2Plugin {
    /// Free the live UI state (descriptor cleanup + libloading
    /// drop). Safe to call when no editor is open.
    fn close_editor(&mut self) {
        if let Some(state) = self.editor.take() {
            unsafe {
                ((*state.descriptor).cleanup)(state.handle);
            }
            // Library drops here, which closes the dlopen handle.
            drop(state);
        }
        self.drain_pending_controls();
    }

    /// Discard the current native instance, deactivating it only when the host still considers
    /// it active. This is also used after failed state restore because native restore may mutate
    /// before returning an error, so that instance cannot be safely reused.
    fn free_instance(&mut self) {
        if !self.instance.is_null() {
            // SAFETY: `instance` is owned from the matching Lilv instantiate call. Its world and
            // features remain owned by `self`; deactivate is called only for an active instance.
            unsafe {
                if self.active_layout.is_some() {
                    lilv_sys::lilv_instance_deactivate(self.instance);
                }
                lilv_sys::lilv_instance_free(self.instance);
            }
            self.instance = std::ptr::null_mut();
        }
        self.sample_rate = 0.0;
        self.active_layout = None;
    }

    /// Retain the instance when its rate and declared buffer bound are unchanged. Otherwise
    /// replace it before activation and preserve native state rather than discarding a preset.
    fn prepare_instance(&mut self, sample_rate: f64, maximum_block: usize) -> Result<()> {
        let needs_reinstantiate = configuration_requires_new_instance(
            !self.instance.is_null(),
            self.sample_rate,
            sample_rate,
            self.features.options.maximum_block(),
            maximum_block,
        );
        if !needs_reinstantiate {
            return Ok(());
        }
        self.drain_pending_controls();
        let retained_state = if self.instance.is_null() {
            None
        } else {
            Some(state::save(self)?)
        };
        self.close_editor();
        self.free_instance();
        self.features
            .options
            .configure(sample_rate, maximum_block)?;
        // SAFETY: the plugin belongs to the retained Lilv world and all feature pointers remain
        // valid in `self`; the returned instance is owned until `free_instance`.
        let instance = unsafe {
            lilv_sys::lilv_plugin_instantiate(
                self.plugin,
                sample_rate,
                self.features.feature_ptrs.as_ptr(),
            )
        };
        if instance.is_null() {
            return Err(Error::Other("lilv_plugin_instantiate returned NULL".into()));
        }
        if let Err(error) = self.parameters.set_sample_rate(sample_rate) {
            // SAFETY: the new instance has not been activated or published, and this is the
            // matching owning free before returning a failed activation to the host.
            unsafe { lilv_sys::lilv_instance_free(instance) };
            return Err(error);
        }
        self.instance = instance;
        self.sample_rate = sample_rate;
        if let Some(bytes) = retained_state
            && let Err(error) = self.restore_state(&bytes)
        {
            self.free_instance();
            return Err(error);
        }
        Ok(())
    }

    fn prepare_midi_buffers(&mut self, capacity: usize) -> Result<()> {
        resize_zeroed_buffer(&mut self.midi_in_buf, capacity)?;
        resize_zeroed_buffer(&mut self.midi_out_buf, capacity)
    }

    /// Run the part of activation that is shared by ordinary and stateful activation.
    fn activate_prepared(&mut self, layout: BusLayout) {
        // SAFETY: prepare_instance established a non-null inactive instance at the requested
        // rate, and activation runs on the same serialized lifecycle thread.
        unsafe { lilv_sys::lilv_instance_activate(self.instance) };
        self.active_layout = Some(layout);
        self.invalidate_latency_after_state_change();
    }

    fn update_latency_snapshot(&mut self) {
        let value = match self.latency_port {
            LatencyPort::NotReported | LatencyPort::Unknown => None,
            LatencyPort::OutputControl { offset } => self.control_values.get(offset).copied(),
        };
        self.latency_frames = latency_snapshot(self.latency_port, value);
    }

    fn invalidate_latency_after_state_change(&mut self) {
        self.latency_frames = match self.latency_port {
            LatencyPort::NotReported => Some(0),
            LatencyPort::OutputControl { .. } | LatencyPort::Unknown => None,
        };
    }
}

impl PluginCore for Lv2Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn active_layout(&self) -> Option<&BusLayout> {
        self.active_layout.as_ref()
    }
    fn latency_frames(&self) -> Option<u32> {
        self.latency_frames
    }
    fn supported_layouts(&self) -> &[BusLayout] {
        &self.layouts
    }
    fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
    fn parameter_info(&self, index: usize) -> Result<ParameterInfo> {
        self.parameters.info(index)
    }
    fn parameter_value(&self, index: usize) -> Result<f64> {
        let stored = self.parameters.value(index, &self.control_values)?;
        let offset = self.parameters.offset(index)?;
        Ok(self
            .controller
            .pending
            .peek(offset)
            .map_or(stored, f64::from))
    }
    fn parameter_value_string(&self, index: usize, value: f64) -> Result<String> {
        self.parameters.formatted(index, value)
    }
    fn set_parameter(&mut self, index: usize, value: f64) -> Result<()> {
        self.drain_pending_controls();
        self.parameters.set(index, value, &mut self.control_values)
    }
    fn preset_count(&self) -> usize {
        0
    }
    fn preset_info(&self, index: usize) -> Result<PresetInfo> {
        Err(Error::InvalidParameter(index))
    }
    fn load_preset(&mut self, _preset_number: i32) -> Result<()> {
        Err(Error::Other("lv2 preset loading not yet wired".into()))
    }
    fn save_state(&self) -> Result<Vec<u8>> {
        state::save(self)
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        self.restore_state(bytes)?;
        self.invalidate_latency_after_state_change();
        Ok(())
    }

    fn activate(
        &mut self,
        layout: BusLayout,
        sample_rate: f64,
        max_block_size: usize,
    ) -> Result<()> {
        self.activate_with_state(layout, sample_rate, max_block_size, None)
    }

    fn activate_with_state(
        &mut self,
        layout: BusLayout,
        sample_rate: f64,
        max_block_size: usize,
        state: Option<&[u8]>,
    ) -> Result<()> {
        let midi_buffer_capacity = validate_activation_request(
            self.is_active(),
            &self.layouts,
            &layout,
            sample_rate,
            max_block_size,
        )?;
        self.latency_frames = None;
        let mut activation = Lv2Activation {
            plugin: self,
            layout: Some(layout),
            sample_rate,
            maximum_block: max_block_size,
            midi_buffer_capacity,
        };
        restore_then_activate(&mut activation, state)
    }

    fn deactivate(&mut self) {
        if !self.instance.is_null() && self.active_layout.is_some() {
            // SAFETY: this is the retained instance created by Lilv and active_layout proves
            // activation succeeded; this call is serialized with processing and state access.
            unsafe { lilv_sys::lilv_instance_deactivate(self.instance) };
        }
        self.active_layout = None;
        self.latency_frames = None;
    }
    fn is_active(&self) -> bool {
        self.active_layout.is_some()
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        if self.ui_bundle.is_some() {
            Some(self)
        } else {
            None
        }
    }
}

trait ActivationActions {
    fn prepare_instance(&mut self) -> Result<()>;
    fn restore(&mut self, state: &[u8]) -> Result<()>;
    fn activate(&mut self) -> Result<()>;
    fn discard_after_restore_failure(&mut self);
}

struct Lv2Activation<'a> {
    plugin: &'a mut Lv2Plugin,
    layout: Option<BusLayout>,
    sample_rate: f64,
    maximum_block: usize,
    midi_buffer_capacity: usize,
}

impl ActivationActions for Lv2Activation<'_> {
    fn prepare_instance(&mut self) -> Result<()> {
        self.plugin
            .prepare_midi_buffers(self.midi_buffer_capacity)?;
        self.plugin
            .prepare_instance(self.sample_rate, self.maximum_block)
    }

    fn restore(&mut self, state: &[u8]) -> Result<()> {
        self.plugin.restore_state(state)?;
        self.plugin.invalidate_latency_after_state_change();
        Ok(())
    }

    fn activate(&mut self) -> Result<()> {
        let layout = self
            .layout
            .take()
            .ok_or_else(|| Error::Other("LV2 activation layout was already consumed".into()))?;
        self.plugin.activate_prepared(layout);
        Ok(())
    }

    fn discard_after_restore_failure(&mut self) {
        self.plugin.close_editor();
        self.plugin.free_instance();
        self.plugin.invalidate_latency_after_state_change();
    }
}

fn restore_then_activate(actions: &mut impl ActivationActions, state: Option<&[u8]>) -> Result<()> {
    actions.prepare_instance()?;
    if let Some(state) = state
        && let Err(failure) = actions.restore(state)
    {
        actions.discard_after_restore_failure();
        return Err(failure);
    }
    actions.activate()
}

fn configuration_requires_new_instance(
    instance_exists: bool,
    current_sample_rate: f64,
    requested_sample_rate: f64,
    current_maximum_block: usize,
    requested_maximum_block: usize,
) -> bool {
    !instance_exists
        || (current_sample_rate - requested_sample_rate).abs() > f64::EPSILON
        || current_maximum_block != requested_maximum_block
}

fn validate_activation_request(
    active: bool,
    supported_layouts: &[BusLayout],
    layout: &BusLayout,
    sample_rate: f64,
    max_block_size: usize,
) -> Result<usize> {
    if active {
        return Err(Error::Other(
            "stateful activation requires an inactive instance".into(),
        ));
    }
    if !supported_layouts.contains(layout) {
        return Err(Error::Other(
            "requested LV2 bus layout is not a supported native topology".into(),
        ));
    }
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(Error::Other("invalid LV2 sample rate".into()));
    }
    midi_buffer_capacity(max_block_size)
}

fn midi_buffer_capacity(max_block_size: usize) -> Result<usize> {
    if max_block_size == 0 || u32::try_from(max_block_size).is_err() {
        return Err(Error::Other("invalid LV2 maximum block size".into()));
    }
    let event_slot = std::mem::size_of::<LV2AtomEvent>()
        .checked_add(8)
        .ok_or_else(|| Error::Other("LV2 MIDI buffer size overflow".into()))?;
    let capacity = std::mem::size_of::<LV2AtomSequence>()
        .checked_add(TIME_POSITION_MAX_BYTES)
        .and_then(|base| max_block_size.checked_mul(event_slot)?.checked_add(base))
        .map(|capacity| capacity.max(64))
        .filter(|capacity| *capacity <= MAX_LV2_EVENT_BUFFER_BYTES)
        .ok_or_else(|| Error::Other("LV2 MIDI buffer exceeds the 16 MiB limit".into()))?;
    Ok(capacity)
}

fn resize_zeroed_buffer(buffer: &mut Vec<u8>, length: usize) -> Result<()> {
    if buffer.capacity() < length {
        let additional = length - buffer.len();
        buffer.try_reserve_exact(additional).map_err(|failure| {
            Error::Other(format!("LV2 MIDI buffer allocation failed: {failure}"))
        })?;
    }
    buffer.resize(length, 0);
    Ok(())
}

impl PluginEditor for Lv2Plugin {
    fn open(&mut self, parent: WindowHandle, _scale: f64) -> Result<()> {
        let bundle = self
            .ui_bundle
            .as_ref()
            .ok_or_else(|| Error::Other("lv2 plugin has no UI".into()))?
            .clone();
        if self.editor.is_some() {
            return Ok(());
        }

        let parent_ptr: *mut c_void = match parent {
            WindowHandle::NSView(p) | WindowHandle::HWND(p) => p,
            // X11 hands us a u64 XID; LV2 X11UI expects it cast
            // to a pointer (lv2 spec: "an X11 Window with the
            // proper visual"). usize cast is identity on 64-bit
            // and zero-extends on 32-bit, both fine.
            WindowHandle::X11(xid) => xid as usize as *mut c_void,
        };

        let (lib, descriptor) = open_ui_library(&bundle)?;

        // Build the UI feature list. URID map shares the same
        // backing UridMap as the audio side so URIs interned by
        // either path stay consistent.
        let map_handle: *mut UridMap = (&mut self.features.urid_map) as *mut UridMap;
        let ui_features = UiFeatureStorage::new(map_handle, parent_ptr);

        // The callback context is heap-stable from load until UI cleanup returns.
        let controller_ptr: *const Controller = self.controller.as_ref();

        let plugin_uri = CString::new(self.info.unique_id.clone())
            .map_err(|_| Error::Other("lv2 plugin uri contains NUL".into()))?;
        let bundle_path_c = path_to_cstring_with_trailing_sep(&bundle.bundle_path)?;

        let mut widget: LV2UIWidget = std::ptr::null_mut();
        let handle = unsafe {
            ((*descriptor).instantiate_raw)(
                descriptor,
                plugin_uri.as_ptr(),
                bundle_path_c.as_ptr(),
                Some(ui_write_callback),
                controller_ptr.cast::<c_void>(),
                &raw mut widget,
                ui_features.feature_ptrs.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(Error::Other("lv2 ui instantiate returned NULL".into()));
        }

        // For embedded UI types (CocoaUI / WindowsUI / X11UI) the
        // "widget" is the toolkit-native parent-of-the-UI handle
        // (NSView / HWND / X11 Window). The host's parent already
        // contains it as a child after instantiate; we just keep
        // the pointer for size queries.

        // Look up the optional idle / resize extension interfaces
        // the UI may have published. `extension_data` returns a
        // pointer to a shared static struct of function pointers.
        let idle_iface = unsafe {
            if let Some(ext) = (*descriptor).extension_data {
                ext(LV2_UI_IDLE_INTERFACE_URI.as_ptr().cast::<c_char>())
                    as *const lv2_raw::ui::LV2UIIdleInterface
            } else {
                std::ptr::null()
            }
        };
        let resize_iface = unsafe {
            if let Some(ext) = (*descriptor).extension_data {
                ext(LV2_UI_RESIZE_INTERFACE_URI.as_ptr().cast::<c_char>())
                    as *const Lv2UiResizeInterface
            } else {
                std::ptr::null()
            }
        };

        let last_pushed_values = self.control_values.clone();

        self.editor = Some(EditorState {
            _library: lib,
            descriptor,
            handle,
            widget,
            ui_features,
            idle_iface,
            resize_iface,
            last_pushed_values,
        });
        Ok(())
    }

    fn close(&mut self) {
        self.close_editor();
    }

    fn is_open(&self) -> bool {
        self.editor.is_some()
    }

    fn size(&self) -> Option<(u32, u32)> {
        let state = self.editor.as_ref()?;
        native_widget_size(state.widget)
    }

    fn is_resizable(&self) -> bool {
        // Resizable iff the UI exported a `ui:resize` extension —
        // that's the only way for the host to push a new size.
        self.editor
            .as_ref()
            .is_some_and(|e| !e.resize_iface.is_null())
    }

    fn set_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let state = self.editor.as_ref()?;
        if state.resize_iface.is_null() {
            return None;
        }
        // SAFETY: `resize_iface` was returned by the UI's
        // `extension_data(ui:resize)` and is a static struct of
        // function pointers owned by the UI bundle (still loaded
        // because `EditorState._library` is alive).
        let r = unsafe {
            ((*state.resize_iface).ui_resize)(
                (*state.resize_iface).handle,
                i32::try_from(width).unwrap_or(i32::MAX),
                i32::try_from(height).unwrap_or(i32::MAX),
            )
        };
        if r == 0 { Some((width, height)) } else { None }
    }

    fn show(&mut self) {
        // Embedded LV2 UIs are visible immediately after
        // instantiate; nothing to do.
    }

    fn hide(&mut self) {
        // No-op for embedded UIs — closing the editor or the host
        // window is the visibility primitive.
    }

    fn on_idle(&mut self) {
        self.drain_pending_controls();
        let Some(state) = self.editor.as_mut() else {
            return;
        };
        // 1. Drive the optional `ui:idleInterface` first — animations
        //    and the UI's own event pump live here. Non-zero return
        //    means the UI closed itself; we bail and let the next
        //    on_idle skip cleanly.
        if !state.idle_iface.is_null() {
            let rc = unsafe { ((*state.idle_iface).idle)(state.handle) };
            if rc != 0 {
                // UI asked to be torn down. close_editor walks the
                // descriptor's cleanup and drops the library.
                self.close_editor();
                return;
            }
        }
        self.drain_pending_controls();

        // 2. Push host-side parameter changes to the UI via
        //    `port_event`. Compare current control_values against
        //    the snapshot we took last tick; for any port whose value
        //    differs, fire a float-protocol port_event so the UI
        //    redraws. Snapshot is then refreshed.
        let Some(state) = self.editor.as_mut() else {
            return;
        };
        // SAFETY: `descriptor` is the same one we instantiated; its
        // `port_event` field is non-null per the LV2 spec.
        let port_event_fn = unsafe { (*state.descriptor).port_event };
        for (port_index, &offset) in self.control_value_offset.iter().enumerate() {
            if offset == usize::MAX || offset >= self.control_values.len() {
                continue;
            }
            let cur = self.control_values[offset];
            // First idle tick after a re-open may have a shorter
            // snapshot than control_values — guard via .get.
            let prev = state
                .last_pushed_values
                .get(offset)
                .copied()
                .unwrap_or(cur + 1.0);
            // f32 inequality is fine here — we only push when the
            // bit pattern actually changed, NaNs included (NaN != NaN
            // is the right answer; both sides will have NaN if the
            // plugin keeps writing NaN, no spurious push).
            #[allow(clippy::float_cmp)]
            let changed = cur != prev;
            if changed {
                let value = cur;
                let port_index_u32 = u32::try_from(port_index).unwrap_or(u32::MAX);
                port_event_fn(
                    state.handle,
                    port_index_u32,
                    4,
                    0, // float protocol
                    (&raw const value).cast::<c_void>(),
                );
                if offset < state.last_pushed_values.len() {
                    state.last_pushed_values[offset] = cur;
                }
            }
        }
        // suppress unused-warning if no port matched
        let _ = port_event_fn;

        // 3. Apply any UI-requested resize. The host-side ui:resize
        //    callback writes into the notifier; on_idle is the host's
        //    chance to act on it. We can't actually resize the
        //    baseview window from inside this trait method (no window
        //    handle), so we just stash the request — windowed.rs polls
        //    `size()` next frame and resizes accordingly.
        // Currently no-op past the notifier write; future host-driven
        // window resize would consume `notifier.pending.take()` here.
    }
}

fn path_to_cstring_with_trailing_sep(p: &Path) -> Result<CString> {
    let mut s = p
        .to_str()
        .ok_or_else(|| Error::Other("LV2 bundle path is not UTF-8".into()))?
        .to_owned();
    if !s.ends_with(std::path::MAIN_SEPARATOR) {
        s.push(std::path::MAIN_SEPARATOR);
    }
    CString::new(s).map_err(|_| Error::Other("LV2 bundle path contains NUL".into()))
}

fn exact_bundle_path(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    if !path.is_dir()
        || path.extension().and_then(|extension| extension.to_str()) != Some("lv2")
        || !path.join("manifest.ttl").is_file()
    {
        return Err(Error::Other(
            "LV2 path must be one .lv2 bundle containing manifest.ttl".into(),
        ));
    }
    Ok(path)
}

#[cfg(target_os = "macos")]
fn native_widget_size(widget: LV2UIWidget) -> Option<(u32, u32)> {
    use objc2::msg_send;
    use objc2_foundation::NSRect;
    if widget.is_null() {
        return None;
    }
    // SAFETY: For CocoaUI the widget is an NSView*. Reading
    // -frame is always safe on a live NSView; the LV2 spec
    // promises the widget stays valid until cleanup.
    let view = widget as *mut objc2::runtime::AnyObject;
    let frame: NSRect = unsafe { msg_send![view, frame] };
    // `.max(0.0)` clamps the negative branch the sign-loss lint
    // worries about; window dimensions can't reasonably overflow u32.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    Some((
        frame.size.width.max(0.0) as u32,
        frame.size.height.max(0.0) as u32,
    ))
}

#[cfg(not(target_os = "macos"))]
fn native_widget_size(_widget: LV2UIWidget) -> Option<(u32, u32)> {
    // X11 / Win32 paths would query XGetWindowAttributes /
    // GetClientRect respectively; truce-rack-standalone falls back to
    // its INITIAL_WINDOW until that's wired.
    None
}

impl Plugin<f32> for Lv2Plugin {
    fn process(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        events: &EventList,
        context: &mut ProcessContext<'_>,
    ) -> Result<ProcessStatus> {
        if !self.is_active() {
            return Err(Error::NotActivated);
        }
        self.drain_pending_controls();
        let frames = buffer.num_frames();
        if frames == 0 {
            return Ok(ProcessStatus::Continue);
        }

        // Validate the exact bus topology and mapping before touching any native port.
        let layout = self.active_layout.as_ref().ok_or(Error::NotActivated)?;
        let maximum_block = context
            .max_block_size
            .min(self.features.options.maximum_block());
        let planes = AudioPlanes::collect(buffer, layout, maximum_block)?;
        planes.validate_ports(&self.ports)?;

        // Build the input atom sequence (host transport + MIDI) and
        // reset the output.
        self.build_midi_sequence(events, context.transport);
        self.prep_midi_out();

        let control_base = self.control_values.as_mut_ptr();
        let midi_in_ptr = self.midi_in_buf.as_mut_ptr().cast::<c_void>();
        let midi_out_ptr = self.midi_out_buf.as_mut_ptr().cast::<c_void>();

        // Connect each native port using its load-time raw-port → flattened-plane mapping.
        for port in &self.ports {
            let data: *mut c_void = match port.kind {
                PortKind::Audio => {
                    let plane = port.audio_plane.expect("audio mapping preflighted above");
                    if port.is_input {
                        planes.inputs[plane].cast::<c_void>()
                    } else {
                        planes.outputs[plane].cast::<c_void>()
                    }
                }
                PortKind::Control { .. } => {
                    let off = self.control_value_offset[port.index as usize];
                    if off == usize::MAX {
                        std::ptr::null_mut()
                    } else {
                        // SAFETY: `control_base` is the head of the
                        // `control_values` Vec; `off` was computed
                        // from its push order at load time.
                        unsafe { control_base.add(off).cast::<c_void>() }
                    }
                }
                PortKind::AtomSequence => {
                    if port.is_input {
                        midi_in_ptr
                    } else {
                        midi_out_ptr
                    }
                }
                PortKind::Other => std::ptr::null_mut(),
            };
            unsafe {
                lilv_sys::lilv_instance_connect_port(self.instance, port.index, data);
            }
        }

        unsafe {
            lilv_sys::lilv_instance_run(self.instance, frames as u32);
        }
        // LV2 exposes changing latency as a connected output control-port scalar, not a getter.
        // Copy that already-written value after run; this performs no FFI query, allocation, or lock.
        self.update_latency_snapshot();

        // Drain MIDI from the output atom-sequence port (if the
        // plugin connected one) into context.output_events.
        self.drain_midi_out(context);

        Ok(ProcessStatus::Continue)
    }
}

impl Lv2Plugin {
    /// Walk the MIDI output atom-sequence buffer the plugin just
    /// wrote and translate every `MidiEvent`-typed event back into
    /// rack2-core `EventList` events on `context.output_events`.
    fn drain_midi_out(&mut self, context: &mut ProcessContext<'_>) {
        let header_size = std::mem::size_of::<LV2AtomSequence>();
        if self.midi_out_buf.len() < header_size {
            return;
        }
        let buf = self.midi_out_buf.as_ptr();
        // SAFETY: midi_out_buf is sized for an LV2AtomSequence header
        // plus events at activate. Vec<u8> is heap-aligned to >= 8.
        let seq = unsafe { &*buf.cast::<LV2AtomSequence>() };
        // Sequence body size (atom.size) excludes the LV2Atom header
        // itself; the body's events run from sequence_begin to
        // sequence_end.
        let body_size = seq.atom.size;
        let event_header = std::mem::size_of::<LV2AtomEvent>();
        let body_offset = std::mem::size_of::<LV2Atom>();
        let mut cursor = body_offset + std::mem::size_of::<LV2AtomSequenceBody>();
        let body_end = body_offset + body_size as usize;
        while cursor + event_header <= body_end && cursor + event_header <= self.midi_out_buf.len()
        {
            let ev_ptr = unsafe { buf.add(cursor) }.cast::<LV2AtomEvent>();
            // SAFETY: cursor + event_header <= midi_out_buf.len().
            let ev = unsafe { &*ev_ptr };
            let payload_size = ev.body.size as usize;
            let payload_total = event_header + payload_size;
            if cursor + payload_total > body_end {
                break;
            }
            if ev.body.mytype == self.midi_event_urid && (1..=8).contains(&payload_size) {
                let data_ptr = unsafe { (ev_ptr as *const u8).add(event_header) };
                // SAFETY: payload_size bytes immediately follow the
                // event header within the bounds we just checked.
                let bytes = unsafe { std::slice::from_raw_parts(data_ptr, payload_size) };
                if let Some(body) = decode_midi_atom(bytes) {
                    let offset = u32::try_from(ev.time_in_frames.max(0)).unwrap_or(0);
                    context.output_events.push(Event {
                        sample_offset: offset,
                        body,
                    });
                }
            }
            // Advance cursor past this event, padded to 8 bytes per
            // the atom alignment rule.
            cursor += (payload_total + 7) & !7;
        }
    }
}

/// Inverse of `midi_bytes` — turn 1-3 bytes of LV2 MIDI atom payload
/// into a typed `EventBody`. Anything longer (sysex etc.) is wrapped
/// in `MidiData::Raw` up to the 8-byte cap.
fn decode_midi_atom(bytes: &[u8]) -> Option<EventBody> {
    if bytes.is_empty() {
        return None;
    }
    let status = bytes[0];
    let channel = status & 0x0F;
    let kind = status & 0xF0;
    let body = match (kind, bytes) {
        (0x80, [_, note, vel]) => MidiData::NoteOff {
            channel,
            note: *note,
            velocity: *vel,
        },
        (0x90, [_, note, 0]) => MidiData::NoteOff {
            channel,
            note: *note,
            velocity: 0,
        },
        (0x90, [_, note, vel]) => MidiData::NoteOn {
            channel,
            note: *note,
            velocity: *vel,
        },
        (0xA0, [_, note, pressure]) => MidiData::PolyAftertouch {
            channel,
            note: *note,
            pressure: *pressure,
        },
        (0xB0, [_, controller, value]) => MidiData::ControlChange {
            channel,
            controller: *controller,
            value: *value,
        },
        (0xC0, [_, program]) => MidiData::ProgramChange {
            channel,
            program: *program,
        },
        (0xD0, [_, pressure]) => MidiData::ChannelAftertouch {
            channel,
            pressure: *pressure,
        },
        (0xE0, [_, lsb, msb]) => MidiData::PitchBend {
            channel,
            value: u16::from(*msb) << 7 | u16::from(*lsb),
        },
        _ if bytes.len() <= 8 => {
            let mut data = [0u8; 8];
            data[..bytes.len()].copy_from_slice(bytes);
            #[allow(clippy::cast_possible_truncation)]
            MidiData::Raw {
                len: bytes.len() as u8,
                data,
            }
        }
        _ => return None,
    };
    Some(EventBody::Midi(body))
}

#[cfg(test)]
mod bundle_scan_tests {
    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/multibus.lv2")
    }

    #[test]
    fn exact_bundle_scan_retains_uri_identity_and_requested_path() {
        let path = fixture().canonicalize().unwrap();
        let infos = Lv2Scanner::new().scan_path(&path).unwrap();
        assert_eq!(infos.len(), 5);
        assert!(infos.iter().all(|info| info.path == path));
        assert!(infos.iter().all(|info| {
            info.format == FORMAT && info.unique_id.starts_with("urn:mixrelay:multibus-")
        }));
    }

    #[test]
    fn unrelated_paths_are_rejected_before_native_discovery() {
        let bundle = fixture();
        for path in [
            bundle.join("missing.lv2"),
            bundle.join("manifest.ttl"),
            bundle.parent().unwrap().to_owned(),
        ] {
            assert!(Lv2Scanner::new().scan_path(&path).is_err());
        }
    }

    #[test]
    fn exact_bundle_load_rejects_an_unlisted_uri_before_instantiation() {
        let mut info = Lv2Scanner::new().scan_path(&fixture()).unwrap().remove(0);
        info.unique_id = "urn:mixrelay:not-in-selected-bundle".to_owned();
        assert!(matches!(
            Lv2Plugin::load_from(&info),
            Err(Error::LoadFailed { reason, .. }) if reason.contains("no LV2 plugin matching URI")
        ));
    }

    #[test]
    fn native_bundle_path_has_a_trailing_separator_without_lossy_aliases() {
        let path = Path::new("/private/selected % bundle.lv2");
        let encoded = path_to_cstring_with_trailing_sep(path).unwrap();
        assert_eq!(
            encoded.to_str().unwrap(),
            format!("{}{}", path.display(), std::path::MAIN_SEPARATOR)
        );
        assert!(path_to_cstring_with_trailing_sep(Path::new("/invalid\0.lv2")).is_err());
    }
}

#[cfg(test)]
mod latency_tests {
    use super::*;

    #[test]
    fn absent_report_is_zero_but_malformed_metadata_is_unknown() {
        assert_eq!(latency_snapshot(LatencyPort::NotReported, None), Some(0));
        assert_eq!(latency_snapshot(LatencyPort::Unknown, Some(0.0)), None);
    }

    #[test]
    fn latency_port_scalar_requires_a_finite_nonnegative_integer_frame_count() {
        assert_eq!(latency_frames_from_lv2_value(0.0), Some(0));
        assert_eq!(latency_frames_from_lv2_value(128.0), Some(128));
        assert_eq!(latency_frames_from_lv2_value(f32::NAN), None);
        assert_eq!(latency_frames_from_lv2_value(f32::INFINITY), None);
        assert_eq!(latency_frames_from_lv2_value(-1.0), None);
        assert_eq!(latency_frames_from_lv2_value(1.5), None);
        assert_eq!(
            latency_frames_from_lv2_value(4_294_967_040.0),
            Some(4_294_967_040)
        );
        assert_eq!(latency_frames_from_lv2_value(4_294_967_296.0), None);
    }

    #[test]
    fn connected_latency_port_snapshot_can_change_between_runs() {
        let port = LatencyPort::OutputControl { offset: 2 };
        assert_eq!(latency_snapshot(port, Some(24.0)), Some(24));
        assert_eq!(latency_snapshot(port, Some(72.0)), Some(72));
        assert_eq!(latency_snapshot(port, None), None);
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    #[derive(Default)]
    struct FakeActivation {
        calls: Vec<&'static str>,
        restore_failure: bool,
        activation_failure: bool,
    }

    impl ActivationActions for FakeActivation {
        fn prepare_instance(&mut self) -> Result<()> {
            self.calls.push("prepare");
            Ok(())
        }

        fn restore(&mut self, state: &[u8]) -> Result<()> {
            assert_eq!(state, b"fixture-state");
            self.calls.push("restore");
            if self.restore_failure {
                return Err(Error::Other("fake restore failure".into()));
            }
            Ok(())
        }

        fn activate(&mut self) -> Result<()> {
            self.calls.push("activate");
            if self.activation_failure {
                return Err(Error::Other("fake activation failure".into()));
            }
            Ok(())
        }

        fn discard_after_restore_failure(&mut self) {
            self.calls.push("discard");
        }
    }

    #[test]
    fn state_is_restored_before_native_activation() {
        let mut lifecycle = FakeActivation::default();
        restore_then_activate(&mut lifecycle, Some(b"fixture-state")).unwrap();
        assert_eq!(lifecycle.calls, ["prepare", "restore", "activate"]);
    }

    #[test]
    fn no_state_uses_the_shared_activation_path_without_restore() {
        let mut lifecycle = FakeActivation::default();
        restore_then_activate(&mut lifecycle, None).unwrap();
        assert_eq!(lifecycle.calls, ["prepare", "activate"]);
    }

    #[test]
    fn failed_restore_discards_instance_and_never_activates() {
        let mut lifecycle = FakeActivation {
            restore_failure: true,
            ..FakeActivation::default()
        };
        let error = restore_then_activate(&mut lifecycle, Some(b"fixture-state")).unwrap_err();
        assert_eq!(error.to_string(), "fake restore failure");
        assert_eq!(lifecycle.calls, ["prepare", "restore", "discard"]);
    }

    #[test]
    fn activation_failure_is_propagated_after_restore() {
        let mut lifecycle = FakeActivation {
            activation_failure: true,
            ..FakeActivation::default()
        };
        let error = restore_then_activate(&mut lifecycle, Some(b"fixture-state")).unwrap_err();
        assert_eq!(error.to_string(), "fake activation failure");
        assert_eq!(lifecycle.calls, ["prepare", "restore", "activate"]);
    }

    #[test]
    fn same_rate_keeps_the_deactivated_native_instance() {
        assert!(!configuration_requires_new_instance(
            true, 48_000.0, 48_000.0, 128, 128
        ));
        assert!(configuration_requires_new_instance(
            true, 48_000.0, 44_100.0, 128, 128
        ));
        assert!(configuration_requires_new_instance(
            false, 48_000.0, 48_000.0, 128, 128
        ));
        assert!(configuration_requires_new_instance(
            true, 48_000.0, 48_000.0, 128, 256
        ));
    }

    #[test]
    fn activation_validation_rejects_unsupported_layouts_and_invalid_rates() {
        let supported = [BusLayout::stereo()];
        assert!(
            validate_activation_request(false, &supported, &BusLayout::mono(), 48_000.0, 128,)
                .is_err()
        );
        for sample_rate in [0.0, f64::NAN, f64::INFINITY] {
            assert!(
                validate_activation_request(false, &supported, &supported[0], sample_rate, 128,)
                    .is_err()
            );
        }
        assert!(
            validate_activation_request(true, &supported, &supported[0], 48_000.0, 128,).is_err()
        );
    }

    #[test]
    fn activation_validation_rejects_unbounded_or_zero_block_sizes() {
        let supported = [BusLayout::stereo()];
        for max_block_size in [0, usize::MAX] {
            assert!(
                validate_activation_request(
                    false,
                    &supported,
                    &supported[0],
                    48_000.0,
                    max_block_size,
                )
                .is_err()
            );
        }
        assert!(midi_buffer_capacity(4_096).is_ok());
        let overhead = std::mem::size_of::<LV2AtomSequence>() + TIME_POSITION_MAX_BYTES;
        let event_slot = std::mem::size_of::<LV2AtomEvent>() + 8;
        let maximum_frames = (MAX_LV2_EVENT_BUFFER_BYTES - overhead) / event_slot;
        assert!(midi_buffer_capacity(maximum_frames).is_ok());
        assert!(midi_buffer_capacity(maximum_frames + 1).is_err());
    }
}
