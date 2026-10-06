//! VST3 host implementation for the truce-rack framework.
//!
//! Built on the community `vst3` Rust bindings — no Steinberg SDK
//! submodule, no cmake. A fresh checkout builds in seconds.
//!
//! # Lifecycle
//!
//! Each `.vst3` bundle exports `GetPluginFactory` plus its platform
//! module entry and exit points. Loading walks:
//!
//! 1. `dlopen` the binary inside the bundle.
//! 2. The platform entry point (`bundleEntry`, `ModuleEntry`, or optional `InitDll`).
//! 3. `GetPluginFactory()` → `IPluginFactory`.
//! 4. `createInstance(cid, IComponent::IID)` → `IComponent`.
//! 5. `IPluginBase::initialize(host_context)` on the component.
//! 6. Cast to `IAudioProcessor`.
//! 7. `IComponent::getControllerClassId(&mut cid2)` — if separate,
//!    `factory.createInstance(cid2, IEditController::IID)` and
//!    initialize it. Otherwise cast the component itself.
//! 8. Connect the component and controller `IConnectionPoint`s
//!    (separate-controller case only).
//!
//! Activate calls `setBusArrangements`, `setupProcessing`,
//! `setActive(true)`, `setProcessing(true)`. Deactivate reverses.

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

use std::path::{Path, PathBuf};
use std::ptr;
use std::{ffi::OsStr, fs::ReadDir};

use vst3::Steinberg::Vst::{
    Event, Event_::EventTypes_, Event__type0, IAudioProcessor, IAudioProcessorTrait, IComponent,
    IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController,
    IEditControllerTrait, IEventList, IEventListTrait, IParameterChanges, NoteOffEvent,
    NoteOnEvent, ParameterInfo as Vst3ParameterInfo, ParameterInfo_::ParameterFlags_,
    PolyPressureEvent, ProcessContext as Vst3ProcessContext, ProcessContext_::StatesAndFlags_,
    ProcessData, ProcessModes_, ProcessSetup, SymbolicSampleSizes_, ViewType,
};
use vst3::Steinberg::{
    IPlugView, IPlugViewTrait, IPluginBase, IPluginBaseTrait, IPluginFactory, IPluginFactoryTrait,
    PClassInfo, PClassInfo_, TUID, ViewRect, kPlatformTypeHWND, kPlatformTypeNSView,
    kPlatformTypeX11EmbedWindowID, kResultOk, kResultTrue,
};
use vst3::{Class, ComPtr, ComWrapper};

mod activation;
mod bus_layout;
mod editor_frame;
mod host_context;
mod lifecycle;
mod parameter_delivery;
mod plugin_state;
mod state_stream;

/// Format identifier used on returned [`PluginInfo`].
pub const FORMAT: &str = "vst3";

/// Bundle directory suffix every VST3 plugin uses.
pub const VST3_EXTENSION: &str = ".vst3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatencyServiceAction {
    Restart,
    Refresh,
}

fn latency_service_action(latency_changed: bool) -> LatencyServiceAction {
    if latency_changed {
        LatencyServiceAction::Restart
    } else {
        LatencyServiceAction::Refresh
    }
}

fn latency_refresh_allowed(
    activation_ready: bool,
    editor_open: bool,
    pending_changes: bool,
) -> bool {
    activation_ready && !editor_open && !pending_changes
}

fn refresh_latency_cache(
    snapshot: &mut Option<u32>,
    revision: u64,
    query: impl FnOnce() -> u32,
    publish: impl FnOnce(u64) -> bool,
) -> bool {
    let frames = query();
    if !publish(revision) {
        *snapshot = None;
        return false;
    }
    *snapshot = Some(frames);
    true
}

fn latency_revision_is_current(
    captured_revision: u64,
    current_revision: u64,
    pending: bool,
) -> bool {
    captured_revision == current_revision && !pending
}

/// Symbol name `GetPluginFactory` plugins export. Same on every OS.
const GET_FACTORY_SYMBOL: &[u8] = b"GetPluginFactory\0";

#[cfg(target_os = "linux")]
const MODULE_ENTRY_SYMBOL: &[u8] = b"ModuleEntry\0";
#[cfg(target_os = "linux")]
const MODULE_EXIT_SYMBOL: &[u8] = b"ModuleExit\0";

#[cfg(target_os = "windows")]
const DLL_ENTRY_SYMBOL: &[u8] = b"InitDll\0";
#[cfg(target_os = "windows")]
const DLL_EXIT_SYMBOL: &[u8] = b"ExitDll\0";

/// macOS-only bundle entry point.
#[cfg(target_os = "macos")]
const BUNDLE_ENTRY_SYMBOL: &[u8] = b"bundleEntry\0";

/// macOS-only bundle exit point.
#[cfg(target_os = "macos")]
const BUNDLE_EXIT_SYMBOL: &[u8] = b"bundleExit\0";

/// VST3 scanner.
#[derive(Debug, Default)]
pub struct Vst3Scanner;

impl Vst3Scanner {
    /// Construct a default scanner.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PluginScanner for Vst3Scanner {
    type Plugin = Vst3Plugin;

    fn scan(&self) -> Result<Vec<PluginInfo>> {
        let mut out = Vec::new();
        for dir in default_vst3_paths() {
            scan_optional_default_dir(&dir, &mut out)?;
        }
        Ok(out)
    }

    fn scan_path(&self, path: &Path) -> Result<Vec<PluginInfo>> {
        let mut out = Vec::new();
        if path.extension() == Some(OsStr::new("vst3")) {
            scan_bundle(path, &mut out)?;
        } else {
            scan_dir_into(path, &mut out)?;
        }
        Ok(out)
    }

    fn load(&self, info: &PluginInfo) -> Result<Self::Plugin> {
        Vst3Plugin::load_from(info)
    }
}

/// Standard VST3 install locations for the current OS.
#[must_use]
pub fn default_vst3_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let mut user = PathBuf::from(home);
        #[cfg(target_os = "macos")]
        user.push("Library/Audio/Plug-Ins/VST3");
        #[cfg(target_os = "linux")]
        user.push(".vst3");
        #[cfg(target_os = "windows")]
        user.push("AppData/Local/Programs/Common/VST3");
        out.push(user);
    }
    #[cfg(target_os = "macos")]
    out.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
    #[cfg(target_os = "linux")]
    out.push(PathBuf::from("/usr/lib/vst3"));
    #[cfg(target_os = "windows")]
    {
        if let Some(pf) = std::env::var_os("CommonProgramFiles") {
            let mut p = PathBuf::from(pf);
            p.push("VST3");
            out.push(p);
        }
    }
    out
}

fn scan_optional_default_dir(dir: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(scan_directory_error(dir, &error)),
    };
    scan_entries(dir, entries, out)
}

fn scan_dir_into(dir: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|error| scan_directory_error(dir, &error))?;
    scan_entries(dir, entries, out)
}

fn scan_entries(dir: &Path, entries: ReadDir, out: &mut Vec<PluginInfo>) -> Result<()> {
    for entry in entries {
        let entry = entry.map_err(|error| scan_directory_error(dir, &error))?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("vst3")) {
            continue;
        }
        scan_bundle(&path, out)?;
    }
    Ok(())
}

fn scan_bundle(bundle: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let mut bundle_plugins = Vec::new();
    scan_bundle_into(bundle, &mut bundle_plugins)
        .map_err(|error| scan_bundle_error(bundle, &error))?;
    out.extend(bundle_plugins);
    Ok(())
}

fn scan_directory_error(dir: &Path, error: &std::io::Error) -> Error {
    Error::Other(format!(
        "failed to read VST3 scan directory {}: {error}",
        dir.display()
    ))
}

fn scan_bundle_error(bundle: &Path, error: &Error) -> Error {
    Error::Other(format!(
        "failed to scan VST3 bundle {}: {error}",
        bundle.display()
    ))
}

fn scan_bundle_into(bundle: &Path, out: &mut Vec<PluginInfo>) -> Result<()> {
    let module = unsafe { LoadedModule::open(bundle) }?;
    let factory = module.factory()?;
    let count = unsafe { factory.countClasses() };
    if count < 0 {
        return Err(Error::Other(format!(
            "IPluginFactory::countClasses returned invalid count {count}"
        )));
    }
    let mut info = empty_pclass_info();
    for idx in 0..count {
        let status = unsafe { factory.getClassInfo(idx, &raw mut info) };
        if status != kResultOk {
            return Err(Error::Format {
                format: FORMAT,
                code: i64::from(status),
                message: format!("IPluginFactory::getClassInfo({idx}) failed"),
            });
        }
        let category = char8_array_to_string(&info.category);
        if category != "Audio Module Class" {
            continue;
        }
        let name = char8_array_to_string(&info.name);
        out.push(PluginInfo {
            name,
            vendor: String::new(),
            version: 0,
            category: PluginCategory::Effect,
            path: bundle.to_path_buf(),
            unique_id: tuid_to_hex(&info.cid),
            format: FORMAT,
            has_editor: false,
            accepts_midi: false,
        });
    }
    Ok(())
}

