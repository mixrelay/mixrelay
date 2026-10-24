//! Audio Unit v2 host for the truce-rack framework. Apple-only.
//!
//! Built on `objc2-audio-toolbox` — the modern Rust bindings
//! that replace the C++ `AudioToolbox` scanner the legacy rack 0.4
//! relied on. No cmake, no bridging-header step.
//!
//! # Status
//!
//! - **Scan** via `AudioComponentFindNext` across the AU v2 type
//!   families (Effect, `MusicDevice`, Generator, Mixer, `MusicEffect`,
//!   `MIDIProcessor`). Each match yields one [`PluginInfo`] with the
//!   AU's four-CC packed as a `"type:sub:mfr"` `unique_id`.
//! - **Load** via `AudioComponentInstanceNew`, or asynchronous
//!   `AudioComponentInstantiate` when component flags require it.
//! - **Process** via `AudioUnitRender` into a per-block
//!   `AudioBufferList` whose buffers point directly at the host's
//!   output planes (see [`AuPlugin::process`]).
//! - **MIDI** via `MusicDeviceMIDIEvent`. Routes channel-voice
//!   messages with sample-accurate offsets; raw bytes pass through
//!   the same call. Non-MIDI-accepting effects no-op silently.
//! - **Editor** via the AUv2 Cocoa view (`kAudioUnitProperty_CocoaUI`) or
//!   AUv3 view-controller request (`kAudioUnitProperty_RequestViewController`).
//!
//! AU v3 plugins that set `kAudioComponentFlag_RequiresAsyncInstantiation`
//! are routed through `AudioComponentInstantiate`. The scanner surface is
//! still synchronous and pumps the current run loop with a bounded timeout;
//! native AUv3 loading requires qualification in the actual host lifecycle,
//! because Apple warns that waiting on the main thread can deadlock.

#![cfg(target_vendor = "apple")]

mod au3_editor;
mod channel_layouts;
mod latency_signal;

use truce_rack_core::buffer::AudioBuffer;
use truce_rack_core::bus::{Bus, BusKind, BusLayout, ChannelConfig};
use truce_rack_core::error::{Error, Result};
use truce_rack_core::events::EventList;
use truce_rack_core::info::{ParameterInfo, PluginCategory, PluginInfo, PresetInfo};
use truce_rack_core::plugin::{
    MainThreadUpdate, Plugin, PluginCore, ProcessContext, ProcessStatus,
};
use truce_rack_core::scanner::PluginScanner;
use truce_rack_core::transport::TransportInfo;

use objc2_audio_toolbox::{
    AUPreset, AURenderCallbackStruct, AudioComponent, AudioComponentCopyName,
    AudioComponentDescription, AudioComponentFindNext, AudioComponentFlags, AudioComponentInstance,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioComponentInstantiate,
    AudioComponentInstantiationOptions, AudioUnit, AudioUnitAddPropertyListener, AudioUnitElement,
    AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo, AudioUnitInitialize,
    AudioUnitParameterID, AudioUnitParameterInfo, AudioUnitPropertyID,
    AudioUnitRemovePropertyListenerWithUserData, AudioUnitRender, AudioUnitRenderActionFlags,
    AudioUnitScope, AudioUnitSetParameter, AudioUnitSetProperty, AudioUnitUninitialize,
    HostCallbackInfo, MusicDeviceMIDIEvent, kAudioUnitErr_CannotDoInCurrentContext,
    kAudioUnitErr_InvalidProperty, kAudioUnitProperty_AudioChannelLayout,
    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_ElementCount, kAudioUnitProperty_ElementName,
    kAudioUnitProperty_FactoryPresets, kAudioUnitProperty_HostCallbacks,
    kAudioUnitProperty_Latency, kAudioUnitProperty_MaximumFramesPerSlice,
    kAudioUnitProperty_ParameterInfo, kAudioUnitProperty_ParameterList,
    kAudioUnitProperty_PresentPreset, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
    kAudioUnitScope_Output, kAudioUnitType_Effect, kAudioUnitType_Generator,
    kAudioUnitType_MIDIProcessor, kAudioUnitType_Mixer, kAudioUnitType_MusicDevice,
    kAudioUnitType_MusicEffect,
};
use objc2_core_audio_types::{
    AudioBuffer as CAAudioBuffer, AudioBufferList, AudioChannelDescription, AudioChannelLayout,
    AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioChannelLayoutTag_AudioUnit_5_1, kAudioChannelLayoutTag_AudioUnit_7_1,
    kAudioChannelLayoutTag_Mono, kAudioChannelLayoutTag_Stereo, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFString;

use latency_signal::{LatencySignalLease, acquire as acquire_latency_signal};
use std::path::Path;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

/// Format identifier used on returned [`PluginInfo`].
pub const FORMAT: &str = "au";

/// AU type families we surface to hosts. Output / `FormatConverter`
/// are intentionally skipped — they're system plumbing, not
/// effects.
const SCAN_TYPES: &[u32] = &[
    kAudioUnitType_Effect,
    kAudioUnitType_MusicDevice,
    kAudioUnitType_Generator,
    kAudioUnitType_MusicEffect,
    kAudioUnitType_MIDIProcessor,
    kAudioUnitType_Mixer,
];

/// AU v2 scanner.
#[derive(Debug, Default)]
pub struct AuScanner;

impl AuScanner {
    /// Construct a default scanner.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Load an AU for an already selected catalog layout without enumerating alternatives.
    ///
    /// `layout` must retain the native bus order and roles. Native format acceptance and
    /// initialization are still checked by activation, including after state restoration.
    ///
    /// # Errors
    /// Returns an error for a load failure, invalid topology, or a rejected channel declaration.
    pub fn load_with_layout(&self, info: &PluginInfo, layout: &BusLayout) -> Result<AuPlugin> {
        AuPlugin::load_from(info, Some(layout))
    }
}

impl PluginScanner for AuScanner {
    type Plugin = AuPlugin;

    fn scan(&self) -> Result<Vec<PluginInfo>> {
        let mut out = Vec::new();
        for &type_code in SCAN_TYPES {
            unsafe { scan_family(type_code, &mut out) };
        }
        Ok(out)
    }

    fn scan_path(&self, _path: &Path) -> Result<Vec<PluginInfo>> {
        // AU discovery is registry-based, not path-based.
        Err(Error::Other(
            "truce-rack-au path-bounded scan is not meaningful (AU uses a registry)".into(),
        ))
    }

    fn load(&self, info: &PluginInfo) -> Result<Self::Plugin> {
        AuPlugin::load_from(info, None)
    }
}

unsafe fn scan_family(component_type: u32, out: &mut Vec<PluginInfo>) {
    let mut desc = AudioComponentDescription {
        componentType: component_type,
        componentSubType: 0,
        componentManufacturer: 0,
        componentFlags: 0,
        componentFlagsMask: 0,
    };
    let mut component: AudioComponent = ptr::null_mut();
    let category = type_category(component_type);
    let accepts_midi =
        component_type != kAudioUnitType_Effect && component_type != kAudioUnitType_Generator;
    loop {
        let next = unsafe {
            AudioComponentFindNext(component, ptr::NonNull::new_unchecked(&raw mut desc))
        };
        if next.is_null() {
            break;
        }
        component = next;
        let mut comp_desc = AudioComponentDescription {
            componentType: 0,
            componentSubType: 0,
            componentManufacturer: 0,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let _ = unsafe {
            objc2_audio_toolbox::AudioComponentGetDescription(
                component,
                ptr::NonNull::new_unchecked(&raw mut comp_desc),
            )
        };
        // Skip AUv3 (app-extension) components — they require
        // AudioComponentInstantiate (async / sandboxed) and are
        // owned by truce-rack-au3. AUv3 components advertise themselves
        // via the IsV3AudioUnit flag in componentFlags.
        if AudioComponentFlags::from_bits_retain(comp_desc.componentFlags)
            .contains(AudioComponentFlags::IsV3AudioUnit)
        {
            continue;
        }
        let name = unsafe { component_name(component) };
        let (vendor, display) = split_name(&name);
        out.push(PluginInfo {
            name: display,
            vendor,
            version: unsafe { component_version(component) },
            category,
            path: std::path::PathBuf::new(),
            unique_id: format_unique_id(&comp_desc),
            format: FORMAT,
            has_editor: false,
            accepts_midi,
        });
    }
}

/// Query `kAudioUnitProperty_ParameterInfo` for one parameter.
unsafe fn fetch_parameter_info(
    unit: AudioComponentInstance,
    id: AudioUnitParameterID,
) -> Result<AudioUnitParameterInfo> {
    let mut info = AudioUnitParameterInfo {
        name: [0; 52],
        unitName: ptr::null(),
        clumpID: 0,
        cfNameString: ptr::null(),
        unit: objc2_audio_toolbox::AudioUnitParameterUnit::Generic,
        minValue: 0.0,
        maxValue: 0.0,
        defaultValue: 0.0,
        flags: objc2_audio_toolbox::AudioUnitParameterOptions::empty(),
    };
    #[allow(clippy::cast_possible_truncation)]
    let mut size = std::mem::size_of::<AudioUnitParameterInfo>() as u32;
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_ParameterInfo,
            kAudioUnitScope_Global,
            id,
            ptr::NonNull::new_unchecked((&raw mut info).cast()),
            ptr::NonNull::new_unchecked(&raw mut size),
        )
    };
    if status != 0 {
        return Err(Error::Other(format!(
            "kAudioUnitProperty_ParameterInfo failed: OSStatus {status}"
        )));
    }
    Ok(info)
}

#[allow(clippy::cast_sign_loss)]
fn au_parameter_to_rack(id: AudioUnitParameterID, info: &AudioUnitParameterInfo) -> ParameterInfo {
    // CFNameString takes precedence over the 52-byte ASCII name
    // field — AU plugins set the latter to empty more often than
    // not since the introduction of CFString-based names.
    let name = if info.cfNameString.is_null() {
        // SAFETY: `info.name` is a NUL-terminated 52-byte ASCII
        // buffer per AU spec.
        let bytes: Vec<u8> = info
            .name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        // SAFETY: `info.cfNameString` is a +0 reference per AU
        // spec — we don't release it.
        let s = unsafe { &*info.cfNameString };
        s.to_string()
    };
    let unit_name = if info.unitName.is_null() {
        String::new()
    } else {
        let s = unsafe { &*info.unitName };
        s.to_string()
    };
    let flags = au_param_flags_to_rack(info);
    ParameterInfo {
        id,
        name: name.clone(),
        short_name: name,
        unit: unit_name,
        min: f64::from(info.minValue),
        max: f64::from(info.maxValue),
        default: f64::from(info.defaultValue),
        step_count: 0,
        flags,
    }
}

fn au_param_flags_to_rack(info: &AudioUnitParameterInfo) -> truce_rack_core::info::ParameterFlags {
    use objc2_audio_toolbox::AudioUnitParameterOptions as Opt;
    let mut flags = truce_rack_core::info::ParameterFlags::empty();
    // AU treats most parameters as automatable by default; only
    // explicit MeterReadOnly takes that away.
    if info.flags.contains(Opt::Flag_MeterReadOnly) {
        flags |= truce_rack_core::info::ParameterFlags::READ_ONLY;
    } else {
        flags |= truce_rack_core::info::ParameterFlags::AUTOMATABLE;
    }
    if info.flags.contains(Opt::Flag_OmitFromPresets) {
        flags |= truce_rack_core::info::ParameterFlags::HIDDEN;
    }
    flags
}

/// Query `kAudioUnitProperty_FactoryPresets` and return one
/// [`PresetInfo`] per preset. The property hands us a `CFArrayRef`
/// of `AUPreset` structs — we copy the fields out and release the
/// array.
unsafe fn fetch_factory_presets(unit: AudioComponentInstance) -> Option<Vec<PresetInfo>> {
    use objc2_core_foundation::{CFArray, CFRetained};
    let mut presets: *const CFArray = ptr::null();
    let mut size = u32::try_from(std::mem::size_of::<*const CFArray>()).unwrap_or(0);
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_FactoryPresets,
            kAudioUnitScope_Global,
            0,
            ptr::NonNull::new_unchecked((&raw mut presets).cast()),
            ptr::NonNull::new_unchecked(&raw mut size),
        )
    };
    if status != 0 || presets.is_null() {
        return None;
    }
    // CFArrayRef from FactoryPresets is owned per Apple docs —
    // we wrap in CFRetained so it's released on drop.
    let array = unsafe { CFRetained::from_raw(ptr::NonNull::new_unchecked(presets.cast_mut())) };
    let count = array.count();
    if count <= 0 {
        return Some(Vec::new());
    }
    #[allow(clippy::cast_sign_loss)]
    let count_usize = count as usize;
    let mut out = Vec::with_capacity(count_usize);
    for i in 0..count {
        // CFArrayGetValueAtIndex returns *const c_void — points
        // at an `AUPreset` struct living inside the CFArray.
        let value = unsafe { array.value_at_index(i) };
        if value.is_null() {
            continue;
        }
        let preset = unsafe { &*value.cast::<AUPreset>() };
        let name = if preset.presetName.is_null() {
            format!("Preset {}", preset.presetNumber)
        } else {
            unsafe { &*preset.presetName }.to_string()
        };
        #[allow(clippy::cast_sign_loss)]
        out.push(PresetInfo {
            index: i as usize,
            name,
            preset_number: preset.presetNumber,
        });
    }
    Some(out)
}

/// Route one truce-rack [`truce_rack_core::events::Event`] into the AU via
/// `MusicDeviceMIDIEvent`. Non-MIDI events (param automation,
/// transport flags) are skipped — AU parameter automation goes
/// through `AudioUnitSetParameter` with a sample offset, and
/// transport flips are surfaced via the render-callback context.
fn send_midi(unit: AudioComponentInstance, event: &truce_rack_core::events::Event) {
    use truce_rack_core::events::{EventBody, MidiData};
    let offset = event.sample_offset;
    let (status, d1, d2) = match event.body {
        EventBody::Midi(MidiData::NoteOn {
            channel,
            note,
            velocity,
        }) => (
            0x90 | u32::from(channel & 0x0F),
            u32::from(note & 0x7F),
            u32::from(velocity & 0x7F),
        ),
        EventBody::Midi(MidiData::NoteOff {
            channel,
            note,
            velocity,
        }) => (
            0x80 | u32::from(channel & 0x0F),
            u32::from(note & 0x7F),
            u32::from(velocity & 0x7F),
        ),
        EventBody::Midi(MidiData::ControlChange {
            channel,
            controller,
            value,
        }) => (
            0xB0 | u32::from(channel & 0x0F),
            u32::from(controller & 0x7F),
            u32::from(value & 0x7F),
        ),
        EventBody::Midi(MidiData::ProgramChange { channel, program }) => (
            0xC0 | u32::from(channel & 0x0F),
            u32::from(program & 0x7F),
            0,
        ),
        EventBody::Midi(MidiData::ChannelAftertouch { channel, pressure }) => (
            0xD0 | u32::from(channel & 0x0F),
            u32::from(pressure & 0x7F),
            0,
        ),
        EventBody::Midi(MidiData::PolyAftertouch {
            channel,
            note,
            pressure,
        }) => (
            0xA0 | u32::from(channel & 0x0F),
            u32::from(note & 0x7F),
            u32::from(pressure & 0x7F),
        ),
        EventBody::Midi(MidiData::PitchBend { channel, value }) => (
            0xE0 | u32::from(channel & 0x0F),
            u32::from(value & 0x7F),
            u32::from((value >> 7) & 0x7F),
        ),
        EventBody::Midi(MidiData::Raw { len, data }) if len >= 1 => {
            let s = u32::from(data[0]);
            let d1 = if len >= 2 { u32::from(data[1]) } else { 0 };
            let d2 = if len >= 3 { u32::from(data[2]) } else { 0 };
            (s, d1, d2)
        }
        _ => return,
    };
    // Non-MusicDevice / non-MIDIFX components return
    // kAudioUnitErr_InvalidProperty here; ignore.
    let _ = unsafe { MusicDeviceMIDIEvent(unit, status, d1, d2, offset) };
}

