//! Format adapters built on the shared `truce-rack` host contract.
//!
//! Discovery and probing run only in disposable child processes. A third-party binary can
//! therefore crash or hang without taking down the graph/control process. DSP instances run in
//! the same bounded shared-memory worker model for every supported format.

use std::{
    mem::ManuallyDrop,
    path::{Path, PathBuf},
};

use arrayvec::ArrayVec;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tonenari_control_protocol::{
    ExternalProcessorDescriptor, ExternalProcessorFormat, ProcessorParameterDescriptor,
};
use truce_rack_core::{
    Plugin, WindowHandle,
    buffer::{AudioBuffer, BusRange},
    bus::BusLayout,
    events::EventList,
    info::{ParameterFlags, PluginCategory, PluginInfo},
    plugin::{ProcessContext, ProcessStatus},
    scanner::PluginScanner,
};

#[cfg(test)]
mod channel_tests;

pub const SCAN_ARGUMENT: &str = "--tonenari-plugin-scan";
pub const PROBE_ARGUMENT: &str = "--tonenari-plugin-probe";
pub const WORKER_ARGUMENT: &str = "--tonenari-plugin-worker";

const MAX_CHANNELS: u16 = 64;
const CHANNEL_SLOTS: usize = MAX_CHANNELS as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackFormat {
    Au,
    Auv3,
    Clap,
    Lv2,
    Vst3,
}