fn empty_pclass_info() -> PClassInfo {
    PClassInfo {
        cid: [0; 16],
        cardinality: 0,
        category: [0; PClassInfo_::kCategorySize as usize],
        name: [0; PClassInfo_::kNameSize as usize],
    }
}

fn char8_array_to_string(array: &[vst3::Steinberg::char8]) -> String {
    // `char8` is signed on some targets and unsigned on others. Its native
    // representation is one byte, so this preserves the original UTF-8 bytes.
    let bytes: Vec<u8> = array
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&byte| byte.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn tuid_to_hex(cid: &TUID) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(32);
    for &byte in cid {
        let _ = write!(s, "{:02x}", byte.to_ne_bytes()[0]);
    }
    s
}

fn hex_to_tuid(hex: &str) -> Option<TUID> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out: TUID = [0; 16];
    for i in 0..16 {
        let byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        out[i] = std::ffi::c_char::from_ne_bytes([byte]);
    }
    Some(out)
}

#[cfg(target_os = "macos")]
mod mac {
    //! `CFBundle`-backed loader for VST3 bundles on macOS.
    //!
    //! Plugins like Surge XT Effects call into `CFPlugin` during
    //! `bundleEntry` (specifically `CFPlugInRegisterFactories`,
    //! which lives behind the `AddInstanceForFactory` log line).
    //! Those APIs only work when the dylib was loaded through a
    //! registered `CFBundle` — raw `dlopen` leaves the bundle
    //! unknown to CoreFoundation and the plugin dereferences
    //! garbage. Going through `CFBundleLoadExecutable` gives the
    //! plugin the context it expects.

    use std::path::{Path, PathBuf};

    use core_foundation::base::TCFType;
    use core_foundation::bundle::CFBundle;
    use core_foundation::string::CFString;
    use core_foundation::url::{CFURL, kCFURLPOSIXPathStyle};

    use truce_rack_core::error::{Error, Result};

    pub(super) struct MacBundle {
        bundle: CFBundle,
        path: PathBuf,
    }

    impl MacBundle {
        pub(super) fn open(bundle_path: &Path) -> Result<Self> {
            let path_str = bundle_path.to_str().ok_or_else(|| Error::LoadFailed {
                path: bundle_path.to_path_buf(),
                reason: "bundle path is not valid UTF-8".into(),
            })?;
            let cf_path = CFString::new(path_str);
            // CFURLCreateWithFileSystemPath with `isDirectory = true`
            // is what every reference VST3 host uses on macOS — a
            // bundle URL has to be flagged as a directory or
            // CFBundleCreate silently picks the wrong layout.
            let url = unsafe {
                use core_foundation_sys::base::kCFAllocatorDefault;
                use core_foundation_sys::url::CFURLCreateWithFileSystemPath;
                let raw = CFURLCreateWithFileSystemPath(
                    kCFAllocatorDefault,
                    cf_path.as_concrete_TypeRef(),
                    kCFURLPOSIXPathStyle,
                    1,
                );
                if raw.is_null() {
                    return Err(Error::LoadFailed {
                        path: bundle_path.to_path_buf(),
                        reason: "CFURLCreateWithFileSystemPath returned NULL".into(),
                    });
                }
                CFURL::wrap_under_create_rule(raw)
            };

            let bundle = CFBundle::new(url).ok_or_else(|| Error::LoadFailed {
                path: bundle_path.to_path_buf(),
                reason: "CFBundleCreate returned NULL".into(),
            })?;

            // CFBundleLoadExecutable must run before any function
            // pointer lookup. Returns true on success.
            let loaded = unsafe {
                use core_foundation_sys::bundle::CFBundleLoadExecutable;
                CFBundleLoadExecutable(bundle.as_concrete_TypeRef()) != 0
            };
            if !loaded {
                return Err(Error::LoadFailed {
                    path: bundle_path.to_path_buf(),
                    reason: "CFBundleLoadExecutable failed".into(),
                });
            }

            Ok(Self {
                bundle,
                path: bundle_path.to_path_buf(),
            })
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        /// The raw `CFBundleRef`, type-erased to `*mut c_void` so
        /// the call site doesn't need to depend on
        /// `core-foundation-sys`. Hand this to `bundleEntry` —
        /// it's the host's identity to the plugin.
        pub(super) fn raw(&self) -> *mut std::ffi::c_void {
            self.bundle.as_concrete_TypeRef().cast::<std::ffi::c_void>()
        }

        /// Look up an exported symbol. `name` may end in a trailing
        /// NUL (we strip it before handing the text to `CFString`).
        pub(super) unsafe fn function_ptr(&self, name: &[u8]) -> Option<*mut std::ffi::c_void> {
            let name = match name.split_last() {
                Some((0, rest)) => rest,
                _ => name,
            };
            let name_str = std::str::from_utf8(name).ok()?;
            let cf_name = CFString::new(name_str);
            let ptr = unsafe {
                use core_foundation_sys::bundle::CFBundleGetFunctionPointerForName;
                CFBundleGetFunctionPointerForName(
                    self.bundle.as_concrete_TypeRef(),
                    cf_name.as_concrete_TypeRef(),
                )
            };
            if ptr.is_null() {
                None
            } else {
                Some(ptr.cast_mut().cast::<std::ffi::c_void>())
            }
        }
    }