/// Synchronous `AudioComponentInstanceNew` wrapper. Used for AU v2
/// and for AU v3 plugins that don't set `RequiresAsyncInstantiation`.
fn instantiate_sync(
    component: AudioComponent,
    info_path: &std::path::Path,
) -> Result<AudioComponentInstance> {
    let mut instance: AudioComponentInstance = ptr::null_mut();
    let status = unsafe {
        AudioComponentInstanceNew(component, ptr::NonNull::new_unchecked(&raw mut instance))
    };
    if status != 0 || instance.is_null() {
        return Err(Error::LoadFailed {
            path: info_path.to_path_buf(),
            reason: format!("AudioComponentInstanceNew failed with OSStatus {status}"),
        });
    }
    Ok(instance)
}

/// Asynchronous instantiation via `AudioComponentInstantiate`,
/// required for AU v3 plugins flagged `RequiresAsyncInstantiation`
/// (sandboxed App-Extension-hosted units). This synchronous scanner
/// path pumps the current run loop until completion or a bounded
/// deadline. Because the native callback can arrive after timeout,
/// shared completion state explicitly transfers or abandons ownership;
/// a late successful instance is disposed by the callback exactly once.
///
/// This remains a synchronous, reentrant run-loop wait. Apple warns
/// against waiting on the main thread because instantiation can deadlock;
/// native AUv3 loading through this API therefore still needs qualification
/// in the actual host lifecycle.
fn instantiate_async(
    component: AudioComponent,
    info_path: &std::path::Path,
) -> Result<AudioComponentInstance> {
    use std::time::{Duration, Instant};

    let completion = Arc::new(AsyncInstantiationCompletion::default());
    let callback_completion = Arc::clone(&completion);
    let deadline = Instant::now() + Duration::from_secs(10);

    // RcBlock heap-allocates the block and refcounts via Apple's
    // blocks runtime; AudioComponentInstantiate retains it for the
    // asynchronous completion. The captured Arc keeps its state alive
    // when the caller returns on timeout and the native callback is late.
    let block = block2::RcBlock::new(move |inst: AudioComponentInstance, status: i32| {
        if let Some(late_instance) = callback_completion.complete(inst as usize, status, deadline) {
            // SAFETY: the completion callback transfers each abandoned instance here once;
            // AudioComponentInstanceDispose has no caller-thread restriction in Apple's SDK.
            unsafe { dispose_instance(late_instance) };
        }
    });
    unsafe {
        AudioComponentInstantiate(
            component,
            AudioComponentInstantiationOptions::empty(),
            &block,
        );
    }

    // Pump the caller's current run loop in short slices so its reentrant
    // callbacks and queued work can progress during this legacy sync wait.
    // Completion is documented to run on an arbitrary thread and does
    // not depend on the callback being dispatched onto this run loop.
    loop {
        if Instant::now() >= deadline {
            // This mutex transition races atomically with callback completion. If completion
            // won, abandon takes its pointer for disposal; if timeout won, the callback will
            // dispose any later success. Exactly one side owns every non-null instance.
            if let Some(raw) = completion.abandon() {
                unsafe { dispose_instance(raw) };
            }
            return Err(Error::LoadFailed {
                path: info_path.to_path_buf(),
                reason: "AudioComponentInstantiate timed out after 10s".into(),
            });
        }
        if let Some((raw, status)) = completion.take_completed() {
            if status != 0 || raw == 0 {
                // A failed native completion may still supply a non-null instance.
                // Ownership was transferred to this thread, so release it here.
                unsafe { dispose_instance(raw) };
                return Err(Error::LoadFailed {
                    path: info_path.to_path_buf(),
                    reason: format!("AudioComponentInstantiate failed with OSStatus {status}"),
                });
            }
            return Ok(raw as AudioComponentInstance);
        }
        let mode = unsafe { objc2_core_foundation::kCFRunLoopDefaultMode };
        objc2_core_foundation::CFRunLoop::run_in_mode(mode, 0.05, true);
    }
}

/// Ownership state shared with the one-shot native completion callback.
#[derive(Default)]
struct AsyncInstantiationCompletion {
    state: Mutex<AsyncInstantiationState>,
}

#[derive(Clone, Copy, Default)]
enum AsyncInstantiationState {
    #[default]
    Pending,
    Completed {
        instance: usize,
        status: i32,
    },
    Abandoned,
    /// The callback arrived after abandonment and its instance was handed off for disposal.
    LateCompletion {
        instance: usize,
    },
    /// The waiting caller took ownership of a completed instance (including failed completions).
    Taken {
        instance: usize,
    },
}

impl AsyncInstantiationCompletion {
    /// Records a native completion. A returned nonzero pointer belongs to the caller for
    /// disposal because the timeout path had already abandoned the operation.
    fn complete(
        &self,
        instance: usize,
        status: i32,
        deadline: std::time::Instant,
    ) -> Option<usize> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match *state {
            AsyncInstantiationState::Pending => {
                if std::time::Instant::now() < deadline {
                    *state = AsyncInstantiationState::Completed { instance, status };
                    None
                } else {
                    *state = AsyncInstantiationState::LateCompletion { instance };
                    (instance != 0).then_some(instance)
                }
            }
            AsyncInstantiationState::Abandoned => {
                *state = AsyncInstantiationState::LateCompletion { instance };
                (instance != 0).then_some(instance)
            }
            AsyncInstantiationState::LateCompletion { instance: prior }
            | AsyncInstantiationState::Taken { instance: prior }
            | AsyncInstantiationState::Completed {
                instance: prior, ..
            } => (instance != 0 && instance != prior).then_some(instance),
        }
    }

    /// Transfers a completed result to the waiting caller at most once.
    fn take_completed(&self) -> Option<(usize, i32)> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let AsyncInstantiationState::Completed { instance, status } = *state else {
            return None;
        };
        *state = AsyncInstantiationState::Taken { instance };
        Some((instance, status))
    }

    /// Abandons a pending request or transfers an already-completed instance to timeout cleanup.
    fn abandon(&self) -> Option<usize> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match *state {
            AsyncInstantiationState::Pending => {
                *state = AsyncInstantiationState::Abandoned;
                None
            }
            AsyncInstantiationState::Completed { instance, .. } => {
                *state = AsyncInstantiationState::LateCompletion { instance };
                (instance != 0).then_some(instance)
            }
            AsyncInstantiationState::Abandoned
            | AsyncInstantiationState::LateCompletion { .. }
            | AsyncInstantiationState::Taken { .. } => None,
        }
    }
}

/// Dispose a non-null AU instance returned by an asynchronous completion.
///
/// # Safety
/// `instance` must be a live instance whose ownership has been transferred to this caller.
unsafe fn dispose_instance(instance: usize) {
    if instance != 0 {
        let instance = instance as AudioComponentInstance;
        // SAFETY: upheld by the caller's exclusive ownership of this completed instance.
        unsafe { AudioComponentInstanceDispose(instance) };
    }
}

fn type_category(type_code: u32) -> PluginCategory {
    match type_code {
        t if t == kAudioUnitType_MusicDevice => PluginCategory::Instrument,
        t if t == kAudioUnitType_MIDIProcessor => PluginCategory::NoteEffect,
        t if t == kAudioUnitType_Mixer => PluginCategory::Tool,
        _ => PluginCategory::Effect,
    }
}

/// AU `AudioComponentCopyName` returns `"<vendor>: <name>"`. Split
/// it so the host's browser shows them in separate columns.
fn split_name(full: &str) -> (String, String) {
    full.split_once(": ").map_or_else(
        || (String::new(), full.to_string()),
        |(v, n)| (v.to_string(), n.to_string()),
    )
}

unsafe fn component_name(component: AudioComponent) -> String {
    let mut cf_str: *const CFString = ptr::null();
    let status =
        unsafe { AudioComponentCopyName(component, ptr::NonNull::new_unchecked(&raw mut cf_str)) };
    if status != 0 || cf_str.is_null() {
        return String::new();
    }
    // SAFETY: AudioComponentCopyName follows the "Copy" rule —
    // we own the +1 retain on cf_str. Wrap it in CFRetained so it
    // gets released on drop.
    let retained = unsafe {
        objc2_core_foundation::CFRetained::from_raw(ptr::NonNull::new_unchecked(cf_str.cast_mut()))
    };
    retained.to_string()
}

unsafe fn component_version(component: AudioComponent) -> u32 {
    let mut version: u32 = 0;
    let _ = unsafe {
        objc2_audio_toolbox::AudioComponentGetVersion(
            component,
            ptr::NonNull::new_unchecked(&raw mut version),
        )
    };
    version
}

fn format_unique_id(desc: &AudioComponentDescription) -> String {
    format!(
        "{}:{}:{}",
        four_cc(desc.componentType),
        four_cc(desc.componentSubType),
        four_cc(desc.componentManufacturer),
    )
}

/// Render a four-CC as its printable ASCII representation if all
/// bytes are printable, otherwise its hex. AU manufacturer codes
/// are conventionally ASCII (`"appl"`, `"vlse"`, …).
fn four_cc(code: u32) -> String {
    let bytes = code.to_be_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() && *b != b':') {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("{code:08x}")
    }
}

/// Parse a `"type:sub:mfr"` unique id back into a description for
/// `AudioComponentFindNext`.
fn parse_unique_id(id: &str) -> Option<AudioComponentDescription> {
    let mut parts = id.split(':');
    let t = parse_four_cc(parts.next()?)?;
    let s = parse_four_cc(parts.next()?)?;
    let m = parse_four_cc(parts.next()?)?;
    Some(AudioComponentDescription {
        componentType: t,
        componentSubType: s,
        componentManufacturer: m,
        componentFlags: 0,
        componentFlagsMask: 0,
    })
}

fn parse_four_cc(s: &str) -> Option<u32> {
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_graphic()) {
        let b = s.as_bytes();
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    } else if s.len() == 8 {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    }
}

/// Convert an AU's reported latency in seconds to sample frames.
///
/// AU reports processing latency as `Float64` seconds. Rounding up ensures the host
/// does not under-compensate a fractional-frame report. Invalid rates, negative or
/// non-finite latency, and values that cannot fit the host's frame counter stay unknown.
fn latency_frames_from_seconds(seconds: f64, sample_rate: f64) -> Option<u32> {
    if !seconds.is_finite() || seconds < 0.0 || !sample_rate.is_finite() || sample_rate <= 0.0 {
        return None;
    }
    let frames = seconds * sample_rate;
    if !frames.is_finite() {
        return None;
    }
    // AU stores seconds as Float64. For a value derived from an integer frame count, rounding
    // the seconds and multiplying back can land a couple representable steps above that integer.
    // Bound that round-trip error by half an input-seconds ULP scaled to frames plus half a
    // product ULP; only snap inside this arithmetic bound. Genuine fractional-frame reports ceil.
    let round_trip_error =
        ((sample_rate * float_spacing(seconds) + float_spacing(frames)) * 0.5).next_up();
    let nearest_integer = frames.round();
    let rounded_frames = if (frames - nearest_integer).abs() <= round_trip_error {
        nearest_integer
    } else {
        frames.ceil()
    };
    if rounded_frames > f64::from(u32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(rounded_frames as u32)
}

/// Distance to the adjacent representable Float64 value for finite nonnegative input.
fn float_spacing(value: f64) -> f64 {
    let upward = value.next_up() - value;
    let downward = value - value.next_down();
    if upward.is_finite() {
        upward.max(downward)
    } else {
        downward
    }
}

/// Read the AU latency property on the serialized plug-in lifecycle thread.
///
/// Apple's AU contract defines an unimplemented latency property as zero latency;
/// other property-query failures are not reliable zeroes and remain unknown.
///
/// # Safety
/// `unit` must be a live initialized AU instance, and the caller must serialize this property
/// query with processing and other native calls on that instance.
unsafe fn query_latency_frames(unit: AudioComponentInstance, sample_rate: f64) -> Option<u32> {
    let mut seconds = 0.0_f64;
    let mut size = u32::try_from(std::mem::size_of::<f64>()).ok()?;
    // SAFETY: `unit` is the live initialized instance owned by `AuPlugin`; `seconds` and `size`
    // are valid writable storage of the exact property type/size for the duration of the call.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            ptr::NonNull::new((&raw mut seconds).cast::<std::ffi::c_void>())?,
            ptr::NonNull::new(&raw mut size)?,
        )
    };
    if status == kAudioUnitErr_InvalidProperty {
        return Some(0);
    }
    if status != 0 || usize::try_from(size).ok()? != std::mem::size_of::<f64>() {
        return None;
    }
    latency_frames_from_seconds(seconds, sample_rate)
}

/// AU property-listener callback. It deliberately touches no native state: all
/// latency queries happen later from `service_main_thread` on the serialized owner.
unsafe extern "C-unwind" fn latency_property_changed(
    user_data: std::ptr::NonNull<std::ffi::c_void>,
    _unit: AudioUnit,
    _property_id: AudioUnitPropertyID,
    _scope: AudioUnitScope,
    _element: AudioUnitElement,
) {
    latency_signal::signal(user_data);
}

fn latency_snapshot(cached: Option<u32>, signal: Option<&LatencySignalLease<'_>>) -> Option<u32> {
    match signal {
        Some(lease) if !lease.is_dirty() => cached,
        _ => None,
    }
}

fn take_latency_invalidation(signal: Option<&LatencySignalLease<'_>>) -> bool {
    signal.is_some_and(|lease| lease.take_invalidation())
}

fn with_latency_invalidation<T>(
    cached: &mut Option<u32>,
    signal: Option<&LatencySignalLease<'_>>,
    mutation: impl FnOnce() -> T,
) -> T {
    *cached = None;
    if let Some(signal) = signal {
        signal.mark_dirty();
    }
    mutation()
}