impl RackFormat {
    #[must_use]
    pub const fn argument(self) -> &'static str {
        match self {
            Self::Au => "au",
            Self::Auv3 => "auv3",
            Self::Clap => "clap",
            Self::Lv2 => "lv2",
            Self::Vst3 => "vst3",
        }
    }

    /// Parses the stable command-line representation used by scanner children.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested format is unknown to this host build.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "au" => Ok(Self::Au),
            "auv3" => Ok(Self::Auv3),
            "clap" => Ok(Self::Clap),
            "lv2" => Ok(Self::Lv2),
            "vst3" => Ok(Self::Vst3),
            _ => Err(format!("unsupported processor format: {value}")),
        }
    }

    pub(crate) const fn protocol(self) -> ExternalProcessorFormat {
        match self {
            Self::Au => ExternalProcessorFormat::Au,
            Self::Auv3 => ExternalProcessorFormat::Auv3,
            Self::Clap => ExternalProcessorFormat::Clap,
            Self::Lv2 => ExternalProcessorFormat::Lv2,
            Self::Vst3 => ExternalProcessorFormat::Vst3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RackCategory {
    Effect,
    Instrument,
    NoteEffect,
    Analyzer,
    Tool,
}

impl From<PluginCategory> for RackCategory {
    fn from(value: PluginCategory) -> Self {
        match value {
            PluginCategory::Effect => Self::Effect,
            PluginCategory::Instrument => Self::Instrument,
            PluginCategory::NoteEffect => Self::NoteEffect,
            PluginCategory::Analyzer => Self::Analyzer,
            PluginCategory::Tool => Self::Tool,
        }
    }
}

impl From<RackCategory> for PluginCategory {
    fn from(value: RackCategory) -> Self {
        match value {
            RackCategory::Effect => Self::Effect,
            RackCategory::Instrument => Self::Instrument,
            RackCategory::NoteEffect => Self::NoteEffect,
            RackCategory::Analyzer => Self::Analyzer,
            RackCategory::Tool => Self::Tool,
        }
    }
}

/// Serializable identity passed between the scanner, probe, and DSP worker processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RackLocator {
    pub format: RackFormat,
    pub unique_id: String,
    pub path: PathBuf,
    name: String,
    vendor: String,
    version: u32,
    category: RackCategory,
    has_editor: bool,
    accepts_midi: bool,
}

impl RackLocator {
    fn from_info(format: RackFormat, info: PluginInfo) -> Self {
        Self {
            format,
            unique_id: info.unique_id,
            path: info.path,
            name: info.name,
            vendor: info.vendor,
            version: info.version,
            category: info.category.into(),
            has_editor: info.has_editor,
            accepts_midi: info.accepts_midi,
        }
    }

    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.name.clone(),
            vendor: self.vendor.clone(),
            version: self.version,
            category: self.category.into(),
            path: self.path.clone(),
            unique_id: self.unique_id.clone(),
            format: self.format.argument(),
            has_editor: self.has_editor,
            accepts_midi: self.accepts_midi,
        }
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.name
    }

    fn is_audio_processor(&self) -> bool {
        self.category == RackCategory::Effect
            && match self.format {
                // Audio Unit generators and instruments can expose nominal stereo buses even
                // though they are not insert effects. Restrict the processor menu to the native
                // effect and music-effect component families.
                RackFormat::Au | RackFormat::Auv3 => {
                    self.unique_id.starts_with("aufx:") || self.unique_id.starts_with("aumf:")
                }
                RackFormat::Clap | RackFormat::Lv2 | RackFormat::Vst3 => true,
            }
    }

    /// Serializes this identity for a worker command-line argument.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity cannot be serialized.
    pub fn encode(&self) -> Result<String, String> {
        serde_json::to_vec(self)
            .map(|bytes| BASE64.encode(bytes))
            .map_err(|error| error.to_string())
    }

    /// Restores a worker identity from its command-line representation.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed Base64 or invalid serialized identity data.
    pub fn decode(encoded: &str) -> Result<Self, String> {
        let bytes = BASE64.decode(encoded).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RackProbeRecord {
    pub descriptor: ExternalProcessorDescriptor,
    pub locator: RackLocator,
    pub layout_index: usize,
}

/// Formats available in the current target build.
#[must_use]
pub(crate) fn supported_formats() -> Vec<RackFormat> {
    #[cfg(target_vendor = "apple")]
    let formats = vec![
        RackFormat::Clap,
        RackFormat::Vst3,
        RackFormat::Au,
        RackFormat::Auv3,
    ];
    #[cfg(target_os = "linux")]
    let formats = vec![RackFormat::Clap, RackFormat::Vst3, RackFormat::Lv2];
    #[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
    let formats = vec![RackFormat::Clap, RackFormat::Vst3];
    formats
}

/// Runs one format scanner in a child process and prints serializable plugin identities.
///
/// # Errors
///
/// Returns an error when discovery fails or its result cannot be serialized.
pub fn run_scan(format: RackFormat, custom_paths: &[PathBuf]) -> Result<(), String> {
    let infos = match format {
        RackFormat::Clap => scan_with_paths(&truce_rack_clap::ClapScanner::new(), custom_paths)?,
        RackFormat::Vst3 => scan_with_paths(&truce_rack_vst3::Vst3Scanner::new(), custom_paths)?,
        #[cfg(target_vendor = "apple")]
        RackFormat::Au => truce_rack_au::AuScanner::new()
            .scan()
            .map_err(|error| error.to_string())?,
        #[cfg(target_vendor = "apple")]
        RackFormat::Auv3 => truce_rack_au3::Au3Scanner::new()
            .scan()
            .map_err(|error| error.to_string())?,
        #[cfg(target_os = "linux")]
        RackFormat::Lv2 => truce_rack_lv2::Lv2Scanner::new()
            .scan()
            .map_err(|error| error.to_string())?,
        #[allow(unreachable_patterns)]
        _ => {
            return Err(format!(
                "{} is unavailable on this platform",
                format.argument()
            ));
        }
    };
    let records = infos
        .into_iter()
        .map(|info| RackLocator::from_info(format, info))
        .filter(RackLocator::is_audio_processor)
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string(&records).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn scan_with_paths<S: PluginScanner>(
    scanner: &S,
    custom_paths: &[PathBuf],
) -> Result<Vec<PluginInfo>, String> {
    let mut infos = scanner.scan().map_err(|error| error.to_string())?;
    for path in custom_paths {
        infos.extend(scanner.scan_path(path).map_err(|error| error.to_string())?);
    }
    Ok(infos)
}

/// Loads a single candidate in an isolated process and emits its usable effect layout.
///
/// # Errors
///
/// Returns an error when the processor cannot load, activate, or expose a supported layout.
pub fn run_probe(locator: RackLocator) -> Result<(), String> {
    let record = probe(locator)?;
    println!(
        "{}",
        serde_json::to_string(&record).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn probe(locator: RackLocator) -> Result<RackProbeRecord, String> {
    match locator.format {
        RackFormat::Clap => probe_with_scanner(&truce_rack_clap::ClapScanner::new(), locator),
        RackFormat::Vst3 => probe_with_scanner(&truce_rack_vst3::Vst3Scanner::new(), locator),
        #[cfg(target_vendor = "apple")]
        RackFormat::Au => probe_with_scanner(&truce_rack_au::AuScanner::new(), locator),
        #[cfg(target_vendor = "apple")]
        RackFormat::Auv3 => probe_with_scanner(&truce_rack_au3::Au3Scanner::new(), locator),
        #[cfg(target_os = "linux")]
        RackFormat::Lv2 => probe_with_scanner(&truce_rack_lv2::Lv2Scanner::new(), locator),
        #[allow(unreachable_patterns)]
        _ => Err(format!(
            "{} is unavailable on this platform",
            locator.format.argument()
        )),
    }
}

fn probe_with_scanner<S>(scanner: &S, locator: RackLocator) -> Result<RackProbeRecord, String>
where
    S: PluginScanner,
    S::Plugin: Plugin<f32> + 'static,
{
    let mut plugin = scanner
        .load(&locator.info())
        .map_err(|error| error.to_string())?;
    describe_loaded_plugin(locator, &mut plugin)
}

fn describe_loaded_plugin<P: Plugin<f32>>(
    mut locator: RackLocator,
    plugin: &mut P,
) -> Result<RackProbeRecord, String> {
    let (layout_index, layout) = select_effect_layout(plugin.supported_layouts())
        .ok_or_else(|| "plugin exposes no supported audio effect layout".to_owned())?;
    let layout = layout.clone();
    // A successful scan only proves that the binary can be opened. Activate the selected layout
    // in the disposable probe process so an incompatible plugin never reaches the live catalog.
    plugin
        .activate(layout.clone(), 48_000.0, 128)
        .map_err(|error| error.to_string())?;
    // Some formats (notably Audio Unit and CLAP) can only report native-editor support after
    // loading an instance. The scan locator is therefore a hint; the disposable probe is the
    // authoritative capability source exposed to both the UI catalog and the live worker.
    locator.has_editor = plugin.info().has_editor;
    let result = (|| {
        let parameters = (0..plugin.parameter_count())
            .filter_map(|index| plugin.parameter_info(index).ok())
            .filter(|parameter| !parameter.flags.contains(ParameterFlags::HIDDEN))
            .map(|parameter| ProcessorParameterDescriptor {
                id: parameter.id,
                name: parameter.name,
                default_value: normalize_parameter(parameter.default, parameter.min, parameter.max),
                step_count: i32::try_from(parameter.step_count).unwrap_or(i32::MAX),
                can_automate: parameter.flags.contains(ParameterFlags::AUTOMATABLE),
                is_read_only: parameter.flags.contains(ParameterFlags::READ_ONLY),
            })
            .collect();
        let (input_channels, output_channels) = layout_channels(&layout)?;
        Ok(RackProbeRecord {
            descriptor: ExternalProcessorDescriptor {
                type_id: format!("{}:{}", locator.format.argument(), locator.unique_id),
                display_name: locator.name.clone(),
                vendor: locator.vendor.clone(),
                version: locator.version.to_string(),
                format: locator.format.protocol(),
                input_channels,
                output_channels,
                has_editor: locator.has_editor,
                parameters,
            },
            locator,
            layout_index,
        })
    })();
    plugin.deactivate();
    result
}

fn checked_channels(channels: u32) -> Result<u16, String> {
    u16::try_from(channels)
        .ok()
        .filter(|channels| (1..=MAX_CHANNELS).contains(channels))
        .ok_or_else(|| format!("plugin effect layout requires 1..={MAX_CHANNELS} channels"))
}

fn layout_channels(layout: &BusLayout) -> Result<(u16, u16), String> {
    fn total(buses: &[truce_rack_core::bus::Bus]) -> Result<u16, String> {
        let channels = buses
            .iter()
            .try_fold(0_u32, |total, bus| total.checked_add(bus.channels.count()));
        checked_channels(channels.unwrap_or(u32::MAX))
    }
    Ok((total(&layout.inputs)?, total(&layout.outputs)?))
}

fn select_effect_layout(layouts: &[BusLayout]) -> Option<(usize, &BusLayout)> {
    layouts
        .iter()
        .enumerate()
        .filter(|(_, layout)| layout_channels(layout).is_ok())
        .min_by_key(|(_, layout)| {
            let inputs = layout.total_input_channels();
            let outputs = layout.total_output_channels();
            let stereo_penalty = u32::from(inputs != 2 || outputs != 2);
            let asymmetric_penalty = u32::from(inputs != outputs);
            (stereo_penalty, asymmetric_penalty, inputs + outputs)
        })
}

fn normalize_parameter(value: f64, minimum: f64, maximum: f64) -> f64 {
    if !value.is_finite() || !minimum.is_finite() || !maximum.is_finite() || maximum <= minimum {
        return 0.0;
    }
    ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
}

/// Format-erased plugin instance owned exclusively by one DSP worker process.
pub(crate) struct RackWorkerPlugin {
    plugin: Box<dyn Plugin<f32>>,
    editor_allowed: bool,
    input_storage: Vec<Vec<f32>>,
    output_storage: Vec<Vec<f32>>,
    input_ranges: Vec<BusRange>,
    output_ranges: Vec<BusRange>,
    parameter_ranges: Vec<ParameterRange>,
    input_events: EventList,
    output_events: EventList,
    sample_rate: f64,
    max_block_size: usize,
}

#[derive(Debug, Clone, Copy)]
struct ParameterRange {
    id: u32,
    index: usize,
    minimum: f64,
    maximum: f64,
}

struct WorkerActivation {
    editor_allowed: bool,
    sample_rate: f64,
    max_block_size: usize,
}

impl RackWorkerPlugin {
    pub(crate) fn load(
        locator: &RackLocator,
        layout_index: usize,
        sample_rate: f64,
        max_block_size: usize,
        parameters: &serde_json::Value,
    ) -> Result<Self, String> {
        Self::activate(
            load_plugin(locator)?,
            layout_index,
            &WorkerActivation {
                editor_allowed: locator.has_editor,
                sample_rate,
                max_block_size,
            },
            parameters,
        )
    }

    fn activate(
        mut plugin: Box<dyn Plugin<f32>>,
        layout_index: usize,
        activation: &WorkerActivation,
        parameters: &serde_json::Value,
    ) -> Result<Self, String> {
        let layout = plugin
            .supported_layouts()
            .get(layout_index)
            .cloned()
            .ok_or_else(|| "processor layout is no longer available".to_owned())?;
        // A saved index identifies a position, not an immutable bus layout. Revalidate the
        // freshly loaded binary before activation or allocating its planar channels.
        let (input_channels, output_channels) = layout_channels(&layout)?;
        let input_ranges = bus_ranges(&layout.inputs);
        let output_ranges = bus_ranges(&layout.outputs);
        let parameter_ranges = (0..plugin.parameter_count())
            .filter_map(|index| {
                plugin
                    .parameter_info(index)
                    .ok()
                    .map(|parameter| ParameterRange {
                        id: parameter.id,
                        index,
                        minimum: parameter.min,
                        maximum: parameter.max,
                    })
            })
            .collect::<Vec<_>>();
        plugin
            .activate(layout, activation.sample_rate, activation.max_block_size)
            .map_err(|error| error.to_string())?;
        let mut worker = Self {
            plugin,
            editor_allowed: activation.editor_allowed,
            input_storage: planar_storage(usize::from(input_channels), activation.max_block_size),
            output_storage: planar_storage(usize::from(output_channels), activation.max_block_size),
            input_ranges,
            output_ranges,
            parameter_ranges,
            input_events: EventList::new(),
            output_events: EventList::new(),
            sample_rate: activation.sample_rate,
            max_block_size: activation.max_block_size,
        };
        worker.apply_initial_state(parameters)?;
        Ok(worker)
    }

    pub(crate) fn input_channels(&self) -> usize {
        self.input_storage.len()
    }

    pub(crate) fn output_channels(&self) -> usize {
        self.output_storage.len()
    }

    pub(crate) fn inputs_mut(&mut self) -> &mut [Vec<f32>] {
        &mut self.input_storage
    }

    pub(crate) fn outputs(&self) -> &[Vec<f32>] {
        &self.output_storage
    }

    pub(crate) fn clear_outputs(&mut self) {
        for channel in &mut self.output_storage {
            channel.fill(0.0);
        }
    }

    pub(crate) fn bypass(&mut self, frames: usize) {
        for (channel_index, output) in self.output_storage.iter_mut().enumerate() {
            if let Some(input) = self.input_storage.get(channel_index) {
                output[..frames].copy_from_slice(&input[..frames]);
            } else {
                output[..frames].fill(0.0);
            }
        }
    }

    pub(crate) fn set_parameter(&mut self, id: u32, normalized: f64) -> Result<(), String> {
        let range = self
            .parameter_ranges
            .iter()
            .find(|range| range.id == id)
            .copied()
            .ok_or_else(|| format!("processor parameter {id} is unavailable"))?;
        let value = range.minimum + normalized.clamp(0.0, 1.0) * (range.maximum - range.minimum);
        self.plugin
            .set_parameter(range.index, value)
            .map_err(|error| error.to_string())
    }

    /// Control/editor broker only. The native instance is serialized against DSP and editor
    /// mutation by the caller. Formats with unsupported state APIs deliberately return errors.
    pub(crate) fn save_state(&self) -> Result<Vec<u8>, super::ProcessorStateError> {
        self.plugin
            .save_state()
            .map_err(|_| super::ProcessorStateError::NativeFailure)
    }

    pub(crate) fn process(&mut self, frames: usize) -> Result<(), String> {
        // Activation validates the channel cap. Inline vectors initialize only live channels
        // without allocating the temporary slice-of-slices required by the host API.
        let mut input_refs = ArrayVec::<&[f32], CHANNEL_SLOTS>::new();
        for channel in &self.input_storage {
            input_refs.push(&channel[..frames]);
        }
        // AudioBuffer ties the outer borrow to its inner mutable references. Suppress the
        // container's drop check: it owns only borrowed slices, never the channel allocations.
        let mut output_refs = ManuallyDrop::new(ArrayVec::<&mut [f32], CHANNEL_SLOTS>::new());
        for channel in &mut self.output_storage {
            output_refs.push(&mut channel[..frames]);
        }
        let mut buffer = AudioBuffer::new(
            &input_refs,
            &mut output_refs,
            frames,
            &self.input_ranges,
            &self.output_ranges,
        );
        self.output_events.clear();
        let mut context = ProcessContext {
            sample_rate: self.sample_rate,
            max_block_size: self.max_block_size,
            transport: None,
            output_events: &mut self.output_events,
        };
        let status = self
            .plugin
            .process(&mut buffer, &self.input_events, &mut context)
            .map_err(|error| error.to_string())?;
        match status {
            ProcessStatus::Error => Err("processor reported a fatal process error".to_owned()),
            ProcessStatus::Continue | ProcessStatus::Sleep | ProcessStatus::Tail { .. } => Ok(()),
        }
    }

    pub(crate) fn has_editor(&self) -> bool {
        self.editor_allowed && self.plugin.info().has_editor
    }

    pub(crate) fn open_editor(
        &mut self,
        parent: WindowHandle,
        scale: f64,
    ) -> Result<EditorSnapshot, String> {
        let editor = self
            .plugin
            .editor()
            .ok_or_else(|| "processor does not expose a native editor".to_owned())?;
        editor
            .open(parent, scale)
            .map_err(|error| error.to_string())?;
        editor.show();
        Ok(EditorSnapshot {
            open: editor.is_open(),
            size: editor.size(),
            resizable: editor.is_resizable(),
        })
    }

    pub(crate) fn close_editor(&mut self) {
        if let Some(editor) = self.plugin.editor() {
            editor.hide();
            editor.close();
        }
    }

    pub(crate) fn tick_editor(&mut self) -> EditorSnapshot {
        let Some(editor) = self.plugin.editor() else {
            return EditorSnapshot::default();
        };
        editor.on_idle();
        EditorSnapshot {
            open: editor.is_open(),
            size: editor.size(),
            resizable: editor.is_resizable(),
        }
    }

    pub(crate) fn resize_editor(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        self.plugin.editor()?.set_size(width, height)
    }

    fn apply_initial_state(&mut self, parameters: &serde_json::Value) -> Result<(), String> {
        let Some(object) = parameters.as_object() else {
            return Ok(());
        };
        if let Some(encoded) = object
            .get("stateBase64")
            .and_then(serde_json::Value::as_str)
        {
            if encoded.len() > super::STATE_CAPACITY.div_ceil(3) * 4 {
                return Err("processor state exceeds 4 MiB".to_owned());
            }
            let state = BASE64.decode(encoded).map_err(|error| error.to_string())?;
            if state.len() > super::STATE_CAPACITY {
                return Err("processor state exceeds 4 MiB".to_owned());
            }
            self.plugin
                .load_state(&state)
                .map_err(|error| error.to_string())?;
        }
        if let Some(values) = object
            .get("parameterValues")
            .and_then(serde_json::Value::as_object)
        {
            for (id, value) in values {
                let id = id
                    .parse::<u32>()
                    .map_err(|error| format!("invalid processor parameter ID {id}: {error}"))?;
                let value = value
                    .as_f64()
                    .ok_or_else(|| format!("processor parameter {id} is not numeric"))?;
                self.set_parameter(id, value)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditorSnapshot {
    pub open: bool,
    pub size: Option<(u32, u32)>,
    pub resizable: bool,
}

impl Drop for RackWorkerPlugin {
    fn drop(&mut self) {
        self.plugin.deactivate();
    }
}

fn load_plugin(locator: &RackLocator) -> Result<Box<dyn Plugin<f32>>, String> {
    let info = locator.info();
    match locator.format {
        RackFormat::Vst3 => truce_rack_vst3::Vst3Scanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        RackFormat::Clap => truce_rack_clap::ClapScanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        #[cfg(target_vendor = "apple")]
        RackFormat::Au => truce_rack_au::AuScanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        #[cfg(target_vendor = "apple")]
        RackFormat::Auv3 => truce_rack_au3::Au3Scanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        #[cfg(target_os = "linux")]
        RackFormat::Lv2 => truce_rack_lv2::Lv2Scanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        #[allow(unreachable_patterns)]
        _ => Err(format!(
            "{} is unavailable on this platform",
            locator.format.argument()
        )),
    }
}

fn bus_ranges(buses: &[truce_rack_core::bus::Bus]) -> Vec<BusRange> {
    let mut offset = 0_usize;
    buses
        .iter()
        .map(|bus| {
            let channels = usize::try_from(bus.channels.count()).unwrap_or(usize::MAX);
            let range = BusRange::new(offset, channels);
            offset = offset.saturating_add(channels);
            range
        })
        .collect()
}

fn planar_storage(channels: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..channels).map(|_| vec![0.0; frames]).collect()
}

#[must_use]
pub(crate) fn lv2_search_path(custom_paths: &[PathBuf]) -> Option<std::ffi::OsString> {
    #[cfg(target_os = "linux")]
    {
        let mut paths = custom_paths.to_vec();
        if let Some(existing) = std::env::var_os("LV2_PATH") {
            paths.extend(std::env::split_paths(&existing));
        } else if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".lv2"));
            paths.extend([
                PathBuf::from("/usr/local/lib/lv2"),
                PathBuf::from("/usr/lib/lv2"),
            ]);
        }
        std::env::join_paths(paths).ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = custom_paths;
        None
    }
}

#[allow(dead_code)]
fn _assert_path_is_send(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn apple_limiter_native_state_restores_a_changed_parameter() {
        // The built-in AU fixture requires neither hardware nor a vendor authorization. This
        // checks native ClassInfo bytes, not a mock JSON parameter round trip.
        let info = truce_rack_au::AuScanner::new()
            .scan()
            .unwrap()
            .into_iter()
            .find(|info| info.name.contains("AUPeakLimiter"))
            .expect("macOS ships the AUPeakLimiter fixture");
        let locator = RackLocator::from_info(RackFormat::Au, info);
        let record = probe(locator.clone()).unwrap();
        let parameter = record
            .descriptor
            .parameters
            .iter()
            .find(|parameter| !parameter.is_read_only)
            .expect("limiter exposes a writable parameter");
        let mut first = RackWorkerPlugin::load(
            &locator,
            record.layout_index,
            48_000.0,
            128,
            &serde_json::json!({}),
        )
        .unwrap();
        first.set_parameter(parameter.id, 0.37).unwrap();
        let range = first
            .parameter_ranges
            .iter()
            .find(|range| range.id == parameter.id)
            .copied()
            .unwrap();
        let expected = first.plugin.parameter_value(range.index).unwrap();
        let (shared_path, config_path) = crate::worker_paths().expect("private worker paths");
        let (audio, control) = crate::shared_transport::create(
            &shared_path,
            crate::AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 128,
            },
        )
        .unwrap();
        drop(audio);
        let control = crate::HostedProcessorControl::new(control);
        let (_, shared) = crate::shared_transport::worker(&shared_path).unwrap();
        shared
            .header()
            .worker_status
            .store(crate::WORKER_READY, std::sync::atomic::Ordering::Release);
        let client =
            std::thread::spawn(move || control.snapshot_state(std::time::Duration::from_secs(2)));
        let first = std::sync::Mutex::new(first);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !client.is_finished() {
            assert!(std::time::Instant::now() < deadline);
            crate::apply_pending_state_request(&shared, &first);
            std::thread::yield_now();
        }
        let state = client.join().unwrap().unwrap();
        assert!(!state["stateBase64"].as_str().unwrap().is_empty());
        first
            .lock()
            .unwrap()
            .set_parameter(parameter.id, 0.91)
            .unwrap();
        let restored =
            RackWorkerPlugin::load(&locator, record.layout_index, 48_000.0, 128, &state).unwrap();
        let actual = restored.plugin.parameter_value(range.index).unwrap();
        assert!(
            (actual - expected).abs() < 0.001,
            "native state failed to restore {expected}: {actual}"
        );
        drop(shared);
        crate::cleanup_worker_paths(&shared_path, &config_path);
        assert!(!shared_path.exists());
    }

    #[test]
    fn effect_layout_prefers_plain_stereo() {
        let layouts = [
            BusLayout::stereo_with_sidechain("Sidechain"),
            BusLayout::mono(),
            BusLayout::stereo(),
        ];

        let (index, selected) = select_effect_layout(&layouts).expect("one layout is usable");

        assert_eq!(index, 2);
        assert_eq!(selected.total_input_channels(), 2);
        assert_eq!(selected.total_output_channels(), 2);
    }

    #[test]
    fn effect_layout_rejects_empty_and_over_limit_layouts() {
        use truce_rack_core::bus::{Bus, ChannelConfig};

        let mut over_limit = BusLayout::new();
        over_limit
            .inputs
            .push(Bus::main("Input", ChannelConfig::Discrete(65)));
        over_limit
            .outputs
            .push(Bus::main("Output", ChannelConfig::Discrete(65)));

        assert!(select_effect_layout(&[BusLayout::new(), over_limit]).is_none());
    }

    #[test]
    fn parameter_normalization_is_bounded_and_handles_invalid_ranges() {
        assert!((normalize_parameter(0.0, -1.0, 1.0) - 0.5).abs() < f64::EPSILON);
        assert!(normalize_parameter(-4.0, -1.0, 1.0).abs() < f64::EPSILON);
        assert!((normalize_parameter(4.0, -1.0, 1.0) - 1.0).abs() < f64::EPSILON);
        assert!(normalize_parameter(1.0, 1.0, 1.0).abs() < f64::EPSILON);
        assert!(normalize_parameter(f64::NAN, 0.0, 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn locator_round_trips_through_worker_argument() {
        let directory = tonenari_test_support::FixtureDir::new("rack-locator").unwrap();
        let locator = RackLocator {
            format: RackFormat::Clap,
            unique_id: "io.tonenari.fixture".to_owned(),
            path: directory.join("ToneNari Fixture.clap"),
            name: "Fixture".to_owned(),
            vendor: "ToneNari".to_owned(),
            version: 3,
            category: RackCategory::Effect,
            has_editor: true,
            accepts_midi: false,
        };

        let decoded = RackLocator::decode(&locator.encode().expect("locator should encode"))
            .expect("locator should decode");

        assert_eq!(decoded.format, locator.format);
        assert_eq!(decoded.unique_id, locator.unique_id);
        assert_eq!(decoded.path, locator.path);
        assert_eq!(decoded.name, locator.name);
        assert_eq!(decoded.vendor, locator.vendor);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn apple_bandpass_editor_capability_reaches_catalog_and_worker() {
        let info = truce_rack_au::AuScanner::new()
            .scan()
            .unwrap()
            .into_iter()
            .find(|info| info.name.contains("AUBandpass"))
            .expect("macOS ships the AUBandpass fixture");
        let record = probe(RackLocator::from_info(RackFormat::Au, info)).unwrap();
        assert!(record.descriptor.has_editor);
        assert!(record.locator.has_editor);
        let worker = RackWorkerPlugin::load(
            &record.locator,
            record.layout_index,
            48_000.0,
            128,
            &serde_json::json!({}),
        )
        .unwrap();
        assert!(worker.has_editor());
    }
}