    // SAFETY: CFBundle is reference-counted by CoreFoundation; we
    // hold one owned reference and CoreFoundation itself is
    // thread-safe for read access. The Drop impl releases the
    // CFBundle but deliberately never calls
    // `CFBundleUnloadExecutable` — VST3 plugins leave Objective-C
    // class registrations and runloop callbacks pointing into the
    // dylib, and unloading invalidates them. Same "don't dlclose"
    // discipline truce-loader follows on the plugin side.
    unsafe impl Send for MacBundle {}
}

/// Per-platform VST3 bundle layout. Linux uses
/// `Contents/<arch>-linux/<stem>.so`; Windows uses
/// `Contents/<arch>-win/<stem>.vst3`. macOS goes through `CFBundle`
/// instead (see [`mac::MacBundle`]) so its binary lookup lives
/// there.
#[cfg(not(target_os = "macos"))]
fn bundle_binary_path(bundle: &Path) -> PathBuf {
    let stem = bundle
        .file_stem()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    {
        if bundle.is_dir() {
            let arch_dir = format!("{}-linux", std::env::consts::ARCH);
            let mut binary = stem.clone();
            binary.push(".so");
            return bundle.join("Contents").join(arch_dir).join(binary);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if bundle.is_dir() {
            let arch_dir = windows_arch_dir(std::env::consts::ARCH);
            let mut binary = stem.clone();
            binary.push(".vst3");
            return bundle.join("Contents").join(arch_dir).join(binary);
        }
    }
    let _ = stem;
    bundle.to_path_buf()
}

#[cfg(any(target_os = "windows", test))]
fn windows_arch_dir(arch: &str) -> String {
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    format!("{arch}-win")
}

/// RAII wrapper around the loaded module. On macOS this is a real
/// `CFBundle` so the plugin's `bundleEntry` sees the `CFPlugin` /
/// `CFBundleGetIdentifier` context it expects (raw dlopen crashes
/// some bundles — Surge XT Effects calls into `CFPlugin`'s
/// `AddInstanceForFactory` during init). On Linux / Windows the
/// underlying file is a plain dynamic library; `libloading` owns its
/// handle while platform exit runs before unload.
#[cfg(not(target_os = "macos"))]
struct LoadedModule {
    library: libloading::Library,
    exit: Option<unsafe extern "system" fn() -> bool>,
}

#[cfg(target_os = "macos")]
struct LoadedModule {
    bundle: mac::MacBundle,
    entered: bool,
}

#[cfg(not(target_os = "macos"))]
impl LoadedModule {
    unsafe fn open(bundle: &Path) -> Result<Self> {
        let binary = bundle_binary_path(bundle);
        let library =
            unsafe { libloading::Library::new(&binary) }.map_err(|e| Error::LoadFailed {
                path: bundle.to_path_buf(),
                reason: format!("dlopen: {e}"),
            })?;
        #[cfg(target_os = "linux")]
        let (library, raw_handle) = linux_library_handle(library);
        let mut module = Self {
            library,
            exit: None,
        };
        #[cfg(target_os = "linux")]
        module.enter_linux(bundle, raw_handle)?;
        #[cfg(target_os = "windows")]
        module.enter_windows(bundle)?;
        Ok(module)
    }

    #[cfg(target_os = "linux")]
    fn enter_linux(&mut self, bundle: &Path, raw_handle: *mut std::ffi::c_void) -> Result<()> {
        let entry = unsafe { self.library.get::<LinuxEntry>(MODULE_ENTRY_SYMBOL) }
            .ok()
            .map(|symbol| *symbol);
        let exit = unsafe {
            self.library
                .get::<unsafe extern "system" fn() -> bool>(MODULE_EXIT_SYMBOL)
        }
        .ok()
        .map(|symbol| *symbol);
        self.enter_linux_exports(bundle, raw_handle, entry, exit)
    }

    #[cfg(target_os = "linux")]
    fn enter_linux_exports(
        &mut self,
        bundle: &Path,
        raw_handle: *mut std::ffi::c_void,
        entry: Option<LinuxEntry>,
        exit: Option<unsafe extern "system" fn() -> bool>,
    ) -> Result<()> {
        let entry = entry.ok_or_else(|| Error::LoadFailed {
            path: bundle.to_path_buf(),
            reason: "missing mandatory ModuleEntry".into(),
        })?;
        let exit = exit.ok_or_else(|| Error::LoadFailed {
            path: bundle.to_path_buf(),
            reason: "missing mandatory ModuleExit".into(),
        })?;
        // Keep the exit callback armed even if entry rejects after partial setup.
        self.exit = Some(exit);
        if !unsafe { entry(raw_handle) } {
            return Err(Error::LoadFailed {
                path: bundle.to_path_buf(),
                reason: "ModuleEntry rejected the module".into(),
            });
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn enter_windows(&mut self, bundle: &Path) -> Result<()> {
        let entry = unsafe {
            self.library
                .get::<unsafe extern "system" fn() -> bool>(DLL_ENTRY_SYMBOL)
        }
        .ok()
        .map(|symbol| *symbol);
        self.exit = unsafe {
            self.library
                .get::<unsafe extern "system" fn() -> bool>(DLL_EXIT_SYMBOL)
        }
        .ok()
        .map(|symbol| *symbol);
        if entry.is_some_and(|entry| !unsafe { entry() }) {
            return Err(Error::LoadFailed {
                path: bundle.to_path_buf(),
                reason: "InitDll rejected the module".into(),
            });
        }
        Ok(())
    }

    fn factory(&self) -> Result<ComPtr<IPluginFactory>> {
        let get_factory: libloading::Symbol<
            '_,
            unsafe extern "system" fn() -> *mut IPluginFactory,
        > = unsafe { self.library.get(GET_FACTORY_SYMBOL) }.map_err(|e| Error::LoadFailed {
            path: PathBuf::new(),
            reason: format!("missing GetPluginFactory: {e}"),
        })?;
        let ptr = unsafe { get_factory() };
        let factory = unsafe { ComPtr::<IPluginFactory>::from_raw(ptr) }.ok_or_else(|| {
            Error::LoadFailed {
                path: PathBuf::new(),
                reason: "GetPluginFactory returned NULL".into(),
            }
        })?;
        Ok(factory)
    }
}

#[cfg(target_os = "linux")]
type LinuxEntry = unsafe extern "system" fn(*mut std::ffi::c_void) -> bool;

#[cfg(target_os = "linux")]
fn linux_library_handle(
    library: libloading::Library,
) -> (libloading::Library, *mut std::ffi::c_void) {
    let native: libloading::os::unix::Library = library.into();
    let raw_handle = native.into_raw();
    // SAFETY: `raw_handle` came from this exact owned Library; immediately
    // reconstructing it transfers the single dlclose responsibility back.
    let library = unsafe { libloading::os::unix::Library::from_raw(raw_handle) };
    (library.into(), raw_handle)
}

#[cfg(not(target_os = "macos"))]
impl Drop for LoadedModule {
    fn drop(&mut self) {
        if let Some(exit) = self.exit {
            // SAFETY: the callback came from `self.library`, which remains loaded
            // until this Drop body returns and its field is dropped afterward.
            unsafe { exit() };
        }
    }
}

#[cfg(target_os = "macos")]
impl LoadedModule {
    unsafe fn open(bundle: &Path) -> Result<Self> {
        let mac_bundle = mac::MacBundle::open(bundle)?;

        // VST3 macOS spec: bundleEntry takes the CFBundleRef the
        // host loaded the plugin from. Surge XT Effects (and any
        // bundle that touches CFPlugin/AU registration in init)
        // dereferences that argument; passing nothing crashes
        // inside CFRetain on a register that happened to be
        // non-zero. Reference impl: Steinberg's `module_mac.mm`.
        let entered = unsafe {
            match mac_bundle.function_ptr(BUNDLE_ENTRY_SYMBOL) {
                Some(ptr) => {
                    let entry: unsafe extern "C" fn(*mut std::ffi::c_void) -> bool =
                        std::mem::transmute(ptr);
                    entry(mac_bundle.raw())
                }
                None => false,
            }
        };

        Ok(Self {
            bundle: mac_bundle,
            entered,
        })
    }

    fn factory(&self) -> Result<ComPtr<IPluginFactory>> {
        let raw = unsafe {
            self.bundle
                .function_ptr(GET_FACTORY_SYMBOL)
                .ok_or_else(|| Error::LoadFailed {
                    path: self.bundle.path().to_path_buf(),
                    reason: "missing GetPluginFactory".into(),
                })?
        };
        let get_factory: unsafe extern "C" fn() -> *mut IPluginFactory =
            unsafe { std::mem::transmute(raw) };
        let ptr = unsafe { get_factory() };
        let factory = unsafe { ComPtr::<IPluginFactory>::from_raw(ptr) }.ok_or_else(|| {
            Error::LoadFailed {
                path: self.bundle.path().to_path_buf(),
                reason: "GetPluginFactory returned NULL".into(),
            }
        })?;
        Ok(factory)
    }
}

#[cfg(target_os = "macos")]
impl Drop for LoadedModule {
    fn drop(&mut self) {
        // bundleExit is the symmetric partner to bundleEntry and
        // takes no arguments per the VST3 macOS spec — only
        // bundleEntry sees the CFBundleRef. Most plugins are
        // no-ops; some unregister CFPlugin factories here.
        if self.entered
            && let Some(ptr) = unsafe { self.bundle.function_ptr(BUNDLE_EXIT_SYMBOL) }
        {
            let exit: unsafe extern "C" fn() -> bool = unsafe { std::mem::transmute(ptr) };
            unsafe {
                exit();
            }
        }
    }
}

/// One loaded VST3 plugin instance.
///
/// Holds three COM smart pointers — component, audio processor,
/// edit controller — plus the dlopen handle that keeps the
/// underlying dylib mapped. When `Drop`s, the COM pointers
/// release their objects which triggers `terminate()` and
/// component disposal.
pub struct Vst3Plugin {
    info: PluginInfo,
    layouts: Vec<BusLayout>,
    native_layouts: Vec<bus_layout::NativeLayout>,
    active_layout: Option<bus_layout::NativeLayout>,
    /// Cached `IAudioProcessor::getLatencySamples` value from setup/activation.
    latency_frames: Option<u32>,
    activation_setup: Option<ProcessSetup>,

    // Rust releases fields in declaration order. Release child interfaces before
    // their owning controller/component; some vendors track them independently.
    controller_cp: Option<ComPtr<IConnectionPoint>>,
    controller: ComPtr<IEditController>,
    processor: ComPtr<IAudioProcessor>,
    component_cp: Option<ComPtr<IConnectionPoint>>,
    component: ComPtr<IComponent>,
    /// `true` when controller and component are *different* COM
    /// objects (separate-controller architecture) and we've wired
    /// their connection points.
    separate_controller: bool,

    param_count: usize,
    input_parameters: parameter_delivery::InputParameters,
    processing: bool,

    /// `IPlugView` and host frame retained from attach through close.
    editor: Option<editor_frame::EditorBinding>,
    editor_open: bool,

    // Drop only after every plug-in COM interface so terminate/release can still call back.
    host_services: host_context::HostServices,

    // Rust drops fields in declaration order after our Drop implementation.
    // Keep this LAST: every COM Release must run while its vtable's module is
    // still loaded, including editor and connection-point interfaces.
    _module: LoadedModule,
}

impl Vst3Plugin {
    fn load_from(info: &PluginInfo) -> Result<Self> {
        let module = unsafe { LoadedModule::open(&info.path) }?;
        let factory = module.factory()?;
        let host_services = host_context::HostServices::new();
        let cid = hex_to_tuid(&info.unique_id).ok_or_else(|| Error::LoadFailed {
            path: info.path.clone(),
            reason: format!("could not parse VST3 unique_id {:?}", info.unique_id),
        })?;
        let context = LoadContext {
            info,
            factory: &factory,
            host_services: &host_services,
        };
        let ComponentParts {
            component,
            processor,
            lifecycle: mut component_lifecycle,
        } = load_component(&context, &cid)?;
        let mut controller_cid: TUID = [0; 16];
        let cls_id_status = unsafe { component.getControllerClassId(&raw mut controller_cid) };
        let ControllerParts {
            controller,
            separate,
            lifecycle: mut controller_lifecycle,
        } = load_controller(&context, &component, &controller_cid, cls_id_status)?;

        if !separate {
            host_services
                .attach_handler(&controller)
                .map_err(|error| Error::LoadFailed {
                    path: info.path.clone(),
                    reason: error.to_string(),
                })?;
        }

        // Optional VST3 peer interfaces are used only as a complete connected pair. The guard
        // disconnects them if any later load step fails.
        let peer_connections =
            connect_peers(&component, &controller, separate).map_err(|error| {
                Error::LoadFailed {
                    path: info.path.clone(),
                    reason: error.to_string(),
                }
            })?;

        let param_count_raw = unsafe { controller.getParameterCount() }.max(0);
        #[allow(clippy::cast_sign_loss)]
        let param_count = param_count_raw as usize;
        let input_parameters =
            parameter_delivery::InputParameters::from_controller(&controller, param_count)?;
        let native_layouts = bus_layout::discover(&component, &processor)?;
        let layouts = native_layouts
            .iter()
            .map(|layout| layout.layout.clone())
            .collect();

        let mut info = info.clone();
        // The editor exists if `createView("editor")` returns a
        // non-null view. We probe by creating + releasing it once
        // at load time. (Some plugins are slow to create the view;
        // a heavier-weight host would defer this to first open.)
        info.has_editor = unsafe { create_editor_view(&controller) }.is_some();

        let (component_cp, controller_cp) = peer_connections.into_parts();
        component_lifecycle.disarm();
        if let Some(guard) = controller_lifecycle.as_mut() {
            guard.disarm();
        }
        Ok(Self {
            info,
            layouts,
            native_layouts,
            active_layout: None,
            latency_frames: None,
            activation_setup: None,
            _module: module,
            component,
            processor,
            controller,
            separate_controller: separate,
            component_cp,
            controller_cp,
            host_services,
            param_count,
            input_parameters,
            processing: false,
            editor: None,
            editor_open: false,
        })
    }

    fn restart_for_latency_change(&mut self) -> Result<()> {
        self.latency_frames = None;
        let Some(layout) = self.active_layout.clone() else {
            return Ok(());
        };
        let mut setup = self
            .activation_setup
            .ok_or_else(|| Error::Other("VST3 active instance lost its processing setup".into()))?;
        let revision = self.host_services.latency_revision();
        let mut state = activation::RestartState {
            setup: &mut setup,
            processing: &mut self.processing,
            latency_frames: &mut self.latency_frames,
        };
        let result = activation::restart(&self.component, &self.processor, &layout, &mut state);
        result?;
        self.active_layout = Some(layout);
        self.activation_setup = Some(setup);
        self.publish_latency_revision_if_current(revision);
        Ok(())
    }

    fn publish_latency_revision_if_current(&mut self, revision: u64) -> bool {
        if !latency_revision_is_current(
            revision,
            self.host_services.latency_revision(),
            self.has_pending_parameter_changes(),
        ) {
            self.latency_frames = None;
            return false;
        }
        self.host_services.publish_latency_revision(revision);
        if !latency_revision_is_current(
            revision,
            self.host_services.latency_revision(),
            self.has_pending_parameter_changes(),
        ) {
            self.latency_frames = None;
            return false;
        }
        true
    }

    fn refresh_latency_snapshot(&mut self) {
        if self.host_services.latency_snapshot_valid() {
            return;
        }
        self.latency_frames = None;
        if !latency_refresh_allowed(
            self.active_layout.is_some() && self.processing && self.activation_setup.is_some(),
            self.editor_open,
            self.has_pending_parameter_changes(),
        ) {
            return;
        }

        let revision = self.host_services.latency_revision();
        let processor = &self.processor;
        let host_services = &self.host_services;
        let input_parameters = &self.input_parameters;
        let mut snapshot = None;
        let published = refresh_latency_cache(
            &mut snapshot,
            revision,
            || {
                // SAFETY: this cache refresh runs only on the fixed creator thread after
                // setupProcessing and while the worker serializes access to the VST3 object.
                // Native latency getters are never called from process().
                unsafe { processor.getLatencySamples() }
            },
            |captured_revision| {
                let pending =
                    input_parameters.has_pending_changes() || host_services.has_pending_edits();
                if !latency_revision_is_current(
                    captured_revision,
                    host_services.latency_revision(),
                    pending,
                ) {
                    return false;
                }
                host_services.publish_latency_revision(captured_revision);
                let pending =
                    input_parameters.has_pending_changes() || host_services.has_pending_edits();
                latency_revision_is_current(
                    captured_revision,
                    host_services.latency_revision(),
                    pending,
                )
            },
        );
        if published {
            self.latency_frames = snapshot;
        }
    }

    fn service_requests(&mut self, flags: u32) -> Result<MainThreadUpdate> {
        let requests = host_context::parse_restart_flags(flags)?;
        let services = &self.host_services;
        let parameters = &mut self.input_parameters;
        parameters.service_output(&self.controller)?;
        services.service_edits(|id, value| parameters.queue_id_value(id, value).is_ok())?;

        let mut update = MainThreadUpdate::Unchanged;
        if requests.parameter_titles {
            self.input_parameters.refresh_metadata(&self.controller)?;
            update = MainThreadUpdate::ParametersChanged;
        }
        // kParamValuesChanged invalidates host caches; it is not an outbound edit.
        // parameter_value() reads the controller directly, so there is no value cache
        // to refresh. Re-queuing restored values would block state capture until DSP runs.
        match latency_service_action(requests.latency) {
            LatencyServiceAction::Restart => self.restart_for_latency_change()?,
            LatencyServiceAction::Refresh => self.refresh_latency_snapshot(),
        }
        Ok(update)
    }
}

struct LoadContext<'a> {
    info: &'a PluginInfo,
    factory: &'a ComPtr<IPluginFactory>,
    host_services: &'a host_context::HostServices,
}

struct ComponentParts {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    lifecycle: lifecycle::InitializedPlugin,
}

struct ControllerParts {
    controller: ComPtr<IEditController>,
    separate: bool,
    lifecycle: Option<lifecycle::InitializedPlugin>,
}

fn load_component(context: &LoadContext<'_>, cid: &TUID) -> Result<ComponentParts> {
    let component =
        unsafe { create_instance::<IComponent>(context.factory, cid) }.ok_or_else(|| {
            Error::LoadFailed {
                path: context.info.path.clone(),
                reason: "factory.createInstance(IComponent) returned NULL".into(),
            }
        })?;
    let base = component
        .as_com_ref()
        .cast::<IPluginBase>()
        .expect("IComponent inherits IPluginBase");
    let lifecycle =
        lifecycle::InitializedPlugin::initialize(&base, context.host_services.context_ptr())
            .map_err(|error| load_error(context.info, &error))?;
    let processor = component
        .as_com_ref()
        .cast::<IAudioProcessor>()
        .ok_or_else(|| Error::LoadFailed {
            path: context.info.path.clone(),
            reason: "component does not implement IAudioProcessor".into(),
        })?;
    Ok(ComponentParts {
        component,
        processor,
        lifecycle,
    })
}

fn load_controller(
    context: &LoadContext<'_>,
    component: &ComPtr<IComponent>,
    cid: &TUID,
    status: i32,
) -> Result<ControllerParts> {
    if status == kResultTrue {
        let controller = unsafe { create_instance::<IEditController>(context.factory, cid) }
            .ok_or_else(|| Error::LoadFailed {
                path: context.info.path.clone(),
                reason: "factory.createInstance(IEditController) returned NULL".into(),
            })?;
        let base = controller
            .as_com_ref()
            .cast::<IPluginBase>()
            .expect("IEditController inherits IPluginBase");
        let lifecycle = context
            .host_services
            .initialize_separate_controller(&base, &controller)
            .map_err(|error| load_error(context.info, &error))?;
        return Ok(ControllerParts {
            controller,
            separate: true,
            lifecycle: Some(lifecycle),
        });
    }
    if status != vst3::Steinberg::kResultFalse {
        return Err(Error::LoadFailed {
            path: context.info.path.clone(),
            reason: "IComponent::getControllerClassId failed".into(),
        });
    }
    let controller = component
        .as_com_ref()
        .cast::<IEditController>()
        .ok_or_else(|| Error::LoadFailed {
            path: context.info.path.clone(),
            reason: "component reported no separate controller but lacks IEditController".into(),
        })?;
    Ok(ControllerParts {
        controller,
        separate: false,
        lifecycle: None,
    })
}

fn load_error(info: &PluginInfo, error: &Error) -> Error {
    Error::LoadFailed {
        path: info.path.clone(),
        reason: error.to_string(),
    }
}

fn connect_peers(
    component: &ComPtr<IComponent>,
    controller: &ComPtr<IEditController>,
    separate: bool,
) -> Result<PeerConnections> {
    if !separate {
        return Ok(PeerConnections::empty());
    }
    let component_peer = component.as_com_ref().cast::<IConnectionPoint>();
    let controller_peer = controller.as_com_ref().cast::<IConnectionPoint>();
    if let (Some(component), Some(controller)) = (&component_peer, &controller_peer) {
        return connect_pair(component.clone(), controller.clone());
    }
    Ok(PeerConnections::empty())
}

fn connect_pair(
    component: ComPtr<IConnectionPoint>,
    controller: ComPtr<IConnectionPoint>,
) -> Result<PeerConnections> {
    // SAFETY: both retained interfaces remain alive through each synchronous call; this pair is
    // connected and owned on the serialized plugin lifecycle thread.
    if unsafe { component.connect(controller.as_ptr()) } != kResultOk {
        return Err(Error::Other(
            "VST3 component IConnectionPoint::connect failed".into(),
        ));
    }
    // SAFETY: same retained pair and lifecycle thread. Roll back the first successful connection
    // if the reverse connection is rejected; never transfer a half-connected pair to the owner.
    if unsafe { controller.connect(component.as_ptr()) } != kResultOk {
        unsafe { component.disconnect(controller.as_ptr()) };
        return Err(Error::Other(
            "VST3 controller IConnectionPoint::connect failed".into(),
        ));
    }
    Ok(PeerConnections {
        component: Some(component),
        controller: Some(controller),
        connected: true,
    })
}

struct PeerConnections {
    component: Option<ComPtr<IConnectionPoint>>,
    controller: Option<ComPtr<IConnectionPoint>>,
    connected: bool,
}

impl PeerConnections {
    fn empty() -> Self {
        Self {
            component: None,
            controller: None,
            connected: false,
        }
    }

    fn into_parts(
        mut self,
    ) -> (
        Option<ComPtr<IConnectionPoint>>,
        Option<ComPtr<IConnectionPoint>>,
    ) {
        self.connected = false;
        (self.component.take(), self.controller.take())
    }
}

impl Drop for PeerConnections {
    fn drop(&mut self) {
        if self.connected
            && let (Some(component), Some(controller)) = (&self.component, &self.controller)
        {
            // SAFETY: this guard retains both connected interfaces until it disconnects them.
            unsafe { component.disconnect(controller.as_ptr()) };
            // SAFETY: same retained pair; disconnect the reverse peer before releasing either.
            unsafe { controller.disconnect(component.as_ptr()) };
        }
    }
}

/// Call `IEditController::createView("editor")` and wrap the raw
/// pointer. Returns `None` when the plugin has no editor.
unsafe fn create_editor_view(controller: &ComPtr<IEditController>) -> Option<ComPtr<IPlugView>> {
    let raw = unsafe { controller.createView(ViewType::kEditor) };
    if raw.is_null() {
        return None;
    }
    unsafe { ComPtr::<IPlugView>::from_raw(raw) }
}

fn platform_type_for_handle(
    handle: truce_rack_core::editor::WindowHandle,
) -> (vst3::Steinberg::FIDString, *mut std::ffi::c_void) {
    use truce_rack_core::editor::WindowHandle;
    match handle {
        WindowHandle::NSView(p) => (kPlatformTypeNSView, p),
        WindowHandle::HWND(p) => (kPlatformTypeHWND, p),
        WindowHandle::X11(id) => (kPlatformTypeX11EmbedWindowID, id as *mut std::ffi::c_void),
    }
}

/// Run `factory.createInstance` for `I` and wrap the result as a
/// `ComPtr<I>`. The factory call uses the interface's `IID`
/// directly so the plugin returns the correct `*mut c_void`.
unsafe fn create_instance<I>(factory: &ComPtr<IPluginFactory>, cid: &TUID) -> Option<ComPtr<I>>
where
    I: vst3::Interface,
{
    let mut obj: *mut std::ffi::c_void = ptr::null_mut();
    // `com_scrape_types::Guid` and `TUID` share the same
    // 16-byte layout — reinterpret to avoid an extra copy, independent of c_char signedness.
    let iid_bytes: &TUID =
        unsafe { &*(std::ptr::from_ref::<vst3::com_scrape_types::Guid>(&I::IID).cast::<TUID>()) };
    let cid_ptr = cid.as_ptr();
    let iid_ptr = iid_bytes.as_ptr();
    if unsafe { factory.createInstance(cid_ptr, iid_ptr, &raw mut obj) } != kResultOk
        || obj.is_null()
    {
        return None;
    }
    unsafe { ComPtr::<I>::from_raw(obj.cast()) }
}

fn deactivate_before_terminate(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
    layout: Option<&bus_layout::NativeLayout>,
    processing: &mut bool,
) {
    if let Some(layout) = layout {
        let _ = activation::deactivate(component, processor, layout, processing);
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // Detach and release the editor before terminating its controller.
        truce_rack_core::PluginEditor::close(self);
        deactivate_before_terminate(
            &self.component,
            &self.processor,
            self.active_layout.as_ref(),
            &mut self.processing,
        );
        if let (Some(a), Some(b)) = (&self.component_cp, &self.controller_cp) {
            unsafe {
                a.disconnect(b.as_com_ref().as_ptr().cast());
                b.disconnect(a.as_com_ref().as_ptr().cast());
            }
        }
        if self.separate_controller {
            unsafe { self.controller.terminate() };
        }
        unsafe { self.component.terminate() };
    }
}

impl PluginCore for Vst3Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn active_layout(&self) -> Option<&BusLayout> {
        self.active_layout.as_ref().map(|layout| &layout.layout)
    }
    fn latency_frames(&self) -> Option<u32> {
        self.host_services
            .latency_snapshot_valid()
            .then_some(self.latency_frames)
            .flatten()
    }
    fn supported_layouts(&self) -> &[BusLayout] {
        &self.layouts
    }

    fn parameter_count(&self) -> usize {
        self.param_count
    }

    fn parameter_info(&self, index: usize) -> Result<ParameterInfo> {
        let info = self
            .input_parameters
            .metadata(index)
            .ok_or(Error::InvalidParameter(index))?;
        Ok(vst3_param_info_to_rack(info))
    }

    fn parameter_value(&self, index: usize) -> Result<f64> {
        let info = self
            .input_parameters
            .metadata(index)
            .ok_or(Error::InvalidParameter(index))?;
        Ok(unsafe { self.controller.getParamNormalized(info.id) })
    }

    fn parameter_value_string(&self, index: usize, _value: f64) -> Result<String> {
        // VST3 exposes IEditController::getParamStringByValue —
        // wiring it requires a TChar (UTF-16) buffer round-trip.
        // Tracked as a follow-on to avoid pulling in a wide-char
        // dep here.
        let _ = index;
        Err(Error::Other("vst3 parameter_value_string TODO".into()))
    }

    fn set_parameter(&mut self, index: usize, value: f64) -> Result<()> {
        self.host_services.ensure_creator_thread()?;
        self.input_parameters.service_output(&self.controller)?;
        self.input_parameters.set(&self.controller, index, value)?;
        self.host_services.invalidate_latency();
        Ok(())
    }

    fn has_pending_parameter_changes(&self) -> bool {
        self.input_parameters.has_pending_changes() || self.host_services.has_pending_edits()
    }

    fn preset_count(&self) -> usize {
        // VST3 exposes presets via the `IUnitInfo` interface,
        // wired in a follow-on. Treat as zero for now.
        0
    }
    fn preset_info(&self, index: usize) -> Result<PresetInfo> {
        Err(Error::InvalidParameter(index))
    }
    fn load_preset(&mut self, _preset_number: i32) -> Result<()> {
        Err(Error::Other("vst3 preset loading not yet wired".into()))
    }

    fn save_state(&self) -> Result<Vec<u8>> {
        self.host_services.ensure_creator_thread()?;
        plugin_state::PluginState::capture(
            |stream| {
                // SAFETY: state capture runs on the fixed VST3 creator thread while the
                // component and its controller are alive; the stream is a live host COM object.
                unsafe { self.component.getState(stream) }
            },
            |stream| {
                // SAFETY: the controller is retained for the plugin lifetime and state capture
                // is serialized on its required host thread.
                unsafe { self.controller.getState(stream) }
            },
        )
        .map_err(|error| Error::Other(error.to_string()))
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        self.host_services.ensure_creator_thread()?;
        plugin_state::PluginState::restore(
            bytes,
            || self.host_services.invalidate_latency(),
            |stream| {
                // SAFETY: restore validates the full container before this callback; the
                // component is retained and this method runs on its VST3 host thread.
                unsafe { self.component.setState(stream) }
            },
            |stream| {
                // SAFETY: this receives the same validated component section after component
                // restore, as required by the VST3 persistence call order.
                unsafe { self.controller.setComponentState(stream) }
            },
            |stream| {
                // SAFETY: this receives a validated controller section and runs on the fixed
                // creator thread while the controller remains retained.
                unsafe { self.controller.setState(stream) }
            },
        )
        .map_err(|error| Error::Other(error.to_string()))?;
        self.input_parameters.discard_output();
        Ok(())
    }

    fn activate(
        &mut self,
        layout: BusLayout,
        sample_rate: f64,
        max_block_size: usize,
    ) -> Result<()> {
        self.host_services.ensure_creator_thread()?;
        if self.is_active() {
            return Err(Error::Other(
                "VST3 must be deactivated before changing layout".into(),
            ));
        }
        let native_layout = bus_layout::select_layout(&self.native_layouts, &layout)?;
        let prepared_output = self.input_parameters.output_replacement(max_block_size)?;
        let maximum_block = i32::try_from(max_block_size)
            .map_err(|_| Error::Other("invalid VST3 maximum block size".into()))?;
        self.latency_frames = None;
        let latency_revision = self.host_services.latency_revision();
        let mut setup = ProcessSetup {
            processMode: i32::from_ne_bytes(ProcessModes_::kRealtime.to_ne_bytes()),
            symbolicSampleSize: i32::from_ne_bytes(SymbolicSampleSizes_::kSample32.to_ne_bytes()),
            maxSamplesPerBlock: maximum_block,
            sampleRate: sample_rate,
        };
        activation::activate(
            &self.component,
            &self.processor,
            &native_layout,
            &mut setup,
            &mut self.latency_frames,
        )?;
        self.input_parameters.install_output(prepared_output);
        self.processing = true;
        self.active_layout = Some(native_layout);
        self.activation_setup = Some(setup);
        self.publish_latency_revision_if_current(latency_revision);
        Ok(())
    }

    fn deactivate(&mut self) {
        if let Some(layout) = self.active_layout.clone()
            && activation::deactivate(
                &self.component,
                &self.processor,
                &layout,
                &mut self.processing,
            )
            .is_ok()
        {
            self.active_layout = None;
            self.activation_setup = None;
        }
        self.host_services.invalidate_latency();
    }
    fn is_active(&self) -> bool {
        self.active_layout.is_some()
    }

    fn service_main_thread(&mut self) -> Result<MainThreadUpdate> {
        if let Err(error) = self.host_services.ensure_creator_thread() {
            self.latency_frames = None;
            self.host_services.fail_closed();
            return Err(error);
        }
        let flags = self.host_services.take_restart_flags();
        if self.host_services.callbacks_failed() {
            self.latency_frames = None;
            return Err(Error::Other("VST3 host callback previously failed".into()));
        }
        let result = self.service_requests(flags);
        if result.is_err() {
            self.latency_frames = None;
            self.host_services.fail_closed();
        }
        result
    }

    fn editor(&mut self) -> Option<&mut dyn truce_rack_core::editor::PluginEditor> {
        if !self.info.has_editor {
            return None;
        }
        Some(self)
    }
}

impl truce_rack_core::editor::PluginEditor for Vst3Plugin {
    fn open(
        &mut self,
        parent: truce_rack_core::editor::WindowHandle,
        _scale: f64,
    ) -> truce_rack_core::error::Result<()> {
        if self.editor_open {
            return Ok(());
        }
        // View creation/attachment may expose mutable native UI state, including on partial
        // failure; do not retain a pre-editor latency snapshot.
        self.latency_frames = None;
        self.host_services.invalidate_latency();
        let view = unsafe { create_editor_view(&self.controller) }
            .ok_or_else(|| Error::Other("IEditController::createView returned NULL".into()))?;
        let (type_str, parent_ptr) = platform_type_for_handle(parent);
        if unsafe { view.isPlatformTypeSupported(type_str) } != kResultOk {
            return Err(Error::Other(
                "IPlugView::isPlatformTypeSupported returned false".into(),
            ));
        }
        self.editor = Some(
            editor_frame::EditorBinding::attach(view, parent_ptr, type_str)
                .map_err(|message| Error::Other(message.into()))?,
        );
        self.editor_open = true;
        Ok(())
    }

    fn close(&mut self) {
        self.editor.take();
        self.editor_open = false;
        // The editor may have mutated latency through native UI state.
        self.latency_frames = None;
        self.host_services.invalidate_latency();
    }

    fn is_open(&self) -> bool {
        self.editor_open
    }

    fn size(&self) -> Option<(u32, u32)> {
        let view = self.editor.as_ref()?.view();
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { view.getSize(&raw mut rect) } != kResultOk {
            return None;
        }
        let w = u32::try_from(rect.right - rect.left).ok()?;
        let h = u32::try_from(rect.bottom - rect.top).ok()?;
        Some((w, h))
    }

    fn is_resizable(&self) -> bool {
        let Some(view) = self.editor.as_ref().map(editor_frame::EditorBinding::view) else {
            return false;
        };
        unsafe { view.canResize() == kResultOk }
    }

    fn set_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let view = self.editor.as_ref()?.view();
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: i32::try_from(width).ok()?,
            bottom: i32::try_from(height).ok()?,
        };
        // Plugins may snap to a constraint via checkSizeConstraint.
        let _ = unsafe { view.checkSizeConstraint(&raw mut rect) };
        if unsafe { view.onSize(&raw mut rect) } != kResultOk {
            return None;
        }
        let w = u32::try_from(rect.right - rect.left).ok()?;
        let h = u32::try_from(rect.bottom - rect.top).ok()?;
        Some((w, h))
    }