fn check_creator_thread(creator: &ThreadId) -> Result<()> {
    if std::thread::current().id().eq(creator) {
        Ok(())
    } else {
        Err(Error::Other(
            "Audio Unit control API called from a non-creator thread".into(),
        ))
    }
}

/// One loaded AU instance. Holds the `AudioComponentInstance` returned by synchronous or
/// asynchronous component instantiation and disposes it on `Drop`.
pub struct AuPlugin {
    info: PluginInfo,
    layouts: Vec<BusLayout>,
    input_bus_channels: Vec<u32>,
    output_bus_channels: Vec<u32>,
    active_layout: Option<BusLayout>,
    // Native initialization can outlive a failed activation or failed uninitialization.
    initialized: bool,
    /// Sample rate for the current activation, used only to convert the AU's seconds value.
    sample_rate: f64,
    /// Maximum host block size accepted by the current activation.
    max_block_size: usize,
    /// Cached intrinsic latency; native property calls stay on the serialized lifecycle path.
    latency_frames: Option<u32>,
    /// Fixed-slot, generation-checked callback lease; it is released only after unit disposal.
    latency_signal: Option<LatencySignalLease<'static>>,
    /// All serialized AU control APIs remain bound to the instance's creation thread.
    creator_thread: ThreadId,
    instance: AudioComponentInstance,
    /// Parameter id list, cached at load. AU exposes parameters
    /// by `AudioUnitParameterID` (u32); truce-rack-core's trait surface
    /// uses zero-based indices. This vec is the index → id map.
    param_ids: Vec<AudioUnitParameterID>,
    /// Stable-address render plumbing — the AU calls our render
    /// callback during `AudioUnitRender` with this Box's pointer as
    /// `inRefCon`. Kept boxed so its address survives moves of
    /// `AuPlugin` itself.
    render_ctx: Option<Box<RenderContext>>,
    /// One preallocated variable-length output `AudioBufferList` per AU output bus.
    output_buffer_storage: Vec<Vec<u8>>,
    /// Running output sample counter for `AudioTimeStamp.mSampleTime`.
    sample_time: f64,
    /// Heap-stable host-transport snapshot whose address is handed
    /// to the AU as `HostCallbackInfo::hostUserData`. `process()`
    /// refreshes it each block; the AU reads it back through the
    /// registered callbacks during `AudioUnitRender`.
    host_transport: Option<Box<HostTransport>>,
    /// Live editor state, populated by `PluginEditor::open`. The
    /// view-factory object (`AUCocoaUIBase`) and the resulting
    /// `NSView` are kept alive here; dropping the plugin tears them
    /// down via objc2's automatic retain/release.
    editor: AuEditorState,
    /// AUv3 views use a view-controller request instead of the AUv2 Cocoa factory.
    is_v3: bool,
}

/// Per-plugin Cocoa-view editor state. Lives on the main thread
/// only — never touched from the audio thread.
#[derive(Default)]
struct AuEditorState {
    factory: Option<objc2::rc::Retained<objc2::runtime::AnyObject>>,
    controller: Option<objc2::rc::Retained<objc2_app_kit::NSViewController>>,
    view: Option<objc2::rc::Retained<objc2_app_kit::NSView>>,
}

/// Audio-thread state shared with the AU render callback. The
/// callback fills the AU's input buffer from `input_planes` —
/// `process()` refreshes these pointers each block.
struct RenderContext {
    /// Flattened one pointer per channel in AU input-element order. NULL means silence.
    input_planes: Vec<*const f32>,
    /// Flat plane range for each AU input element, indexed by the callback bus number.
    input_bus_ranges: Vec<BusPlaneRange>,
    /// Maximum supported frame count for a render callback in this activation.
    frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BusPlaneRange {
    start: usize,
    len: usize,
}

fn bus_plane_ranges(channel_counts: &[u32]) -> Option<Vec<BusPlaneRange>> {
    let mut ranges = Vec::with_capacity(channel_counts.len());
    let mut start = 0_usize;
    for &channels in channel_counts {
        let len = usize::try_from(channels).ok()?;
        ranges.push(BusPlaneRange { start, len });
        start = start.checked_add(len)?;
    }
    Some(ranges)
}

fn bus_plane_index(ranges: &[BusPlaneRange], bus: u32, channel: usize) -> Option<usize> {
    let range = ranges.get(usize::try_from(bus).ok()?)?;
    if channel >= range.len {
        return None;
    }
    range.start.checked_add(channel)
}

unsafe extern "C-unwind" fn render_input_callback(
    in_ref_con: std::ptr::NonNull<std::ffi::c_void>,
    _io_action_flags: std::ptr::NonNull<AudioUnitRenderActionFlags>,
    _in_time_stamp: std::ptr::NonNull<AudioTimeStamp>,
    in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    if io_data.is_null() {
        return -1;
    }
    let ctx_ptr = in_ref_con.as_ptr().cast::<RenderContext>();
    let ctx = unsafe { &*ctx_ptr };
    let list = unsafe { &mut *io_data };
    let frames = usize::try_from(in_number_frames).unwrap_or(usize::MAX);
    let buffer_count = list.mNumberBuffers as usize;
    // The variable-length tail is reached via mBuffers as a [_; 1]
    // declared slice we extend via pointer arithmetic.
    let buffers_ptr: *mut CAAudioBuffer = list.mBuffers.as_mut_ptr();
    let bytes_u32 = frames
        .checked_mul(std::mem::size_of::<f32>())
        .and_then(|bytes| u32::try_from(bytes).ok());
    let range = usize::try_from(in_bus_number)
        .ok()
        .and_then(|index| ctx.input_bus_ranges.get(index));
    let mut failed = !matches!(range, Some(bus) if bus.len == buffer_count)
        || frames > ctx.frames
        || bytes_u32.is_none();
    for i in 0..buffer_count {
        let buffer = unsafe { &mut *buffers_ptr.add(i) };
        if let Some(plane_index) = bus_plane_index(&ctx.input_bus_ranges, in_bus_number, i)
            && let Some(&plane) = ctx.input_planes.get(plane_index)
            && !plane.is_null()
            && frames <= ctx.frames
            && let Some(bytes) = bytes_u32
        {
            buffer.mDataByteSize = bytes;
            buffer.mData = plane.cast::<std::ffi::c_void>().cast_mut();
            continue;
        }
        failed = true;
        // Silence only the storage the AU actually supplied. Never trust the requested frame
        // count to describe the capacity of this callback-owned buffer.
        if !buffer.mData.is_null() {
            let capacity = usize::try_from(buffer.mDataByteSize).unwrap_or(0);
            let silence_bytes = bytes_u32.map_or(capacity, |bytes| {
                capacity.min(usize::try_from(bytes).unwrap_or(0))
            });
            unsafe {
                std::ptr::write_bytes(buffer.mData.cast::<u8>(), 0, silence_bytes);
            }
        }
    }
    if !failed { 0 } else { -1 }
}

/// Host transport snapshot the AU reads via its `HostCallbackInfo`
/// procs. `process()` refreshes this before each `AudioUnitRender`;
/// the AU may then call the procs synchronously on the same audio
/// thread, so no locking is needed. Musical positions are in
/// quarter notes (the AU "beat" unit).
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct HostTransport {
    /// False until `process()` sets a transport for the block; the
    /// procs then report `kAudioUnitErr_CannotDoInCurrentContext`.
    valid: bool,
    tempo: f64,
    /// Current position in quarter notes.
    beat: f64,
    /// Downbeat of the current bar, in quarter notes.
    bar_start_beat: f64,
    time_sig_num: f32,
    time_sig_den: u32,
    sample_in_timeline: f64,
    sample_rate: f64,
    playing: bool,
    recording: bool,
    cycling: bool,
}

/// Copy a host [`TransportInfo`] block into the shared
/// [`HostTransport`]. Falls back to the running sample counter when
/// the host didn't report a sample position, and clears `valid`
/// when there's no transport at all.
#[allow(clippy::cast_precision_loss)]
fn update_host_transport(
    ht: &mut HostTransport,
    transport: Option<TransportInfo>,
    sample_rate: f64,
    sample_time: f64,
) {
    let Some(t) = transport else {
        ht.valid = false;
        return;
    };
    let (num, den) = t.time_signature.unwrap_or((4, 4));
    ht.valid = true;
    ht.tempo = t.tempo_bpm.unwrap_or(120.0);
    ht.beat = t.song_position_beats.unwrap_or(0.0);
    ht.bar_start_beat = t.bar_start_beats.unwrap_or(0.0);
    ht.time_sig_num = num as f32;
    ht.time_sig_den = den;
    ht.sample_in_timeline = t.song_position_samples.map_or(sample_time, |s| s as f64);
    ht.sample_rate = sample_rate;
    ht.playing = t.playing;
    ht.recording = t.recording;
    ht.cycling = t.loop_active;
}

/// `HostCallback_GetBeatAndTempo` — current beat and tempo.
unsafe extern "C-unwind" fn host_beat_and_tempo(
    in_host_user_data: *mut std::ffi::c_void,
    out_beat: *mut f64,
    out_tempo: *mut f64,
) -> i32 {
    let t = unsafe { &*in_host_user_data.cast::<HostTransport>() };
    if !t.valid {
        return kAudioUnitErr_CannotDoInCurrentContext;
    }
    if !out_beat.is_null() {
        unsafe { *out_beat = t.beat };
    }
    if !out_tempo.is_null() {
        unsafe { *out_tempo = t.tempo };
    }
    0
}

/// `HostCallback_GetMusicalTimeLocation` — time signature, the
/// current bar's downbeat, and samples until the next whole beat.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
unsafe extern "C-unwind" fn host_musical_time(
    in_host_user_data: *mut std::ffi::c_void,
    out_delta_to_next_beat: *mut u32,
    out_time_sig_num: *mut f32,
    out_time_sig_den: *mut u32,
    out_measure_downbeat: *mut f64,
) -> i32 {
    let t = unsafe { &*in_host_user_data.cast::<HostTransport>() };
    if !t.valid {
        return kAudioUnitErr_CannotDoInCurrentContext;
    }
    if !out_delta_to_next_beat.is_null() {
        let frac = t.beat - t.beat.floor();
        let samples_per_beat = if t.tempo > 0.0 {
            t.sample_rate * 60.0 / t.tempo
        } else {
            0.0
        };
        let delta = ((1.0 - frac) * samples_per_beat).round();
        let delta = if delta.is_finite() && delta >= 0.0 {
            delta as u32
        } else {
            0
        };
        unsafe { *out_delta_to_next_beat = delta };
    }
    if !out_time_sig_num.is_null() {
        unsafe { *out_time_sig_num = t.time_sig_num };
    }
    if !out_time_sig_den.is_null() {
        unsafe { *out_time_sig_den = t.time_sig_den };
    }
    if !out_measure_downbeat.is_null() {
        unsafe { *out_measure_downbeat = t.bar_start_beat };
    }
    0
}

/// `HostCallback_GetTransportState` — play / cycle state and the
/// current sample position on the host timeline.
unsafe extern "C-unwind" fn host_transport_state(
    in_host_user_data: *mut std::ffi::c_void,
    out_is_playing: *mut u8,
    out_transport_changed: *mut u8,
    out_sample_in_timeline: *mut f64,
    out_is_cycling: *mut u8,
    out_cycle_start: *mut f64,
    out_cycle_end: *mut f64,
) -> i32 {
    let t = unsafe { &*in_host_user_data.cast::<HostTransport>() };
    if !t.valid {
        return kAudioUnitErr_CannotDoInCurrentContext;
    }
    if !out_is_playing.is_null() {
        unsafe { *out_is_playing = u8::from(t.playing) };
    }
    if !out_transport_changed.is_null() {
        unsafe { *out_transport_changed = 0 };
    }
    if !out_sample_in_timeline.is_null() {
        unsafe { *out_sample_in_timeline = t.sample_in_timeline };
    }
    if !out_is_cycling.is_null() {
        unsafe { *out_is_cycling = u8::from(t.cycling) };
    }
    if !out_cycle_start.is_null() {
        unsafe { *out_cycle_start = 0.0 };
    }
    if !out_cycle_end.is_null() {
        unsafe { *out_cycle_end = 0.0 };
    }
    0
}

/// `HostCallback_GetTransportState2` — same as
/// [`host_transport_state`] plus the record-enable flag.
unsafe extern "C-unwind" fn host_transport_state2(
    in_host_user_data: *mut std::ffi::c_void,
    out_is_playing: *mut u8,
    out_is_recording: *mut u8,
    out_transport_changed: *mut u8,
    out_sample_in_timeline: *mut f64,
    out_is_cycling: *mut u8,
    out_cycle_start: *mut f64,
    out_cycle_end: *mut f64,
) -> i32 {
    let t = unsafe { &*in_host_user_data.cast::<HostTransport>() };
    if !t.valid {
        return kAudioUnitErr_CannotDoInCurrentContext;
    }
    if !out_is_playing.is_null() {
        unsafe { *out_is_playing = u8::from(t.playing) };
    }
    if !out_is_recording.is_null() {
        unsafe { *out_is_recording = u8::from(t.recording) };
    }
    if !out_transport_changed.is_null() {
        unsafe { *out_transport_changed = 0 };
    }
    if !out_sample_in_timeline.is_null() {
        unsafe { *out_sample_in_timeline = t.sample_in_timeline };
    }
    if !out_is_cycling.is_null() {
        unsafe { *out_is_cycling = u8::from(t.cycling) };
    }
    if !out_cycle_start.is_null() {
        unsafe { *out_cycle_start = 0.0 };
    }
    if !out_cycle_end.is_null() {
        unsafe { *out_cycle_end = 0.0 };
    }
    0
}

// SAFETY: AU v2 instances are not movable across audio rendering
// threads while active, but we serialize all access through
// `&mut self` and never share the pointer.
unsafe impl Send for AuPlugin {}

impl AuPlugin {
    fn invalidate_latency(&mut self) {
        self.latency_frames = None;
        if let Some(signal) = self.latency_signal.as_ref() {
            signal.mark_dirty();
        }
    }

    fn ensure_creator_thread(&self) -> Result<()> {
        check_creator_thread(&self.creator_thread)
    }

    fn validate_activation_request(
        &self,
        layout: &BusLayout,
        sample_rate: f64,
        max_block_size: usize,
    ) -> Result<u32> {
        if self.initialized {
            return Err(Error::Other(
                "Audio Unit must be deactivated before reconfiguration".into(),
            ));
        }
        if !self.layouts.contains(layout) {
            return Err(Error::Other(
                "requested Audio Unit bus layout was not verified by native discovery".into(),
            ));
        }
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(Error::Other("invalid Audio Unit sample rate".into()));
        }
        u32::try_from(max_block_size)
            .ok()
            .filter(|frames| *frames > 0)
            .ok_or_else(|| Error::Other("invalid Audio Unit maximum block size".into()))
    }