    fn show(&mut self) {
        // VST3 attaches once and stays visible — no show/hide
        // separate from attached/removed.
    }

    fn hide(&mut self) {
        // Same as show — VST3 has no distinct hide.
    }

    fn on_idle(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(editor) = &self.editor {
            editor.on_idle();
        }
    }
}

fn empty_parameter_info() -> Vst3ParameterInfo {
    Vst3ParameterInfo {
        id: 0,
        title: [0; 128],
        shortTitle: [0; 128],
        units: [0; 128],
        stepCount: 0,
        defaultNormalizedValue: 0.0,
        unitId: 0,
        flags: 0,
    }
}

fn vst3_param_info_to_rack(info: &Vst3ParameterInfo) -> ParameterInfo {
    let name = string128_to_string(&info.title);
    let short_name = string128_to_string(&info.shortTitle);
    let unit = string128_to_string(&info.units);
    let mut flags = truce_rack_core::info::ParameterFlags::empty();
    if info.flags & ParameterFlags_::kIsBypass != 0 {
        flags |= truce_rack_core::info::ParameterFlags::BYPASS;
    }
    if info.flags & ParameterFlags_::kCanAutomate != 0 {
        flags |= truce_rack_core::info::ParameterFlags::AUTOMATABLE;
    }
    if info.flags & ParameterFlags_::kIsHidden != 0 {
        flags |= truce_rack_core::info::ParameterFlags::HIDDEN;
    }
    if info.flags & ParameterFlags_::kIsReadOnly != 0 {
        flags |= truce_rack_core::info::ParameterFlags::READ_ONLY;
    }
    if info.flags & ParameterFlags_::kIsList != 0 {
        flags |= truce_rack_core::info::ParameterFlags::ENUMERATED;
    }
    ParameterInfo {
        id: info.id,
        name,
        short_name,
        unit,
        min: 0.0,
        max: 1.0,
        default: info.defaultNormalizedValue,
        step_count: u32::try_from(info.stepCount).unwrap_or(0),
        flags,
    }
}