    fn prepare_activation_storage(&mut self, max_block_size: usize) -> Result<()> {
        let ranges = bus_plane_ranges(&self.input_bus_channels)
            .ok_or_else(|| Error::Other("Audio Unit input plane range overflow".into()))?;
        let plane_count = match ranges.last() {
            Some(range) => range
                .start
                .checked_add(range.len)
                .ok_or_else(|| Error::Other("Audio Unit input plane range overflow".into()))?,
            None => 0,
        };
        if let Some(ctx) = self.render_ctx.as_deref_mut() {
            ctx.input_planes.resize(plane_count, ptr::null());
            ctx.input_planes.fill(ptr::null());
            ctx.input_bus_ranges = ranges;
            ctx.frames = max_block_size;
        } else {
            self.render_ctx = Some(Box::new(RenderContext {
                input_planes: vec![ptr::null(); plane_count],
                input_bus_ranges: ranges,
                frames: max_block_size,
            }));
        }
        let mut host_transport = self
            .host_transport
            .take()
            .unwrap_or_else(|| Box::new(HostTransport::default()));
        *host_transport = HostTransport::default();
        self.host_transport = Some(host_transport);
        self.output_buffer_storage = self
            .output_bus_channels
            .iter()
            .map(|&channels| alloc_audio_buffer_list(channels as usize))
            .collect();
        Ok(())
    }

    /// Install input render callbacks after their stable boxed refcon has been prepared.
    ///
    /// # Safety
    /// The instance is live, inactive, and exclusively accessed by activation.
    unsafe fn install_input_callbacks(&mut self) -> std::result::Result<(), String> {
        let render_ctx_ptr = self
            .render_ctx
            .as_deref_mut()
            .map_or(ptr::null_mut(), |ctx| ctx as *mut RenderContext);
        if render_ctx_ptr.is_null() {
            return Err("Audio Unit render context missing".into());
        }
        for index in 0..self.input_bus_channels.len() {
            let element = u32::try_from(index)
                .map_err(|_| "Audio Unit input bus index overflow".to_owned())?;
            let callback_struct = AURenderCallbackStruct {
                inputProc: Some(render_input_callback),
                inputProcRefCon: render_ctx_ptr.cast(),
            };
            let status = unsafe {
                AudioUnitSetProperty(
                    self.instance,
                    kAudioUnitProperty_SetRenderCallback,
                    kAudioUnitScope_Input,
                    element,
                    (&raw const callback_struct).cast(),
                    u32::try_from(std::mem::size_of::<AURenderCallbackStruct>()).unwrap_or(0),
                )
            };
            if status != 0 {
                return Err(format!(
                    "AudioUnitSetProperty(SetRenderCallback, input element {element}) failed: OSStatus {status}"
                ));
            }
        }
        Ok(())
    }

    /// Install optional transport callbacks backed by the retained host snapshot.
    ///
    /// # Safety
    /// The instance and transport box are live and exclusively accessed by activation.
    unsafe fn install_host_callbacks(&mut self) {
        let Some(host_transport) = self.host_transport.as_deref_mut() else {
            return;
        };
        let host_callbacks = HostCallbackInfo {
            hostUserData: (host_transport as *mut HostTransport).cast(),
            beatAndTempoProc: Some(host_beat_and_tempo),
            musicalTimeLocationProc: Some(host_musical_time),
            transportStateProc: Some(host_transport_state),
            transportStateProc2: Some(host_transport_state2),
        };
        // Host transport callbacks are optional; preserve the historical best-effort behavior.
        let _ = unsafe {
            AudioUnitSetProperty(
                self.instance,
                kAudioUnitProperty_HostCallbacks,
                kAudioUnitScope_Global,
                0,
                (&raw const host_callbacks).cast(),
                u32::try_from(std::mem::size_of::<HostCallbackInfo>()).unwrap_or(0),
            )
        };
    }

    fn complete_activation(&mut self, layout: BusLayout, sample_rate: f64, max_block_size: usize) {
        // Initialization settles sample-rate-dependent parameter state.
        self.param_ids = unsafe { fetch_parameter_ids(self.instance) };
        self.sample_time = 0.0;
        self.sample_rate = sample_rate;
        self.max_block_size = max_block_size;
        if let Some(signal) = self.latency_signal.as_ref() {
            signal.take_invalidation();
        }
        self.latency_frames = self
            .latency_signal
            .as_ref()
            .and_then(|_| unsafe { query_latency_frames(self.instance, sample_rate) });
        self.active_layout = Some(layout);
    }

    fn initialize_selected_layout(
        &mut self,
        layout: &BusLayout,
        sample_rate: f64,
        previous: &[NativeBusFormat],
    ) -> Result<()> {
        // SAFETY: activation owns the instance exclusively and retains both callback boxes.
        let status = unsafe { AudioUnitInitialize(self.instance) };
        if status != 0 {
            // A failed initializer may have acquired resources. Confirm teardown before
            // advertising a reusable instance or restoring its stream formats.
            self.initialized = true;
            self.deactivate();
            if self.initialized {
                return Err(Error::Other(format!(
                    "AudioUnitInitialize failed: OSStatus {status}; AudioUnitUninitialize failed"
                )));
            }
            return Err(unsafe {
                activation_error(
                    self.instance,
                    previous,
                    Error::ActivationRejected {
                        reason: format!("AudioUnitInitialize failed: OSStatus {status}"),
                    },
                )
            });
        }
        self.initialized = true;
        // Initialization may change element counts or formats; never publish the requested
        // layout as active until the native instance agrees with it.
        if let Err(cause) = unsafe { verify_native_formats(self.instance, layout, sample_rate) } {
            self.deactivate();
            if self.initialized {
                return Err(Error::Other(format!(
                    "{cause}; AudioUnitUninitialize failed; instance cannot be reactivated"
                )));
            }
            return Err(unsafe { activation_error(self.instance, previous, Error::Other(cause)) });
        }
        Ok(())
    }

    fn load_from(info: &PluginInfo, selected: Option<&BusLayout>) -> Result<Self> {
        let mut desc = parse_unique_id(&info.unique_id).ok_or_else(|| Error::LoadFailed {
            path: info.path.clone(),
            reason: format!("could not parse AU unique_id {:?}", info.unique_id),
        })?;
        let component = unsafe {
            AudioComponentFindNext(ptr::null_mut(), ptr::NonNull::new_unchecked(&raw mut desc))
        };
        if component.is_null() {
            return Err(Error::LoadFailed {
                path: info.path.clone(),
                reason: format!("no AU matching {}", info.unique_id),
            });
        }

        // Re-query the component's actual flags so we can detect
        // RequiresAsyncInstantiation — `desc` only carries our scan
        // mask, not the component's own.
        let mut comp_desc = AudioComponentDescription {
            componentType: 0,
            componentSubType: 0,
            componentManufacturer: 0,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let _ = unsafe {
            objc2_audio_toolbox::AudioComponentGetDescription(
                component,
                ptr::NonNull::new_unchecked(&raw mut comp_desc),
            )
        };
        let flags = AudioComponentFlags::from_bits_retain(comp_desc.componentFlags);
        let needs_async = flags.contains(AudioComponentFlags::RequiresAsyncInstantiation);

        let instance = if needs_async {
            instantiate_async(component, &info.path)?
        } else {
            instantiate_sync(component, &info.path)?
        };
        // SAFETY: the new instance is exclusively owned here; either path only accesses this
        // inactive instance and any failure disposes it before it can escape.
        let topology = unsafe {
            match selected {
                Some(layout) => channel_layouts::select(instance, layout),
                None => channel_layouts::discover(instance),
            }
        };
        let (layouts, input_bus_channels, output_bus_channels) = match topology {
            Ok(topology) => topology,
            Err(error) => {
                // SAFETY: the native instance is live and has not escaped this failed load.
                unsafe { AudioComponentInstanceDispose(instance) };
                return Err(error);
            }
        };
        let latency_signal = acquire_latency_signal().and_then(|signal| {
            let registered = unsafe {
                AudioUnitAddPropertyListener(
                    instance,
                    kAudioUnitProperty_Latency,
                    Some(latency_property_changed),
                    signal.user_data(),
                ) == 0
            };
            registered.then_some(signal)
        });
        let param_ids = unsafe { fetch_parameter_ids(instance) };
        let is_v3 = flags.contains(AudioComponentFlags::IsV3AudioUnit);
        let has_editor = if is_v3 {
            unsafe { au3_editor::has_custom_view(component) }
        } else {
            unsafe { has_cocoa_ui(instance) }
        };
        let mut updated = info.clone();
        updated.has_editor = has_editor;
        Ok(Self {
            info: updated,
            layouts,
            input_bus_channels,
            output_bus_channels,
            active_layout: None,
            initialized: false,
            sample_rate: 0.0,
            max_block_size: 0,
            latency_frames: None,
            latency_signal,
            creator_thread: std::thread::current().id(),
            instance,
            param_ids,
            render_ctx: None,
            output_buffer_storage: Vec::new(),
            sample_time: 0.0,
            host_transport: None,
            editor: AuEditorState::default(),
            is_v3,
        })
    }
}

/// True if the AU advertises an `AUv2` Cocoa view (one or more
/// classes implementing `AUCocoaUIBase`).
unsafe fn has_cocoa_ui(unit: AudioComponentInstance) -> bool {
    use objc2_audio_toolbox::kAudioUnitProperty_CocoaUI;
    let mut size: u32 = 0;
    let info_status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &raw mut size,
            ptr::null_mut(),
        )
    };
    info_status == 0 && size as usize >= std::mem::size_of::<usize>() * 2
}

/// Allocate enough bytes to hold an `AudioBufferList` with
/// `n_buffers` `AudioBuffer`s tail-included. Returns a `Vec<u8>`
/// large enough; the caller casts the data pointer.
fn alloc_audio_buffer_list(n_buffers: usize) -> Vec<u8> {
    let n = n_buffers.max(1);
    let size =
        std::mem::size_of::<AudioBufferList>() + (n - 1) * std::mem::size_of::<CAAudioBuffer>();
    vec![0u8; size]
}

fn channel_config(channels: u32, native_layout_tag: Option<u32>) -> ChannelConfig {
    match (channels, native_layout_tag) {
        (1, Some(tag)) if tag == kAudioChannelLayoutTag_Mono => ChannelConfig::Mono,
        (2, Some(tag)) if tag == kAudioChannelLayoutTag_Stereo => ChannelConfig::Stereo,
        (6, Some(tag)) if tag == kAudioChannelLayoutTag_AudioUnit_5_1 => ChannelConfig::Surround5_1,
        (8, Some(tag)) if tag == kAudioChannelLayoutTag_AudioUnit_7_1 => ChannelConfig::Surround7_1,
        (other, _) => ChannelConfig::Discrete(other),
    }
}

fn selected_channel_config_matches(
    selected: ChannelConfig,
    channels: u32,
    native_layout_tag: Option<u32>,
) -> bool {
    let verified = channel_config(channels, native_layout_tag);
    selected == verified || selected == ChannelConfig::Discrete(channels)
}

fn validate_bus_channels(inputs: &[u32], outputs: &[u32]) -> Result<()> {
    if outputs.is_empty() {
        return Err(Error::Other("Audio Unit topology has no output bus".into()));
    }
    for (direction, counts) in [("input", inputs), ("output", outputs)] {
        if counts.len() > 64 {
            return Err(Error::Other(format!(
                "Audio Unit topology exceeds 64 {direction} buses"
            )));
        }
        let mut total = 0_u32;
        for &count in counts {
            if count == 0 {
                return Err(Error::Other(format!(
                    "Audio Unit topology has a zero-channel {direction} bus"
                )));
            }
            total = total.checked_add(count).ok_or_else(|| {
                Error::Other(format!("Audio Unit {direction} channel count overflow"))
            })?;
        }
        if total > 64 {
            return Err(Error::Other(format!(
                "Audio Unit topology exceeds 64 {direction} channels"
            )));
        }
    }
    Ok(())
}

fn native_bus_layout(
    input_channels: &[u32],
    input_names: &[Option<String>],
    input_layout_tags: &[Option<u32>],
    output_channels: &[u32],
    output_names: &[Option<String>],
    output_layout_tags: &[Option<u32>],
) -> Result<BusLayout> {
    validate_bus_channels(input_channels, output_channels)?;
    let mut layout = BusLayout::new();
    layout.id = None;
    for (index, &channels) in input_channels.iter().enumerate() {
        let fallback = if index == 0 {
            "Input".to_owned()
        } else {
            format!("Input {}", index + 1)
        };
        let name = input_names
            .get(index)
            .and_then(Option::as_ref)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or(fallback);
        layout.inputs.push(Bus {
            name,
            kind: if index == 0 {
                BusKind::Main
            } else {
                BusKind::Auxiliary
            },
            channels: channel_config(channels, input_layout_tags.get(index).copied().flatten()),
        });
    }
    for (index, &channels) in output_channels.iter().enumerate() {
        let fallback = if index == 0 {
            "Output".to_owned()
        } else {
            format!("Output {}", index + 1)
        };
        let name = output_names
            .get(index)
            .and_then(Option::as_ref)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or(fallback);
        layout.outputs.push(Bus {
            name,
            kind: if index == 0 {
                BusKind::Main
            } else {
                BusKind::Auxiliary
            },
            channels: channel_config(channels, output_layout_tags.get(index).copied().flatten()),
        });
    }
    Ok(layout)
}

/// Read a fixed-width UInt32 AU property.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize this query with all
/// other native access to the instance.
unsafe fn read_u32_property(
    unit: AudioComponentInstance,
    property: AudioUnitPropertyID,
    scope: AudioUnitScope,
    element: AudioUnitElement,
) -> Result<u32> {
    let mut value = 0_u32;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(0);
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            property,
            scope,
            element,
            ptr::NonNull::new((&raw mut value).cast())
                .ok_or_else(|| Error::Other("null Audio Unit property storage".into()))?,
            ptr::NonNull::new(&raw mut size)
                .ok_or_else(|| Error::Other("null Audio Unit property-size storage".into()))?,
        )
    };
    if status != 0 || usize::try_from(size).ok() != Some(std::mem::size_of::<u32>()) {
        return Err(Error::Other(format!(
            "AudioUnitGetProperty({property}, scope {scope}, element {element}) failed: OSStatus {status}, size {size}"
        )));
    }
    Ok(value)
}

/// Read one AU element's native stream format.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize this query with all
/// other native access to the instance.
unsafe fn read_stream_format(
    unit: AudioComponentInstance,
    scope: AudioUnitScope,
    element: AudioUnitElement,
) -> Result<AudioStreamBasicDescription> {
    let mut format = std::mem::MaybeUninit::<AudioStreamBasicDescription>::zeroed();
    let mut size = u32::try_from(std::mem::size_of::<AudioStreamBasicDescription>()).unwrap_or(0);
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            ptr::NonNull::new(format.as_mut_ptr().cast())
                .ok_or_else(|| Error::Other("null Audio Unit stream-format storage".into()))?,
            ptr::NonNull::new(&raw mut size)
                .ok_or_else(|| Error::Other("null Audio Unit property-size storage".into()))?,
        )
    };
    if status != 0
        || usize::try_from(size).ok() != Some(std::mem::size_of::<AudioStreamBasicDescription>())
    {
        return Err(Error::Other(format!(
            "AudioUnitGetProperty(StreamFormat, scope {scope}, element {element}) failed: OSStatus {status}, size {size}"
        )));
    }
    // SAFETY: the AU succeeded and reported exactly the initialized ASBD size.
    Ok(unsafe { format.assume_init() })
}

/// Read the retained name for one AU element, when the property is implemented.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize this query with all
/// other native access to the instance.
unsafe fn read_element_name(
    unit: AudioComponentInstance,
    scope: AudioUnitScope,
    element: AudioUnitElement,
) -> Option<String> {
    use objc2_core_foundation::CFRetained;

    let mut raw_name: *const CFString = ptr::null();
    let mut size = u32::try_from(std::mem::size_of::<*const CFString>()).ok()?;
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_ElementName,
            scope,
            element,
            ptr::NonNull::new((&raw mut raw_name).cast())?,
            ptr::NonNull::new(&raw mut size)?,
        )
    };
    if status != 0 || usize::try_from(size).ok() != Some(std::mem::size_of::<*const CFString>()) {
        return None;
    }
    let name_ptr = ptr::NonNull::new(raw_name.cast_mut())?;
    // SAFETY: AudioUnitProperties.h specifies that the host owns the returned CFStringRef.
    let name = unsafe { CFRetained::<CFString>::from_raw(name_ptr) };
    Some(name.to_string())
}

/// Read a recognized native channel-layout tag for an AU element.
///
/// Custom descriptions and unavailable/oversized layout properties remain unclassified, causing
/// the host-facing layout to use `Discrete(channel_count)` rather than guessing channel roles.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize this query with all
/// other native access to the instance.
unsafe fn read_channel_layout_tag(
    unit: AudioComponentInstance,
    scope: AudioUnitScope,
    element: AudioUnitElement,
) -> Option<u32> {
    let mut size = 0_u32;
    let info_status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_AudioChannelLayout,
            scope,
            element,
            &raw mut size,
            ptr::null_mut(),
        )
    };
    let header_size = std::mem::offset_of!(AudioChannelLayout, mChannelDescriptions);
    let max_size = header_size
        .checked_add(64_usize.checked_mul(std::mem::size_of::<AudioChannelDescription>())?)?;
    let property_size = usize::try_from(size).ok()?;
    if info_status != 0 || property_size < header_size || property_size > max_size {
        return None;
    }

    // Use u64 backing words to keep the variable-length structure suitably aligned. This runs
    // during topology discovery/activation, never from the render callback.
    let word_count =
        property_size.checked_add(std::mem::size_of::<u64>() - 1)? / std::mem::size_of::<u64>();
    let mut storage = vec![0_u64; word_count];
    let mut io_size = size;
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_AudioChannelLayout,
            scope,
            element,
            ptr::NonNull::new(storage.as_mut_ptr().cast())?,
            ptr::NonNull::new(&raw mut io_size)?,
        )
    };
    let returned_size = usize::try_from(io_size).ok()?;
    if status != 0 || returned_size < header_size || returned_size > property_size {
        return None;
    }

    let layout_ptr = storage.as_ptr().cast::<AudioChannelLayout>();
    // SAFETY: property retrieval wrote a full layout header into aligned storage, as bounded
    // above; both fields are plain integer values at the documented offsets.
    let tag = unsafe { ptr::read_unaligned(ptr::addr_of!((*layout_ptr).mChannelLayoutTag)) };
    let description_count =
        unsafe { ptr::read_unaligned(ptr::addr_of!((*layout_ptr).mNumberChannelDescriptions)) };
    (description_count == 0).then_some(tag)
}

/// Observe the AU's current per-element input and output topology.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize these queries with
/// all other native access to the instance.
unsafe fn discover_bus_layout(
    unit: AudioComponentInstance,
) -> Result<(BusLayout, Vec<u32>, Vec<u32>)> {
    let input_count = unsafe {
        read_u32_property(
            unit,
            kAudioUnitProperty_ElementCount,
            kAudioUnitScope_Input,
            0,
        )?
    };
    let output_count = unsafe {
        read_u32_property(
            unit,
            kAudioUnitProperty_ElementCount,
            kAudioUnitScope_Output,
            0,
        )?
    };
    let input_count = usize::try_from(input_count)
        .ok()
        .filter(|&count| count <= 64)
        .ok_or_else(|| Error::Other("Audio Unit input bus count exceeds 64".into()))?;
    let output_count = usize::try_from(output_count)
        .ok()
        .filter(|&count| count <= 64)
        .ok_or_else(|| Error::Other("Audio Unit output bus count exceeds 64".into()))?;
    let mut input_channels = Vec::with_capacity(input_count);
    let mut input_names = Vec::with_capacity(input_count);
    let mut input_layout_tags = Vec::with_capacity(input_count);
    for index in 0..input_count {
        let element = u32::try_from(index)
            .map_err(|_| Error::Other("Audio Unit input bus index overflow".into()))?;
        let format = unsafe { read_stream_format(unit, kAudioUnitScope_Input, element)? };
        input_channels.push(format.mChannelsPerFrame);
        input_names.push(unsafe { read_element_name(unit, kAudioUnitScope_Input, element) });
        input_layout_tags
            .push(unsafe { read_channel_layout_tag(unit, kAudioUnitScope_Input, element) });
    }
    let mut output_channels = Vec::with_capacity(output_count);
    let mut output_names = Vec::with_capacity(output_count);
    let mut output_layout_tags = Vec::with_capacity(output_count);
    for index in 0..output_count {
        let element = u32::try_from(index)
            .map_err(|_| Error::Other("Audio Unit output bus index overflow".into()))?;
        let format = unsafe { read_stream_format(unit, kAudioUnitScope_Output, element)? };
        output_channels.push(format.mChannelsPerFrame);
        output_names.push(unsafe { read_element_name(unit, kAudioUnitScope_Output, element) });
        output_layout_tags
            .push(unsafe { read_channel_layout_tag(unit, kAudioUnitScope_Output, element) });
    }
    let layout = native_bus_layout(
        &input_channels,
        &input_names,
        &input_layout_tags,
        &output_channels,
        &output_names,
        &output_layout_tags,
    )?;
    Ok((layout, input_channels, output_channels))
}

#[derive(Clone, Copy)]
struct NativeBusFormat {
    scope: AudioUnitScope,
    element: AudioUnitElement,
    format: AudioStreamBasicDescription,
}

fn planar_f32_format(sample_rate: f64, channels: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

fn render_format_matches(
    actual: &AudioStreamBasicDescription,
    expected: &AudioStreamBasicDescription,
) -> bool {
    actual.mSampleRate.to_bits() == expected.mSampleRate.to_bits()
        && actual.mFormatID == expected.mFormatID
        && actual.mFormatFlags == expected.mFormatFlags
        && actual.mBytesPerPacket == expected.mBytesPerPacket
        && actual.mFramesPerPacket == expected.mFramesPerPacket
        && actual.mBytesPerFrame == expected.mBytesPerFrame
        && actual.mChannelsPerFrame == expected.mChannelsPerFrame
        && actual.mBitsPerChannel == expected.mBitsPerChannel
}

/// Set one AU element's native stream format.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize this mutation with
/// all other native access to the instance.
unsafe fn set_stream_format(
    unit: AudioComponentInstance,
    scope: AudioUnitScope,
    element: AudioUnitElement,
    format: &AudioStreamBasicDescription,
) -> i32 {
    unsafe {
        AudioUnitSetProperty(
            unit,
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            (format as *const AudioStreamBasicDescription).cast(),
            u32::try_from(std::mem::size_of::<AudioStreamBasicDescription>()).unwrap_or(0),
        )
    }
}

/// Restore previously observed formats after a failed activation attempt.
///
/// # Safety
/// `unit` must be a live Audio Unit instance, and the caller must serialize native access to it.
unsafe fn restore_native_formats(
    unit: AudioComponentInstance,
    previous: &[NativeBusFormat],
) -> Vec<String> {
    let mut failures = Vec::new();
    for item in previous {
        let status = unsafe { set_stream_format(unit, item.scope, item.element, &item.format) };
        if status != 0 {
            failures.push(format!(
                "restore stream format scope {} element {} returned OSStatus {status}",
                item.scope, item.element
            ));
            continue;
        }
        match unsafe { read_stream_format(unit, item.scope, item.element) } {
            Ok(restored) if restored == item.format => {}
            Ok(_) => failures.push(format!(
                "restore stream format scope {} element {} did not reproduce the observed format",
                item.scope, item.element
            )),
            Err(error) => failures.push(format!(
                "verify restored stream format scope {} element {} failed: {error}",
                item.scope, item.element
            )),
        }
    }
    failures
}

/// Build a failure while restoring the AU's pre-activation stream formats.
///
/// # Safety
/// `unit` must be a live Audio Unit instance and native access must be serialized by the caller.
unsafe fn activation_error(
    unit: AudioComponentInstance,
    previous: &[NativeBusFormat],
    cause: Error,
) -> Error {
    let failures = unsafe { restore_native_formats(unit, previous) };
    if failures.is_empty() {
        cause
    } else {
        Error::Other(format!(
            "{cause}; additionally, native format rollback was incomplete: {}",
            failures.join("; ")
        ))
    }
}

/// Snapshot every native bus format before activation mutates any element.
///
/// # Safety
/// `unit` must be live and exclusively accessed by the caller.
unsafe fn capture_native_formats(
    unit: AudioComponentInstance,
    input_channels: &[u32],
    output_channels: &[u32],
) -> Result<Vec<NativeBusFormat>> {
    for (scope, expected) in [
        (kAudioUnitScope_Input, input_channels.len()),
        (kAudioUnitScope_Output, output_channels.len()),
    ] {
        let count = unsafe { read_u32_property(unit, kAudioUnitProperty_ElementCount, scope, 0)? };
        if usize::try_from(count).ok() != Some(expected) {
            return Err(Error::Other(
                "Audio Unit bus counts changed after topology discovery".into(),
            ));
        }
    }
    let mut formats = Vec::with_capacity(input_channels.len() + output_channels.len());
    for (scope, channels) in [
        (kAudioUnitScope_Input, input_channels),
        (kAudioUnitScope_Output, output_channels),
    ] {
        for index in 0..channels.len() {
            let element = u32::try_from(index)
                .map_err(|_| Error::Other("Audio Unit bus index overflow".into()))?;
            formats.push(NativeBusFormat {
                scope,
                element,
                // SAFETY: the instance is live and serialized by activation.
                format: unsafe { read_stream_format(unit, scope, element)? },
            });
        }
    }
    Ok(formats)
}

/// Apply and read back planar f32 formats and the observed bus topology.
///
/// # Safety
/// `unit` must be live and exclusively accessed by the caller. Any partial mutation must be
/// rolled back using the pre-activation snapshot if this returns an error.
unsafe fn apply_and_verify_native_formats(
    unit: AudioComponentInstance,
    input_channels: &[u32],
    output_channels: &[u32],
    layout: &BusLayout,
    sample_rate: f64,
) -> std::result::Result<(), String> {
    for (scope, channels) in [
        (kAudioUnitScope_Input, input_channels),
        (kAudioUnitScope_Output, output_channels),
    ] {
        for (index, &count) in channels.iter().enumerate() {
            let element = u32::try_from(index).map_err(|_| "Audio Unit bus index overflow")?;
            let expected = planar_f32_format(sample_rate, count);
            let status = unsafe { set_stream_format(unit, scope, element, &expected) };
            if status != 0 {
                return Err(format!(
                    "AudioUnitSetProperty(StreamFormat, scope {scope}, element {element}) failed: OSStatus {status}"
                ));
            }
        }
    }
    unsafe { verify_native_formats(unit, layout, sample_rate) }
}

/// Read back native counts, PCM formats and channel grouping without mutating the instance.
///
/// # Safety
/// The live instance is exclusively accessed on its creator thread, outside rendering.
unsafe fn verify_native_formats(
    unit: AudioComponentInstance,
    layout: &BusLayout,
    sample_rate: f64,
) -> std::result::Result<(), String> {
    for (scope, buses) in [
        (kAudioUnitScope_Input, layout.inputs.as_slice()),
        (kAudioUnitScope_Output, layout.outputs.as_slice()),
    ] {
        let count = unsafe { read_u32_property(unit, kAudioUnitProperty_ElementCount, scope, 0) }
            .map_err(|error| format!("verify Audio Unit bus count failed: {error}"))?;
        if usize::try_from(count).ok() != Some(buses.len()) {
            return Err(format!("Audio Unit bus count changed on scope {scope}"));
        }
        for (index, bus) in buses.iter().enumerate() {
            let element = u32::try_from(index).map_err(|_| "Audio Unit bus index overflow")?;
            let expected = planar_f32_format(sample_rate, bus.channels.count());
            let actual = unsafe { read_stream_format(unit, scope, element) }
                .map_err(|error| format!("verify Audio Unit stream format failed: {error}"))?;
            if !render_format_matches(&actual, &expected) {
                return Err(format!(
                    "Audio Unit did not accept planar f32 on scope {scope}, element {element}"
                ));
            }
            let layout_tag = unsafe { read_channel_layout_tag(unit, scope, element) };
            if !selected_channel_config_matches(bus.channels, actual.mChannelsPerFrame, layout_tag)
            {
                return Err(format!(
                    "Audio Unit channel layout changed on scope {scope}, element {element}"
                ));
            }
        }
    }
    Ok(())
}

/// Set the AU's maximum render quantum.
///
/// # Safety
/// `unit` must be live and exclusively accessed by the caller.
unsafe fn set_maximum_frames(
    unit: AudioComponentInstance,
    max_frames: u32,
) -> std::result::Result<(), String> {
    let status = unsafe {
        AudioUnitSetProperty(
            unit,
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            (&raw const max_frames).cast(),
            u32::try_from(std::mem::size_of::<u32>()).unwrap_or(0),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "AudioUnitSetProperty(MaximumFramesPerSlice) failed: OSStatus {status}"
        ))
    }
}