/// VST3 `String128` is `[char16; 128]` (UTF-16). Walk the slice
/// until the first NUL and decode to UTF-8.
fn string128_to_string(buf: &[u16; 128]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Translate a host [`TransportInfo`] snapshot into Steinberg's
/// `Vst::ProcessContext`. Only the fields the host reported get
/// their `k…Valid` flag set; everything else stays zero so the
/// plugin treats it as absent.
// `StatesAndFlags` is `c_int` or `c_uint` depending on platform, so
// the `as u32` casts are only redundant on some targets.
#[allow(clippy::unnecessary_cast)]
fn build_vst3_context(t: &TransportInfo, sample_rate: f64) -> Vst3ProcessContext {
    // SAFETY: ProcessContext is a repr(C) aggregate of integers,
    // floats, and small POD sub-structs (Chord, FrameRate). An
    // all-zero value is the valid "no flags set" state; we then
    // fill in the fields the host actually reported.
    let mut ctx: Vst3ProcessContext = unsafe { std::mem::zeroed() };
    ctx.sampleRate = sample_rate;

    let mut state: u32 = 0;
    if let Some(tempo) = t.tempo_bpm {
        ctx.tempo = tempo;
        state |= StatesAndFlags_::kTempoValid as u32;
    }
    if let Some((num, den)) = t.time_signature {
        ctx.timeSigNumerator = i32::try_from(num).unwrap_or(4);
        ctx.timeSigDenominator = i32::try_from(den).unwrap_or(4);
        state |= StatesAndFlags_::kTimeSigValid as u32;
    }
    if let Some(beats) = t.song_position_beats {
        // projectTimeMusic is in quarter notes (TQuarterNotes = f64).
        ctx.projectTimeMusic = beats;
        state |= StatesAndFlags_::kProjectTimeMusicValid as u32;
    }
    if let Some(samples) = t.song_position_samples {
        ctx.projectTimeSamples = samples;
    }
    if let Some(bar) = t.bar_start_beats {
        ctx.barPositionMusic = bar;
        state |= StatesAndFlags_::kBarPositionValid as u32;
    }
    if t.playing {
        state |= StatesAndFlags_::kPlaying as u32;
    }
    if t.recording {
        state |= StatesAndFlags_::kRecording as u32;
    }
    if t.loop_active {
        state |= StatesAndFlags_::kCycleActive as u32;
    }
    ctx.state = state;
    ctx
}

impl Plugin<f32> for Vst3Plugin {
    fn process(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        events: &EventList,
        context: &mut ProcessContext<'_>,
    ) -> Result<ProcessStatus> {
        let layout = self.active_layout.as_ref().ok_or(Error::NotActivated)?;
        let frames = buffer.num_frames();

        // Build an IEventList for the plugin's inputEvents slot.
        // We translate truce-rack MIDI events to VST3 Event structs.
        let mut translated = EventBuffer::default();
        for event in events {
            translated.push_rack(event);
        }
        let input_events_wrapper = ComWrapper::new(EventList3 {
            events: std::cell::RefCell::new(translated.events),
        });
        let input_events_ptr = input_events_wrapper
            .to_com_ptr::<IEventList>()
            .ok_or_else(|| Error::Other("EventList3 missing IEventList IID".into()))?;
        let output_events_wrapper = ComWrapper::new(EventList3::default());
        let output_events_ptr = output_events_wrapper
            .to_com_ptr::<IEventList>()
            .ok_or_else(|| Error::Other("EventList3 missing IEventList IID".into()))?;

        // Build the transport context up front so its backing
        // struct outlives the process call.
        let mut process_context = context
            .transport
            .map(|t| build_vst3_context(&t, context.sample_rate));

        let status = bus_layout::with_buffers(buffer, layout, |inputs, outputs| {
            let mut data = ProcessData {
                processMode: i32::from_ne_bytes(ProcessModes_::kRealtime.to_ne_bytes()),
                symbolicSampleSize: i32::from_ne_bytes(
                    SymbolicSampleSizes_::kSample32.to_ne_bytes(),
                ),
                numSamples: i32::try_from(frames).unwrap_or(i32::MAX),
                numInputs: i32::try_from(inputs.len()).expect("bounded bus count"),
                numOutputs: i32::try_from(outputs.len()).expect("bounded bus count"),
                inputs: if inputs.is_empty() {
                    ptr::null_mut()
                } else {
                    inputs.as_mut_ptr()
                },
                outputs: if outputs.is_empty() {
                    ptr::null_mut()
                } else {
                    outputs.as_mut_ptr()
                },
                inputParameterChanges: ptr::null_mut::<IParameterChanges>(),
                outputParameterChanges: ptr::null_mut::<IParameterChanges>(),
                inputEvents: input_events_ptr.as_ptr(),
                outputEvents: output_events_ptr.as_ptr(),
                processContext: process_context
                    .as_mut()
                    .map_or(ptr::null_mut(), std::ptr::from_mut),
            };

            run_audio_block_with::<Vst3Plugin, i32>(FORMAT, -1, || {
                self.input_parameters.process(&self.processor, &mut data)
            })
        })?;
        if status == kResultOk {
            Ok(ProcessStatus::Continue)
        } else {
            Ok(ProcessStatus::Error)
        }
    }
}

// ---------------------------------------------------------------------------
// EventList3 — in-memory IEventList impl for MIDI in/out.
// ---------------------------------------------------------------------------

/// Translation buffer for truce-rack-core `EventList` → VST3 `Event[]`.
#[derive(Default)]
struct EventBuffer {
    events: Vec<Event>,
}

impl EventBuffer {
    fn push_rack(&mut self, event: &truce_rack_core::events::Event) {
        use truce_rack_core::events::{EventBody, MidiData};
        let offset = i32::try_from(event.sample_offset).unwrap_or(i32::MAX);
        let EventBody::Midi(body) = event.body else {
            return;
        };
        let header = |type_: u16| Event {
            busIndex: 0,
            sampleOffset: offset,
            ppqPosition: 0.0,
            flags: 0,
            r#type: type_,
            __field0: Event__type0 {
                noteOn: NoteOnEvent {
                    channel: 0,
                    pitch: 0,
                    tuning: 0.0,
                    velocity: 0.0,
                    length: 0,
                    noteId: -1,
                },
            },
        };
        match body {
            MidiData::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let mut ev = header(u16::try_from(EventTypes_::kNoteOnEvent).unwrap_or(0));
                ev.__field0 = Event__type0 {
                    noteOn: NoteOnEvent {
                        channel: i16::from(channel),
                        pitch: i16::from(note),
                        tuning: 0.0,
                        velocity: f32::from(velocity) / 127.0,
                        length: 0,
                        noteId: -1,
                    },
                };
                self.events.push(ev);
            }
            MidiData::NoteOff {
                channel,
                note,
                velocity,
            } => {
                let mut ev = header(u16::try_from(EventTypes_::kNoteOffEvent).unwrap_or(0));
                ev.__field0 = Event__type0 {
                    noteOff: NoteOffEvent {
                        channel: i16::from(channel),
                        pitch: i16::from(note),
                        velocity: f32::from(velocity) / 127.0,
                        noteId: -1,
                        tuning: 0.0,
                    },
                };
                self.events.push(ev);
            }
            MidiData::PolyAftertouch {
                channel,
                note,
                pressure,
            } => {
                let mut ev = header(u16::try_from(EventTypes_::kPolyPressureEvent).unwrap_or(0));
                ev.__field0 = Event__type0 {
                    polyPressure: PolyPressureEvent {
                        channel: i16::from(channel),
                        pitch: i16::from(note),
                        pressure: f32::from(pressure) / 127.0,
                        noteId: -1,
                    },
                };
                self.events.push(ev);
            }
            // VST3 routes CC / ProgramChange / ChannelAftertouch /
            // PitchBend / Sysex through IParameterChanges or
            // IMidiMapping rather than IEventList. Wiring those is
            // a follow-on; for hosting test purposes notes are the
            // primary need.
            _ => {}
        }
    }
}