/// Query `kAudioUnitProperty_ParameterList` and return the
/// parameter ids in declaration order. Returns an empty vec on
/// any failure — a plugin with zero parameters is valid.
unsafe fn fetch_parameter_ids(unit: AudioComponentInstance) -> Vec<AudioUnitParameterID> {
    let mut size: u32 = 0;
    let info_status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global,
            0,
            &raw mut size,
            ptr::null_mut(),
        )
    };
    if info_status != 0 || size == 0 {
        return Vec::new();
    }
    let count = (size as usize) / std::mem::size_of::<AudioUnitParameterID>();
    let mut ids: Vec<AudioUnitParameterID> = vec![0; count];
    let mut io_size = size;
    let get_status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global,
            0,
            ptr::NonNull::new_unchecked(ids.as_mut_ptr().cast()),
            ptr::NonNull::new_unchecked(&raw mut io_size),
        )
    };
    if get_status != 0 {
        return Vec::new();
    }
    ids.truncate(io_size as usize / std::mem::size_of::<AudioUnitParameterID>());
    ids
}

impl Drop for AuPlugin {
    fn drop(&mut self) {
        if self.is_v3 && self.ensure_creator_thread().is_ok() {
            truce_rack_core::editor::PluginEditor::close(self);
        }
        if !self.instance.is_null() {
            if let Some(signal) = self.latency_signal.as_ref() {
                // SAFETY: the instance is live and this token is the exact registered refcon.
                unsafe {
                    AudioUnitRemovePropertyListenerWithUserData(
                        self.instance,
                        kAudioUnitProperty_Latency,
                        Some(latency_property_changed),
                        signal.user_data(),
                    );
                }
            }
            // The signal lease remains live through unit disposal, then its generation is
            // released by field drop. An in-flight callback afterward sees only its stale token.
            unsafe { AudioComponentInstanceDispose(self.instance) };
        }
    }
}

impl PluginCore for AuPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn active_layout(&self) -> Option<&BusLayout> {
        self.active_layout.as_ref()
    }
    fn latency_frames(&self) -> Option<u32> {
        latency_snapshot(self.latency_frames, self.latency_signal.as_ref())
    }
    fn service_main_thread(&mut self) -> Result<truce_rack_core::plugin::MainThreadUpdate> {
        self.ensure_creator_thread()?;
        if take_latency_invalidation(self.latency_signal.as_ref()) {
            self.latency_frames = if self.is_active() && self.latency_signal.is_some() {
                // SAFETY: this service hook runs with exclusive access on the instance's
                // serialized owner thread, apart from the atomic-only property callback.
                unsafe { query_latency_frames(self.instance, self.sample_rate) }
            } else {
                None
            };
        }
        Ok(MainThreadUpdate::Unchanged)
    }
    fn supported_layouts(&self) -> &[BusLayout] {
        &self.layouts
    }
    fn parameter_count(&self) -> usize {
        self.param_ids.len()
    }

    fn parameter_info(&self, index: usize) -> Result<ParameterInfo> {
        self.ensure_creator_thread()?;
        let id = *self
            .param_ids
            .get(index)
            .ok_or(Error::InvalidParameter(index))?;
        let info = unsafe { fetch_parameter_info(self.instance, id)? };
        Ok(au_parameter_to_rack(id, &info))
    }

    fn parameter_value(&self, index: usize) -> Result<f64> {
        self.ensure_creator_thread()?;
        let id = *self
            .param_ids
            .get(index)
            .ok_or(Error::InvalidParameter(index))?;
        let mut value: f32 = 0.0;
        let status = unsafe {
            AudioUnitGetParameter(
                self.instance,
                id,
                kAudioUnitScope_Global,
                0,
                ptr::NonNull::new_unchecked(&raw mut value),
            )
        };
        if status != 0 {
            return Err(Error::Other(format!(
                "AudioUnitGetParameter failed: OSStatus {status}"
            )));
        }
        Ok(f64::from(value))
    }

    fn parameter_value_string(&self, _index: usize, _value: f64) -> Result<String> {
        self.ensure_creator_thread()?;
        // AU exposes a parameter-value-to-string property via
        // kAudioUnitProperty_ParameterValueStrings (for indexed
        // params) and AUParameterStringFromValue (for everything
        // else). Neither maps cleanly to the truce-rack-core surface
        // yet; tracked as a follow-on. For now, fall back to the
        // unit-stripped numeric form so callers get *something*
        // they can display.
        Err(Error::Other(
            "au parameter_value_string not yet wired".into(),
        ))
    }

    fn set_parameter(&mut self, index: usize, value: f64) -> Result<()> {
        self.ensure_creator_thread()?;
        let id = *self
            .param_ids
            .get(index)
            .ok_or(Error::InvalidParameter(index))?;
        #[allow(clippy::cast_possible_truncation)]
        let v = value as f32;
        let instance = self.instance;
        let status = with_latency_invalidation(
            &mut self.latency_frames,
            self.latency_signal.as_ref(),
            || unsafe { AudioUnitSetParameter(instance, id, kAudioUnitScope_Global, 0, v, 0) },
        );
        if status != 0 {
            return Err(Error::Other(format!(
                "AudioUnitSetParameter failed: OSStatus {status}"
            )));
        }
        Ok(())
    }
    fn preset_count(&self) -> usize {
        if self.ensure_creator_thread().is_err() {
            return 0;
        }
        unsafe { fetch_factory_presets(self.instance) }.map_or(0, |v| v.len())
    }

    fn preset_info(&self, index: usize) -> Result<PresetInfo> {
        self.ensure_creator_thread()?;
        let presets = unsafe { fetch_factory_presets(self.instance) }
            .ok_or_else(|| Error::Other("kAudioUnitProperty_FactoryPresets failed".into()))?;
        let preset = presets.get(index).ok_or(Error::InvalidParameter(index))?;
        Ok(preset.clone())
    }

    fn load_preset(&mut self, preset_number: i32) -> Result<()> {
        self.ensure_creator_thread()?;
        let preset = AUPreset {
            presetNumber: preset_number,
            presetName: ptr::null(),
        };
        let instance = self.instance;
        let status = with_latency_invalidation(
            &mut self.latency_frames,
            self.latency_signal.as_ref(),
            || unsafe {
                AudioUnitSetProperty(
                    instance,
                    kAudioUnitProperty_PresentPreset,
                    kAudioUnitScope_Global,
                    0,
                    (&raw const preset).cast(),
                    u32::try_from(std::mem::size_of::<AUPreset>()).unwrap_or(0),
                )
            },
        );
        if status != 0 {
            return Err(Error::Other(format!(
                "AudioUnitSetProperty(PresentPreset) failed: OSStatus {status}"
            )));
        }
        Ok(())
    }
    fn save_state(&self) -> Result<Vec<u8>> {
        self.ensure_creator_thread()?;
        use objc2_core_foundation::{
            CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListFormat, CFRetained,
        };
        let mut class_info: *const CFPropertyList = ptr::null();
        let mut size = u32::try_from(std::mem::size_of::<*const CFPropertyList>()).unwrap_or(0);
        let status = unsafe {
            AudioUnitGetProperty(
                self.instance,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                ptr::NonNull::new_unchecked((&raw mut class_info).cast()),
                ptr::NonNull::new_unchecked(&raw mut size),
            )
        };
        if status != 0 || class_info.is_null() {
            return Err(Error::Other(format!(
                "kAudioUnitProperty_ClassInfo failed: OSStatus {status}"
            )));
        }
        let plist = unsafe {
            CFRetained::<CFPropertyList>::from_raw(ptr::NonNull::new_unchecked(
                class_info.cast_mut(),
            ))
        };
        let mut error: *mut objc2_core_foundation::CFError = ptr::null_mut();
        let data: Option<CFRetained<CFData>> = unsafe {
            CFPropertyListCreateData(
                None,
                Some(&plist),
                CFPropertyListFormat::BinaryFormat_v1_0,
                0,
                &raw mut error,
            )
        };
        let data =
            data.ok_or_else(|| Error::Other("CFPropertyListCreateData returned null".into()))?;
        let len = data.length();
        if len < 0 {
            return Err(Error::Other("CFData length negative".into()));
        }
        #[allow(clippy::cast_sign_loss)]
        let len_usize = len as usize;
        let mut out = vec![0u8; len_usize];
        unsafe {
            data.bytes(
                objc2_core_foundation::CFRange {
                    location: 0,
                    length: len,
                },
                out.as_mut_ptr(),
            );
        }
        Ok(out)
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        self.ensure_creator_thread()?;
        if self.initialized && !self.is_active() {
            return Err(Error::Other(
                "Audio Unit must finish uninitializing before state restoration".into(),
            ));
        }
        use objc2_core_foundation::{
            CFData, CFPropertyList, CFPropertyListCreateWithData, CFPropertyListFormat, CFRetained,
        };
        if bytes.is_empty() {
            return Err(Error::Other("empty AU state".into()));
        }
        #[allow(clippy::cast_possible_wrap)]
        let cf_data: CFRetained<CFData> =
            unsafe { CFData::new(None, bytes.as_ptr(), bytes.len() as isize) }
                .ok_or_else(|| Error::Other("CFData::new returned null".into()))?;
        let mut error: *mut objc2_core_foundation::CFError = ptr::null_mut();
        let mut format = CFPropertyListFormat::BinaryFormat_v1_0;
        let plist: Option<CFRetained<CFPropertyList>> = unsafe {
            CFPropertyListCreateWithData(None, Some(&cf_data), 0, &raw mut format, &raw mut error)
        };
        let plist = plist
            .ok_or_else(|| Error::Other("CFPropertyListCreateWithData returned null".into()))?;
        let plist_ptr: *const CFPropertyList = CFRetained::as_ptr(&plist).as_ptr().cast_const();
        let instance = self.instance;
        let status = with_latency_invalidation(
            &mut self.latency_frames,
            self.latency_signal.as_ref(),
            || unsafe {
                AudioUnitSetProperty(
                    instance,
                    kAudioUnitProperty_ClassInfo,
                    kAudioUnitScope_Global,
                    0,
                    (&raw const plist_ptr).cast(),
                    u32::try_from(std::mem::size_of::<*const CFPropertyList>()).unwrap_or(0),
                )
            },
        );
        if status != 0 {
            return Err(Error::Other(format!(
                "AudioUnitSetProperty(ClassInfo) failed: OSStatus {status}"
            )));
        }
        Ok(())
    }
    fn activate(
        &mut self,
        layout: BusLayout,
        sample_rate: f64,
        max_block_size: usize,
    ) -> Result<()> {
        self.ensure_creator_thread()?;
        let max_frames = self.validate_activation_request(&layout, sample_rate, max_block_size)?;
        self.invalidate_latency();
        self.sample_rate = 0.0;
        let previous_formats = unsafe {
            capture_native_formats(
                self.instance,
                &self.input_bus_channels,
                &self.output_bus_channels,
            )?
        };
        self.input_bus_channels = layout
            .inputs
            .iter()
            .map(|bus| bus.channels.count())
            .collect();
        self.output_bus_channels = layout
            .outputs
            .iter()
            .map(|bus| bus.channels.count())
            .collect();
        self.prepare_activation_storage(max_block_size)?;
        if let Err(cause) = unsafe {
            apply_and_verify_native_formats(
                self.instance,
                &self.input_bus_channels,
                &self.output_bus_channels,
                &layout,
                sample_rate,
            )
        } {
            return Err(unsafe {
                activation_error(self.instance, &previous_formats, Error::Other(cause))
            });
        }
        if let Err(cause) = unsafe { set_maximum_frames(self.instance, max_frames) } {
            return Err(unsafe {
                activation_error(self.instance, &previous_formats, Error::Other(cause))
            });
        }
        if let Err(cause) = unsafe { self.install_input_callbacks() } {
            return Err(unsafe {
                activation_error(self.instance, &previous_formats, Error::Other(cause))
            });
        }
        unsafe { self.install_host_callbacks() };
        self.initialize_selected_layout(&layout, sample_rate, &previous_formats)?;
        self.complete_activation(layout, sample_rate, max_block_size);
        Ok(())
    }
    fn deactivate(&mut self) {
        if self.ensure_creator_thread().is_err() {
            return;
        }
        if self.initialized {
            let status = unsafe { AudioUnitUninitialize(self.instance) };
            if status == 0 {
                self.initialized = false;
                self.render_ctx = None;
                self.host_transport = None;
            }
        }
        self.active_layout = None;
        self.invalidate_latency();
        self.sample_rate = 0.0;
        self.max_block_size = 0;
    }
    fn is_active(&self) -> bool {
        self.initialized
    }

    fn editor(&mut self) -> Option<&mut dyn truce_rack_core::editor::PluginEditor> {
        if self.ensure_creator_thread().is_ok() && self.info.has_editor {
            Some(self)
        } else {
            None
        }
    }
}

impl truce_rack_core::editor::PluginEditor for AuPlugin {
    fn open(&mut self, parent: truce_rack_core::editor::WindowHandle, _scale: f64) -> Result<()> {
        self.ensure_creator_thread()?;
        let _main = objc2::MainThreadMarker::new()
            .ok_or_else(|| Error::Other("AU editor requires the macOS main thread".into()))?;
        use truce_rack_core::editor::WindowHandle;
        let WindowHandle::NSView(parent_ptr) = parent else {
            return Err(Error::Other("AU editor requires an NSView parent".into()));
        };
        if parent_ptr.is_null() {
            return Err(Error::Other("AU editor: parent NSView is null".into()));
        }
        if self.editor.view.is_some() {
            return Ok(());
        }
        // Creating the native view may expose plug-in state that changes latency, even if
        // creation later fails partway through.
        self.invalidate_latency();
        if self.is_v3 {
            unsafe { open_au3_editor(self, parent_ptr) }
        } else {
            unsafe { open_cocoa_editor(self, parent_ptr) }
        }
    }

    fn close(&mut self) {
        if self.ensure_creator_thread().is_err() || objc2::MainThreadMarker::new().is_none() {
            return;
        }
        if let Some(view) = self.editor.view.take() {
            unsafe { remove_from_superview(&view) };
        }
        self.editor.factory = None;
        self.editor.controller = None;
        self.invalidate_latency();
    }

    fn is_open(&self) -> bool {
        self.ensure_creator_thread().is_ok() && self.editor.view.is_some()
    }

    fn size(&self) -> Option<(u32, u32)> {
        self.ensure_creator_thread().ok()?;
        let view = self.editor.view.as_ref()?;
        let frame = unsafe { view_frame(view) };
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Some((frame.2.max(0.0) as u32, frame.3.max(0.0) as u32))
    }

    fn is_resizable(&self) -> bool {
        if self.ensure_creator_thread().is_err() {
            return false;
        }
        // AUv2 Cocoa views are typically fixed-size: the factory
        // returns an NSView with a baked layout. Resizing is
        // accepted by setFrame: but most hosts treat AUv2 views as
        // non-resizable. We mirror that.
        false
    }