#[derive(Default)]
struct EventList3 {
    events: std::cell::RefCell<Vec<Event>>,
}

impl Class for EventList3 {
    type Interfaces = (IEventList,);
}

#[allow(clippy::cast_sign_loss)]
impl IEventListTrait for EventList3 {
    unsafe fn getEventCount(&self) -> i32 {
        i32::try_from(self.events.borrow().len()).unwrap_or(i32::MAX)
    }

    unsafe fn getEvent(&self, index: i32, out: *mut Event) -> i32 {
        if out.is_null() || index < 0 {
            return -1;
        }
        let events = self.events.borrow();
        let Some(event) = events.get(index as usize) else {
            return -1;
        };
        unsafe { *out = *event };
        kResultOk
    }

    unsafe fn addEvent(&self, event: *mut Event) -> i32 {
        if event.is_null() {
            return -1;
        }
        self.events.borrow_mut().push(unsafe { *event });
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct ScannerFixture {
        root: PathBuf,
    }

    impl ScannerFixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock follows Unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "truce-rack-vst3-scan-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir(&root).expect("create isolated scanner fixture");
            Self { root }
        }

        fn child(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }

    impl Drop for ScannerFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    struct ConnectionProbe {
        connect_result: i32,
        connect_event: &'static str,
        disconnect_event: &'static str,
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Class for ConnectionProbe {
        type Interfaces = (IConnectionPoint,);
    }

    impl IConnectionPointTrait for ConnectionProbe {
        unsafe fn connect(&self, _other: *mut IConnectionPoint) -> i32 {
            self.events
                .lock()
                .expect("connection events")
                .push(self.connect_event);
            self.connect_result
        }

        unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> i32 {
            self.events
                .lock()
                .expect("connection events")
                .push(self.disconnect_event);
            kResultOk
        }

        unsafe fn notify(&self, _message: *mut vst3::Steinberg::Vst::IMessage) -> i32 {
            kResultOk
        }
    }

    fn connection_pair(
        component_result: i32,
        controller_result: i32,
        events: Arc<Mutex<Vec<&'static str>>>,
    ) -> (ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>) {
        let component = ComWrapper::new(ConnectionProbe {
            connect_result: component_result,
            connect_event: "component.connect",
            disconnect_event: "component.disconnect",
            events: Arc::clone(&events),
        });
        let controller = ComWrapper::new(ConnectionProbe {
            connect_result: controller_result,
            connect_event: "controller.connect",
            disconnect_event: "controller.disconnect",
            events,
        });
        let component_ptr = component
            .to_com_ptr::<IConnectionPoint>()
            .expect("component connection point");
        let controller_ptr = controller
            .to_com_ptr::<IConnectionPoint>()
            .expect("controller connection point");
        (component_ptr, controller_ptr)
    }