    fn set_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        self.ensure_creator_thread().ok()?;
        let view = self.editor.view.as_ref()?;
        unsafe { set_view_frame(view, f64::from(width), f64::from(height)) };
        Some((width, height))
    }

    fn show(&mut self) {
        if self.ensure_creator_thread().is_err() {
            return;
        }
        if let Some(view) = self.editor.view.as_ref() {
            unsafe { set_hidden(view, false) };
        }
    }

    fn hide(&mut self) {
        if self.ensure_creator_thread().is_err() {
            return;
        }
        if let Some(view) = self.editor.view.as_ref() {
            unsafe { set_hidden(view, true) };
        }
    }
}

/// Signature of `[AUCocoaUIBase uiViewForAudioUnit:withSize:]`.
/// Used to dispatch directly through the method's raw IMP, side-
/// stepping objc2's encoding check (Apple's AUs use one type
/// encoding for the `AudioUnit` argument, third-party AUs use the
/// older Carbon-era one — same ABI, but objc2 panics on mismatch).
type UiViewFn = unsafe extern "C-unwind" fn(
    *mut objc2::runtime::AnyObject,
    objc2::runtime::Sel,
    *mut std::ffi::c_void,
    objc2_foundation::NSSize,
) -> *mut objc2_app_kit::NSView;

/// Ask an AUv3 for its native view controller and attach its view to the existing
/// host-owned NSView. The controller remains retained for as long as the editor is open.
unsafe fn open_au3_editor(plugin: &mut AuPlugin, parent_ptr: *mut std::ffi::c_void) -> Result<()> {
    // SAFETY: the plug-in owns a live instance, and open() runs on its creator/main thread.
    let controller = unsafe { au3_editor::request_view_controller(plugin.instance)? };
    let view = controller.view();
    // SAFETY: open() validated the non-null host NSView, and all AppKit calls are on
    // the creator/main thread. addSubview: retains the view independently.
    let parent = parent_ptr.cast::<objc2_app_kit::NSView>();
    let _: () = unsafe { objc2::msg_send![parent, addSubview: &*view] };
    plugin.editor.controller = Some(controller);
    plugin.editor.view = Some(view);
    Ok(())
}

/// Fetch the AU's Cocoa view factory descriptor, load the bundle,
/// instantiate the factory, ask it for an `NSView`, and add it as a
/// subview of `parent_ptr`. Stores the live factory + view on
/// `self.editor` so they survive `open` returning.
unsafe fn open_cocoa_editor(
    plugin: &mut AuPlugin,
    parent_ptr: *mut std::ffi::c_void,
) -> Result<()> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_audio_toolbox::{AudioUnitCocoaViewInfo, kAudioUnitProperty_CocoaUI};
    use objc2_core_foundation::{CFRetained, CFString, CFURL};

    let mut size: u32 = 0;
    let info_status = unsafe {
        AudioUnitGetPropertyInfo(
            plugin.instance,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &raw mut size,
            ptr::null_mut(),
        )
    };
    if info_status != 0 || (size as usize) < std::mem::size_of::<AudioUnitCocoaViewInfo>() {
        return Err(Error::Other(format!(
            "kAudioUnitProperty_CocoaUI not advertised (status {info_status}, size {size})"
        )));
    }
    // The CocoaUI struct is variable-length: 1 bundle URL +
    // N class names. We get the static-sized struct here and only
    // use the first class name (mCocoaAUViewClass[0]) — the
    // overwhelming majority of AUs publish exactly one view class.
    let mut storage = vec![0u8; size as usize];
    let mut io_size = size;
    let get_status = unsafe {
        AudioUnitGetProperty(
            plugin.instance,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            ptr::NonNull::new_unchecked(storage.as_mut_ptr().cast()),
            ptr::NonNull::new_unchecked(&raw mut io_size),
        )
    };
    if get_status != 0 {
        return Err(Error::Other(format!(
            "kAudioUnitProperty_CocoaUI get failed: OSStatus {get_status}"
        )));
    }
    // SAFETY: The first sizeof(AudioUnitCocoaViewInfo) bytes form
    // the descriptor; we've verified size at the property-info
    // step. The pointers inside are +1 references owned by us —
    // we wrap them in CFRetained to release on drop.
    #[allow(clippy::cast_ptr_alignment)]
    let view_info = unsafe { &*storage.as_ptr().cast::<AudioUnitCocoaViewInfo>() };
    let bundle_url = unsafe { CFRetained::<CFURL>::from_raw(view_info.mCocoaAUViewBundleLocation) };
    let class_name = unsafe { CFRetained::<CFString>::from_raw(view_info.mCocoaAUViewClass[0]) };

    // CFURL ↔ NSURL and CFString ↔ NSString are toll-free
    // bridged, but objc2's msg_send! checks the type encoding —
    // raw CFType pointers register as struct pointers, not '@'.
    // Cast through the matching NSObject type so the encoding
    // matches `id` (and any future runtime type checks pass).
    // SAFETY: toll-free bridging guarantees identical layout.
    let bundle_url_ns: &objc2_foundation::NSURL = unsafe {
        &*CFRetained::as_ptr(&bundle_url)
            .as_ptr()
            .cast::<objc2_foundation::NSURL>()
    };
    let class_name_ns: &objc2_foundation::NSString = unsafe {
        &*CFRetained::as_ptr(&class_name)
            .as_ptr()
            .cast::<objc2_foundation::NSString>()
    };

    // [NSBundle bundleWithURL:url] → NSBundle*
    let ns_bundle_class = objc2::class!(NSBundle);
    let bundle: *mut AnyObject =
        unsafe { msg_send![ns_bundle_class, bundleWithURL: bundle_url_ns] };
    if bundle.is_null() {
        return Err(Error::Other("NSBundle bundleWithURL: returned nil".into()));
    }
    // [bundle classNamed:name] → Class
    let view_factory_class: *mut objc2::runtime::AnyClass =
        unsafe { msg_send![bundle, classNamed: class_name_ns] };
    if view_factory_class.is_null() {
        return Err(Error::Other(format!(
            "view factory class not found in bundle: {}",
            unsafe { (*CFRetained::as_ptr(&class_name).as_ptr()).to_string() }
        )));
    }
    // [[class alloc] init]
    let factory_alloc: *mut AnyObject = unsafe { msg_send![view_factory_class, alloc] };
    let factory: *mut AnyObject = unsafe { msg_send![factory_alloc, init] };
    if factory.is_null() {
        return Err(Error::Other("AU view factory init returned nil".into()));
    }
    let factory_retained = unsafe { objc2::rc::Retained::from_raw(factory) }
        .ok_or_else(|| Error::Other("could not retain AU view factory".into()))?;

    // Default preferred size — most factories ignore this and pick
    // their own.
    let pref = objc2_foundation::NSSize::new(0.0, 0.0);
    // `uiViewForAudioUnit:withSize:` was historically declared with
    // the Carbon-era `^{ComponentInstanceRecord=[1q]}` encoding;
    // Apple's own AU factories migrated to the newer
    // `^{OpaqueAudioComponentInstance=}` form. objc2's msg_send!
    // panics on encoding mismatch. Skip the check by dispatching
    // through the method's raw IMP directly — same ABI, no encoding
    // verification.
    let sel = objc2::sel!(uiViewForAudioUnit:withSize:);
    let factory_class = (*factory_retained).class();
    let Some(method) = factory_class.instance_method(sel) else {
        return Err(Error::Other(
            "AU view factory has no uiViewForAudioUnit:withSize: method".into(),
        ));
    };
    let imp = method.implementation();
    // SAFETY: AUv2 view factories all expose this exact signature;
    // we matched the selector above before transmuting the IMP.
    let typed: UiViewFn = unsafe { std::mem::transmute(imp) };
    let factory_obj: *mut objc2::runtime::AnyObject =
        objc2::rc::Retained::as_ptr(&factory_retained).cast_mut();
    let view_raw: *mut objc2_app_kit::NSView =
        unsafe { typed(factory_obj, sel, plugin.instance.cast(), pref) };
    if view_raw.is_null() {
        return Err(Error::Other(
            "uiViewForAudioUnit:withSize: returned nil".into(),
        ));
    }
    // The returned view is autoreleased; retain it.
    let view = unsafe { objc2::rc::Retained::retain(view_raw) }
        .ok_or_else(|| Error::Other("could not retain AU NSView".into()))?;

    // [parent addSubview:view]
    let parent_view: *mut objc2_app_kit::NSView = parent_ptr.cast();
    let _: () = unsafe { msg_send![parent_view, addSubview: &*view] };

    plugin.editor.factory = Some(factory_retained);
    plugin.editor.view = Some(view);
    Ok(())
}

unsafe fn remove_from_superview(view: &objc2_app_kit::NSView) {
    use objc2::msg_send;
    let _: () = unsafe { msg_send![view, removeFromSuperview] };
}

unsafe fn set_hidden(view: &objc2_app_kit::NSView, hidden: bool) {
    use objc2::msg_send;
    let flag: objc2::runtime::Bool = objc2::runtime::Bool::new(hidden);
    let _: () = unsafe { msg_send![view, setHidden: flag] };
}

unsafe fn view_frame(view: &objc2_app_kit::NSView) -> (f64, f64, f64, f64) {
    use objc2::msg_send;
    use objc2_foundation::NSRect;

    // Force any pending Auto Layout pass so the descendant frames
    // we're about to walk are post-layout. Apple's stock AUv2 views
    // (AULowpass, AUMultibandCompressor, …) only finalize their
    // child-view positions on the first layout pass.
    let _: () = unsafe { msg_send![view, layoutSubtreeIfNeeded] };

    let r: NSRect = unsafe { msg_send![view, frame] };

    // AUGenericView and friends report a `frame` that's smaller
    // than the union of their descendant subviews — macOS doesn't
    // clip children to a parent's bounds by default, so the visible
    // UI extends past the parent's frame. Walk the subtree and use
    // the bounding box of every descendant frame as the real
    // preferred size.
    let (extent_w, extent_h) = unsafe { subtree_extent(view) };
    let w = extent_w.max(r.size.width);
    let h = extent_h.max(r.size.height);
    (r.origin.x, r.origin.y, w, h)
}

/// Recursively walk `view`'s subviews, computing the bounding box
/// (in `view`'s coordinate system) of every descendant's frame.
/// Returns `(max_x, max_y)` — i.e. the max width/height the view
/// would need so no descendant overflows.
unsafe fn subtree_extent(view: &objc2_app_kit::NSView) -> (f64, f64) {
    use objc2::msg_send;
    use objc2_foundation::{NSArray, NSRect};

    let subviews: *mut NSArray<objc2_app_kit::NSView> = unsafe { msg_send![view, subviews] };
    if subviews.is_null() {
        return (0.0, 0.0);
    }
    let count: usize = unsafe { msg_send![subviews, count] };
    let mut max_x = 0.0_f64;
    let mut max_y = 0.0_f64;
    for i in 0..count {
        let sub: *mut objc2_app_kit::NSView = unsafe { msg_send![subviews, objectAtIndex: i] };
        if sub.is_null() {
            continue;
        }
        let frame: NSRect = unsafe { msg_send![sub, frame] };
        max_x = max_x.max(frame.origin.x + frame.size.width);
        max_y = max_y.max(frame.origin.y + frame.size.height);
        // Recurse into the descendant, translating its extent into
        // `view`'s coordinate space.
        let (sub_w, sub_h) = unsafe { subtree_extent(&*sub) };
        max_x = max_x.max(frame.origin.x + sub_w);
        max_y = max_y.max(frame.origin.y + sub_h);
    }
    (max_x, max_y)
}

unsafe fn set_view_frame(view: &objc2_app_kit::NSView, w: f64, h: f64) {
    use objc2::msg_send;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let r = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h));
    let _: () = unsafe { msg_send![view, setFrame: r] };
}

fn process_buffer_matches_topology(
    buffer: &mut AudioBuffer<'_, f32>,
    input_channels: &[u32],
    output_channels: &[u32],
    max_block_size: usize,
) -> bool {
    let frames = buffer.num_frames();
    if frames > max_block_size
        || buffer.num_input_buses() != input_channels.len()
        || buffer.num_output_buses() != output_channels.len()
    {
        return false;
    }
    for (index, &expected) in input_channels.iter().enumerate() {
        let channels = buffer.bus_inputs(index);
        if usize::try_from(expected).ok() != Some(channels.len())
            || channels.iter().any(|plane| plane.len() < frames)
        {
            return false;
        }
    }
    for (index, &expected) in output_channels.iter().enumerate() {
        let channels = buffer.bus_outputs(index);
        if usize::try_from(expected).ok() != Some(channels.len())
            || channels.iter().any(|plane| plane.len() < frames)
        {
            return false;
        }
    }
    true
}

fn refresh_render_inputs(
    render_ctx: Option<&mut RenderContext>,
    buffer: &AudioBuffer<'_, f32>,
) -> bool {
    let Some(ctx) = render_ctx else {
        return buffer.num_input_buses() == 0;
    };
    ctx.input_planes.fill(ptr::null());
    for (bus_index, range) in ctx.input_bus_ranges.iter().copied().enumerate() {
        for (channel_index, channel) in buffer.bus_inputs(bus_index).iter().enumerate() {
            let Some(plane_index) = range.start.checked_add(channel_index) else {
                return false;
            };
            let Some(slot) = ctx.input_planes.get_mut(plane_index) else {
                return false;
            };
            *slot = channel.as_ptr();
        }
    }
    true
}

fn audio_timestamp(sample_time: f64) -> AudioTimeStamp {
    AudioTimeStamp {
        mSampleTime: sample_time,
        mHostTime: 0,
        mRateScalar: 1.0,
        mWordClockTime: 0,
        mSMPTETime: objc2_core_audio_types::SMPTETime {
            mSubframes: 0,
            mSubframeDivisor: 0,
            mCounter: 0,
            mType: objc2_core_audio_types::SMPTETimeType(0),
            mFlags: objc2_core_audio_types::SMPTETimeFlags(0),
            mHours: 0,
            mMinutes: 0,
            mSeconds: 0,
            mFrames: 0,
        },
        mFlags: AudioTimeStampFlags::SampleTimeValid,
        mReserved: 0,
    }
}