    #[test]
    fn tuid_hex_roundtrip() {
        let cid: TUID = [
            0x01u8, 0x23, 0x45, 0x67, 0xf0, 0xf0, 0xf0, 0xf0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
        .map(|byte| std::ffi::c_char::from_ne_bytes([byte]));
        let hex = tuid_to_hex(&cid);
        let parsed = hex_to_tuid(&hex).expect("parse");
        assert_eq!(parsed, cid);
    }

    #[test]
    fn tuid_hex_keeps_two_digits_for_high_bytes() {
        let cid: TUID = [
            0x00u8, 0x01, 0x0f, 0x10, 0x7f, 0x80, 0x81, 0xfe, 0xff, 0x02, 0x03, 0x04, 0x05, 0x06,
            0x07, 0x08,
        ]
        .map(|byte| std::ffi::c_char::from_ne_bytes([byte]));

        let hex = tuid_to_hex(&cid);

        assert_eq!(hex, "00010f107f8081feff02030405060708");
        assert_eq!(hex.len(), 32);
        assert_eq!(hex_to_tuid(&hex), Some(cid));
    }

    #[test]
    fn tuid_hex_rejects_non_ascii_without_slicing_utf8() {
        let malformed = format!("aé{}", "0".repeat(29));
        assert_eq!(malformed.len(), 32);
        assert_eq!(hex_to_tuid(&malformed), None);
    }

    #[test]
    fn char8_skips_trailing_nul() {
        let mut arr = [0 as vst3::Steinberg::char8; 16];
        for (i, &b) in b"Hello".iter().enumerate() {
            arr[i] = vst3::Steinberg::char8::from_ne_bytes([b]);
        }
        assert_eq!(char8_array_to_string(&arr), "Hello");
    }

    #[test]
    fn char8_preserves_non_ascii_utf8_on_both_c_abis() {
        let bytes: Vec<vst3::Steinberg::char8> = "마이크\0ignored"
            .as_bytes()
            .iter()
            .map(|&byte| vst3::Steinberg::char8::from_ne_bytes([byte]))
            .collect();
        assert_eq!(char8_array_to_string(&bytes), "마이크");
    }

    #[test]
    fn connection_guard_rolls_back_failures_and_transfers_complete_pair() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (component_ptr, controller_ptr) = connection_pair(
            vst3::Steinberg::kInvalidArgument,
            kResultOk,
            Arc::clone(&events),
        );
        assert!(connect_pair(component_ptr, controller_ptr).is_err());
        assert_eq!(
            *events.lock().expect("connection events"),
            ["component.connect"]
        );

        events.lock().expect("connection events").clear();
        let (component_ptr, controller_ptr) = connection_pair(
            kResultOk,
            vst3::Steinberg::kInvalidArgument,
            Arc::clone(&events),
        );
        assert!(connect_pair(component_ptr, controller_ptr).is_err());
        assert_eq!(
            *events.lock().expect("connection events"),
            [
                "component.connect",
                "controller.connect",
                "component.disconnect"
            ]
        );

        events.lock().expect("connection events").clear();
        let (component_ptr, controller_ptr) =
            connection_pair(kResultOk, kResultOk, Arc::clone(&events));
        let guard = connect_pair(component_ptr, controller_ptr).expect("complete pair");
        drop(guard);
        assert_eq!(
            *events.lock().expect("connection events"),
            [
                "component.connect",
                "controller.connect",
                "component.disconnect",
                "controller.disconnect"
            ]
        );

        events.lock().expect("connection events").clear();
        let (component_ptr, controller_ptr) =
            connection_pair(kResultOk, kResultOk, Arc::clone(&events));
        let guard = connect_pair(component_ptr, controller_ptr).expect("complete pair");
        let (component, controller) = guard.into_parts();
        assert!(component.is_some() && controller.is_some());
        drop(component);
        drop(controller);
        assert_eq!(
            *events.lock().expect("connection events"),
            ["component.connect", "controller.connect"]
        );
    }

    #[test]
    fn vst3_scanner_distinguishes_optional_default_dirs_from_explicit_paths() {
        let fixture = ScannerFixture::new();
        let optional_missing = fixture.child("optional-default");
        let mut plugins = Vec::new();
        scan_optional_default_dir(&optional_missing, &mut plugins)
            .expect("a missing conventional directory is optional");
        assert!(plugins.is_empty());

        let explicit_missing = fixture.child("explicit-missing");
        let error = Vst3Scanner::new()
            .scan_path(&explicit_missing)
            .expect_err("an explicit missing directory is an error");
        assert!(
            error
                .to_string()
                .contains(&explicit_missing.display().to_string())
        );

        let not_directory = fixture.child("not-a-directory");
        std::fs::write(&not_directory, b"file").expect("create non-directory scan target");
        let error = Vst3Scanner::new()
            .scan_path(&not_directory)
            .expect_err("an unreadable explicit scan target is an error");
        assert!(
            error
                .to_string()
                .contains(&not_directory.display().to_string())
        );

        let empty = fixture.child("empty");
        std::fs::create_dir(&empty).expect("create empty directory");
        assert!(
            Vst3Scanner::new()
                .scan_path(&empty)
                .expect("an existing empty directory scans successfully")
                .is_empty()
        );
    }

    #[test]
    fn vst3_scanner_supports_exact_bundle_paths_and_propagates_corruption() {
        let fixture = ScannerFixture::new();
        let corrupt_bundle = fixture.child("corrupt.vst3");
        std::fs::create_dir(&corrupt_bundle).expect("create empty corrupt bundle");
        let exact_error = Vst3Scanner::new()
            .scan_path(&corrupt_bundle)
            .expect_err("an exact bundle path is opened as a bundle and errors");
        assert!(
            exact_error
                .to_string()
                .contains(&corrupt_bundle.display().to_string())
        );

        let directory = fixture.child("plugins");
        std::fs::create_dir(&directory).expect("create plugin directory");
        let nested_corrupt = directory.join("broken.vst3");
        std::fs::create_dir(&nested_corrupt).expect("create corrupt candidate");
        let directory_error = Vst3Scanner::new()
            .scan_path(&directory)
            .expect_err("a corrupt candidate fails the directory scan");
        assert!(
            directory_error
                .to_string()
                .contains(&nested_corrupt.display().to_string())
        );
    }

    #[test]
    fn windows_arm64_bundle_directory_matches_the_sdk() {
        assert_eq!(windows_arch_dir("aarch64"), "arm64-win");
        assert_eq!(windows_arch_dir("x86_64"), "x86_64-win");
        assert_eq!(windows_arch_dir("x86"), "x86-win");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_module_requires_both_hooks_and_exits_after_entry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static ENTRY_CALLS: AtomicUsize = AtomicUsize::new(0);
        static EXIT_CALLS: AtomicUsize = AtomicUsize::new(0);
        static LAST_HANDLE: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "system" fn accepted(handle: *mut std::ffi::c_void) -> bool {
            LAST_HANDLE.store(handle as usize, Ordering::SeqCst);
            ENTRY_CALLS.fetch_add(1, Ordering::SeqCst);
            true
        }
        unsafe extern "system" fn rejected(handle: *mut std::ffi::c_void) -> bool {
            LAST_HANDLE.store(handle as usize, Ordering::SeqCst);
            ENTRY_CALLS.fetch_add(1, Ordering::SeqCst);
            false
        }
        unsafe extern "system" fn exit() -> bool {
            EXIT_CALLS.fetch_add(1, Ordering::SeqCst);
            true
        }
        fn module() -> LoadedModule {
            LoadedModule {
                library: libloading::os::unix::Library::this().into(),
                exit: None,
            }
        }

        ENTRY_CALLS.store(0, Ordering::SeqCst);
        EXIT_CALLS.store(0, Ordering::SeqCst);
        let bundle = Path::new("test.vst3");
        let handle = 0x1234usize as *mut std::ffi::c_void;
        let mut missing = module();
        assert!(
            missing
                .enter_linux_exports(bundle, handle, None, Some(exit))
                .expect_err("ModuleEntry is mandatory")
                .to_string()
                .contains("ModuleEntry")
        );
        assert!(
            missing
                .enter_linux_exports(bundle, handle, Some(accepted), None)
                .expect_err("ModuleExit is mandatory")
                .to_string()
                .contains("ModuleExit")
        );
        drop(missing);
        assert_eq!(ENTRY_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 0);

        let mut denied = module();
        assert!(
            denied
                .enter_linux_exports(bundle, handle, Some(rejected), Some(exit))
                .expect_err("rejected entry fails loading")
                .to_string()
                .contains("ModuleEntry rejected")
        );
        drop(denied);
        assert_eq!(ENTRY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(LAST_HANDLE.load(Ordering::SeqCst), handle as usize);

        let mut loaded = module();
        loaded
            .enter_linux_exports(bundle, handle, Some(accepted), Some(exit))
            .expect("accepted entry loads");
        drop(loaded);
        assert_eq!(ENTRY_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ordinary_parameter_invalidation_refreshes_without_restart() {
        let mut snapshot = Some(64);
        let calls = std::cell::Cell::new(0);
        let published_revision = std::cell::Cell::new(None);
        assert_eq!(latency_service_action(false), LatencyServiceAction::Refresh);

        let published = refresh_latency_cache(
            &mut snapshot,
            2,
            || {
                calls.set(calls.get() + 1);
                96
            },
            |revision| {
                published_revision.set(Some(revision));
                true
            },
        );

        assert!(published);
        assert_eq!(snapshot, Some(96));
        assert_eq!(published_revision.get(), Some(2));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn native_state_invalidation_refreshes_without_restart() {
        let mut snapshot = None;
        let published_revision = std::cell::Cell::new(None);

        let published = refresh_latency_cache(
            &mut snapshot,
            7,
            || 128,
            |revision| {
                published_revision.set(Some(revision));
                true
            },
        );

        assert!(published);
        assert_eq!(snapshot, Some(128));
        assert_eq!(published_revision.get(), Some(7));
    }

    #[test]
    fn notification_during_latency_query_keeps_snapshot_unknown() {
        let mut snapshot = Some(32);
        let revision = std::cell::Cell::new(10);
        let publish_called = std::cell::Cell::new(false);

        let published = refresh_latency_cache(
            &mut snapshot,
            revision.get(),
            || {
                revision.set(11);
                64
            },
            |captured| {
                let current = revision.get();
                if !latency_revision_is_current(captured, current, false) {
                    return false;
                }
                publish_called.set(true);
                true
            },
        );

        assert!(!published);
        assert_eq!(snapshot, None);
        assert!(!publish_called.get());
    }

    #[test]
    fn pending_parameter_delivery_blocks_refresh_and_latency_flags_restart() {
        assert!(!latency_refresh_allowed(true, false, true));
        assert!(latency_refresh_allowed(true, false, false));
        assert!(!latency_refresh_allowed(false, false, false));
        assert!(!latency_refresh_allowed(true, true, false));
        assert_eq!(latency_service_action(true), LatencyServiceAction::Restart);
    }
}