/// Render every output element into its matching checked host planes.
///
/// # Safety
/// The AU must be initialized and exclusively rendered. `buffer` has already been verified to
/// contain at least `frames` samples in every output plane.
unsafe fn render_output_buses(
    instance: AudioComponentInstance,
    output_channels: &[u32],
    storage: &mut [Vec<u8>],
    buffer: &mut AudioBuffer<'_, f32>,
    timestamp: &AudioTimeStamp,
    frames: u32,
    frame_bytes: u32,
) -> bool {
    if storage.len() != output_channels.len() {
        return false;
    }
    for (bus_index, audio_list) in storage.iter_mut().enumerate() {
        let Ok(output_bus_number) = u32::try_from(bus_index) else {
            return false;
        };
        let Ok(channel_count) = usize::try_from(output_channels[bus_index]) else {
            return false;
        };
        let bus_outputs = buffer.bus_outputs(bus_index);
        // SAFETY: the list is pre-sized for this bus, and each host plane passed validation.
        #[allow(clippy::cast_ptr_alignment)]
        let list_ptr = audio_list.as_mut_ptr().cast::<AudioBufferList>();
        unsafe {
            (*list_ptr).mNumberBuffers = output_channels[bus_index];
            let buffers_start: *mut CAAudioBuffer = (*list_ptr).mBuffers.as_mut_ptr();
            for channel_index in 0..channel_count {
                let output_buffer = &mut *buffers_start.add(channel_index);
                output_buffer.mNumberChannels = 1;
                output_buffer.mDataByteSize = frame_bytes;
                output_buffer.mData = bus_outputs[channel_index].as_mut_ptr().cast();
            }
        }
        let mut flags = AudioUnitRenderActionFlags::empty();
        let status = unsafe {
            AudioUnitRender(
                instance,
                &raw mut flags,
                ptr::NonNull::from(timestamp),
                output_bus_number,
                frames,
                ptr::NonNull::new_unchecked(list_ptr),
            )
        };
        if status != 0 {
            return false;
        }
    }
    true
}

impl Plugin<f32> for AuPlugin {
    fn process(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        events: &EventList,
        context: &mut ProcessContext<'_>,
    ) -> Result<ProcessStatus> {
        if !self.is_active() {
            return Err(Error::NotActivated);
        }
        if !process_buffer_matches_topology(
            buffer,
            &self.input_bus_channels,
            &self.output_bus_channels,
            self.max_block_size,
        ) {
            return Ok(ProcessStatus::Error);
        }
        let frames = buffer.num_frames();
        let Ok(frames_u32) = u32::try_from(frames) else {
            return Ok(ProcessStatus::Error);
        };
        let Some(frame_bytes) = frames
            .checked_mul(std::mem::size_of::<f32>())
            .and_then(|bytes| u32::try_from(bytes).ok())
        else {
            return Ok(ProcessStatus::Error);
        };
        let sample_time = self.sample_time;
        if let Some(ht) = self.host_transport.as_deref_mut() {
            update_host_transport(ht, context.transport, context.sample_rate, sample_time);
        }
        for event in events {
            send_midi(self.instance, event);
        }
        if !refresh_render_inputs(self.render_ctx.as_deref_mut(), buffer) {
            return Ok(ProcessStatus::Error);
        }
        let timestamp = audio_timestamp(sample_time);
        // SAFETY: activation succeeded, bus topology was checked, and every plane is long enough.
        let rendered = unsafe {
            render_output_buses(
                self.instance,
                &self.output_bus_channels,
                &mut self.output_buffer_storage,
                buffer,
                &timestamp,
                frames_u32,
                frame_bytes,
            )
        };
        #[allow(clippy::cast_precision_loss)]
        let frames_f = frames as f64;
        self.sample_time += frames_f;
        if !rendered {
            return Ok(ProcessStatus::Error);
        }
        Ok(ProcessStatus::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_cc_roundtrip() {
        let aufx = u32::from_be_bytes(*b"aufx");
        assert_eq!(four_cc(aufx), "aufx");
        assert_eq!(parse_four_cc("aufx"), Some(aufx));
    }

    #[test]
    fn unique_id_roundtrip() {
        let desc = AudioComponentDescription {
            componentType: u32::from_be_bytes(*b"aufx"),
            componentSubType: u32::from_be_bytes(*b"dely"),
            componentManufacturer: u32::from_be_bytes(*b"appl"),
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let id = format_unique_id(&desc);
        assert_eq!(id, "aufx:dely:appl");
        let parsed = parse_unique_id(&id).unwrap();
        assert_eq!(parsed.componentType, desc.componentType);
        assert_eq!(parsed.componentSubType, desc.componentSubType);
        assert_eq!(parsed.componentManufacturer, desc.componentManufacturer);
    }

    #[test]
    fn native_bus_layout_preserves_order_names_and_does_not_infer_sidechain() {
        let layout = native_bus_layout(
            &[2, 1],
            &[Some("Program".into()), Some("Key Trigger".into())],
            &[
                Some(kAudioChannelLayoutTag_Stereo),
                Some(kAudioChannelLayoutTag_Mono),
            ],
            &[2, 6],
            &[Some("Main Mix".into()), Some("Diagnostics".into())],
            &[
                Some(kAudioChannelLayoutTag_Stereo),
                Some(kAudioChannelLayoutTag_AudioUnit_5_1),
            ],
        )
        .unwrap();

        assert_eq!(layout.id, None);
        assert_eq!(layout.inputs.len(), 2);
        assert_eq!(layout.inputs[0].name, "Program");
        assert_eq!(layout.inputs[0].kind, BusKind::Main);
        assert_eq!(layout.inputs[0].channels, ChannelConfig::Stereo);
        assert_eq!(layout.inputs[1].name, "Key Trigger");
        assert_eq!(layout.inputs[1].kind, BusKind::Auxiliary);
        assert_eq!(layout.inputs[1].channels, ChannelConfig::Mono);
        assert_eq!(layout.outputs.len(), 2);
        assert_eq!(layout.outputs[0].name, "Main Mix");
        assert_eq!(layout.outputs[0].kind, BusKind::Main);
        assert_eq!(layout.outputs[0].channels, ChannelConfig::Stereo);
        assert_eq!(layout.outputs[1].name, "Diagnostics");
        assert_eq!(layout.outputs[1].kind, BusKind::Auxiliary);
        assert_eq!(layout.outputs[1].channels, ChannelConfig::Surround5_1);
    }

    #[test]
    fn native_bus_layout_allows_no_input_and_rejects_unbounded_or_empty_output() {
        let generator = native_bus_layout(&[], &[], &[], &[2], &[None], &[None]).unwrap();
        assert!(generator.inputs.is_empty());
        assert_eq!(generator.outputs[0].name, "Output");

        assert!(native_bus_layout(&[], &[], &[], &[], &[], &[]).is_err());
        assert!(native_bus_layout(&[], &[], &[], &[0], &[None], &[None]).is_err());
        assert!(native_bus_layout(&[], &[], &[], &[u32::MAX], &[None], &[None]).is_err());
        assert!(native_bus_layout(&[1; 65], &[], &[], &[1], &[None], &[None]).is_err());
        assert!(native_bus_layout(&[], &[], &[], &[1; 65], &[], &[]).is_err());

        let unclassified = native_bus_layout(&[], &[], &[], &[6], &[None], &[None]).unwrap();
        assert_eq!(unclassified.outputs[0].channels, ChannelConfig::Discrete(6));
    }

    #[test]
    fn input_plane_ranges_route_native_buses_without_crossing_channels() {
        let ranges = bus_plane_ranges(&[2, 1, 6]).unwrap();
        assert_eq!(
            ranges,
            [
                BusPlaneRange { start: 0, len: 2 },
                BusPlaneRange { start: 2, len: 1 },
                BusPlaneRange { start: 3, len: 6 },
            ]
        );
        assert_eq!(bus_plane_index(&ranges, 0, 0), Some(0));
        assert_eq!(bus_plane_index(&ranges, 0, 1), Some(1));
        assert_eq!(bus_plane_index(&ranges, 1, 0), Some(2));
        assert_eq!(bus_plane_index(&ranges, 2, 5), Some(8));
        assert_eq!(bus_plane_index(&ranges, 0, 2), None);
        assert_eq!(bus_plane_index(&ranges, 3, 0), None);
    }

    #[test]
    fn planar_f32_negotiation_requires_exact_channels_rate_and_packing() {
        let expected = planar_f32_format(48_000.0, 6);
        assert!(render_format_matches(&expected, &expected));

        let mut wrong_channels = expected;
        wrong_channels.mChannelsPerFrame = 2;
        assert!(!render_format_matches(&wrong_channels, &expected));

        let mut wrong_rate = expected;
        wrong_rate.mSampleRate = 44_100.0;
        assert!(!render_format_matches(&wrong_rate, &expected));

        let mut interleaved = expected;
        interleaved.mFormatFlags &= !kAudioFormatFlagIsNonInterleaved;
        assert!(!render_format_matches(&interleaved, &expected));
    }

    #[test]
    fn standard_channel_configs_require_matching_native_layout_tags() {
        assert_eq!(
            channel_config(1, Some(kAudioChannelLayoutTag_Mono)),
            ChannelConfig::Mono
        );
        assert_eq!(
            channel_config(2, Some(kAudioChannelLayoutTag_Stereo)),
            ChannelConfig::Stereo
        );
        assert_eq!(
            channel_config(6, Some(kAudioChannelLayoutTag_AudioUnit_5_1)),
            ChannelConfig::Surround5_1
        );
        assert_eq!(
            channel_config(8, Some(kAudioChannelLayoutTag_AudioUnit_7_1)),
            ChannelConfig::Surround7_1
        );
        assert_eq!(channel_config(2, None), ChannelConfig::Discrete(2));
        assert_eq!(
            channel_config(6, Some(kAudioChannelLayoutTag_Stereo)),
            ChannelConfig::Discrete(6)
        );
        assert!(selected_channel_config_matches(
            ChannelConfig::Discrete(6),
            6,
            Some(kAudioChannelLayoutTag_AudioUnit_5_1)
        ));
        assert!(!selected_channel_config_matches(
            ChannelConfig::Surround5_1,
            6,
            None
        ));
    }

    #[test]
    fn latency_seconds_round_up_to_frames_and_reject_invalid_values() {
        assert_eq!(latency_frames_from_seconds(0.0, 48_000.0), Some(0));
        assert_eq!(latency_frames_from_seconds(0.001, 48_000.0), Some(48));
        assert_eq!(latency_frames_from_seconds(0.000_001, 48_000.0), Some(1));
        for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            for frames in 1..=4_096_u32 {
                let seconds = f64::from(frames) / sample_rate;
                assert_eq!(
                    latency_frames_from_seconds(seconds, sample_rate),
                    Some(frames),
                    "{frames} frames at {sample_rate} Hz"
                );
            }
        }
        assert_eq!(
            latency_frames_from_seconds(1.01 / 48_000.0, 48_000.0),
            Some(2)
        );
        assert_eq!(
            latency_frames_from_seconds(1.5 / 48_000.0, 48_000.0),
            Some(2)
        );
        assert_eq!(latency_frames_from_seconds(f64::NAN, 48_000.0), None);
        assert_eq!(latency_frames_from_seconds(f64::INFINITY, 48_000.0), None);
        assert_eq!(latency_frames_from_seconds(-0.001, 48_000.0), None);
        assert_eq!(latency_frames_from_seconds(0.001, 0.0), None);
        assert_eq!(
            latency_frames_from_seconds(f64::from(u32::MAX) / 48_000.0, 48_000.0),
            Some(u32::MAX)
        );
        assert_eq!(
            latency_frames_from_seconds((f64::from(u32::MAX) + 1.0) / 48_000.0, 48_000.0),
            None
        );
        assert_eq!(
            latency_frames_from_seconds(1.0, f64::from(u32::MAX) + 1.0),
            None
        );
    }

    #[test]
    fn latency_property_notification_only_invalidates_cached_snapshot() {
        let signal = acquire_latency_signal().unwrap();
        assert_eq!(latency_snapshot(Some(0), Some(&signal)), Some(0));
        assert_eq!(latency_snapshot(Some(0), None), None);

        // The callback receives only the opaque token and updates its validated static slot.
        unsafe {
            latency_property_changed(
                std::ptr::NonNull::new(signal.user_data()).unwrap(),
                ptr::null_mut(),
                kAudioUnitProperty_Latency,
                kAudioUnitScope_Global,
                0,
            );
        }
        assert_eq!(latency_snapshot(Some(64), Some(&signal)), None);
        assert!(take_latency_invalidation(Some(&signal)));
        assert!(!take_latency_invalidation(Some(&signal)));
        // The control service can publish its refreshed known zero without confusing it with
        // unknown; query failures remain None through the same snapshot boundary.
        assert_eq!(latency_snapshot(Some(0), Some(&signal)), Some(0));
        assert_eq!(latency_snapshot(None, Some(&signal)), None);
    }

    #[test]
    fn failed_latency_changing_mutation_stays_unknown_without_notification() {
        let signal = acquire_latency_signal().unwrap();
        let mut cached = Some(64);
        let result = with_latency_invalidation(&mut cached, Some(&signal), || {
            // Invalidation precedes the native operation, even if it fails without notifying.
            assert!(signal.is_dirty());
            Err::<(), _>(-50)
        });

        assert_eq!(result, Err(-50));
        assert_eq!(cached, None);
        assert_eq!(latency_snapshot(cached, Some(&signal)), None);
    }

    #[test]
    fn creator_thread_guard_rejects_control_access_from_another_thread() {
        let creator = std::thread::current().id();
        assert!(check_creator_thread(&creator).is_ok());

        let error =
            std::thread::spawn(move || check_creator_thread(&creator).unwrap_err().to_string())
                .join()
                .unwrap();
        assert_eq!(
            error,
            "Audio Unit control API called from a non-creator thread"
        );
    }

    #[test]
    fn async_completion_transfers_success_and_failure_ownership_once() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let success = AsyncInstantiationCompletion::default();
        assert_eq!(success.complete(0x1234, 0, deadline), None);
        assert_eq!(success.take_completed(), Some((0x1234, 0)));
        assert_eq!(success.take_completed(), None);
        assert_eq!(success.abandon(), None);

        let failure = AsyncInstantiationCompletion::default();
        assert_eq!(failure.complete(0x5678, -50, deadline), None);
        assert_eq!(failure.take_completed(), Some((0x5678, -50)));
        // The caller owns this failed non-null instance and disposes it exactly once.
        assert_eq!(failure.abandon(), None);
    }

    #[test]
    fn async_completion_timeout_and_callback_race_have_one_disposal_owner() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let timeout_wins = AsyncInstantiationCompletion::default();
        assert_eq!(timeout_wins.abandon(), None);
        assert_eq!(timeout_wins.complete(0x1234, 0, deadline), Some(0x1234));
        // Re-delivery of the same completion token does not hand the same unit out twice.
        assert_eq!(timeout_wins.complete(0x1234, 0, deadline), None);

        let callback_wins = AsyncInstantiationCompletion::default();
        assert_eq!(callback_wins.complete(0x5678, 0, deadline), None);
        // If completion wins the mutex race just before timeout, timeout takes and disposes
        // the result rather than leaving it owned by a callback that has already returned.
        assert_eq!(callback_wins.abandon(), Some(0x5678));
        assert_eq!(callback_wins.complete(0x5678, 0, deadline), None);

        let expired = AsyncInstantiationCompletion::default();
        let already_expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(expired.complete(0x9abc, 0, already_expired), Some(0x9abc));
        assert_eq!(expired.take_completed(), None);
        assert_eq!(expired.complete(0x9abc, 0, already_expired), None);
    }
}
