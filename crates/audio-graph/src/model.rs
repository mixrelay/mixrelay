use std::fmt;
use std::mem::size_of_val;

use crate::provider_layout::ProviderPorts;
use mixrelay_audio_io::{ApplicationCaptureRequest, VirtualEndpointSpec};
use serde::{Deserialize, Serialize};

pub const CURRENT_GRAPH_SCHEMA_VERSION: u32 = 2;
pub const AUDIO_PORT_ID: &str = "audio";
/// The Driver capture mix (OS playback plus all graph contributions).
pub const FROM_OS_PORT_ID: &str = "from_os";
/// Graph contributions sent to the Driver render stream, not a capture writer.
pub const TO_OS_PORT_ID: &str = "to_os";

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(NodeId);
string_id!(EdgeId);
string_id!(PortId);
string_id!(NodeTypeId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDocument {
    pub schema_version: u32,
    pub revision: u64,
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
}

impl GraphDocument {
    #[must_use]
    pub fn new(revision: u64, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec>) -> Self {
        Self {
            schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
            revision,
            nodes,
            edges,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSpec {
    pub id: NodeId,
    pub label: Option<String>,
    /// Execution and signal state, independent of the node's saved configuration.
    #[serde(default)]
    pub control: NodeControl,
    #[serde(flatten)]
    pub config: NodeConfig,
}

/// Authored node controls. Disabling removes processing; muting preserves processing
/// but silences the node boundary. Neither state changes connections or parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NodeControl {
    /// Whether the node participates in execution. Prepared resources may remain owned while OFF.
    pub enabled: bool,
    /// Whether the executing node emits silence (or records silence for a recorder).
    pub muted: bool,
}

impl Default for NodeControl {
    fn default() -> Self {
        Self {
            enabled: true,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "config", rename_all = "snake_case")]
pub enum NodeConfig {
    ApplicationCapture(ApplicationCaptureRequest),
    AudioDeviceInput(AudioDeviceInputConfig),
    AudioDeviceOutput(AudioDeviceOutputConfig),
    /// File output. Recording state and file handles belong to the runtime, not the graph.
    Recorder {
        channels: u16,
    },
    Compressor(CompressorConfig),
    Limiter(LimiterConfig),
    Effect(crate::effects::EffectConfig),
    Mixer(MixerConfig),
    Gain(GainConfig),
    VirtualEndpoint(VirtualEndpointConfig),
    /// Runtime-only Driver client transport. Authored sources must first be composed
    /// with the Driver capture snapshot; this variant cannot enter persisted JSON.
    #[serde(skip)]
    VirtualEndpointTransport(VirtualEndpointSpec),
    /// Device I/O snapshot storage, prepared only by catalog composition.
    #[serde(skip)]
    DeviceBus {
        channels: u16,
    },
    Provider(ProviderNodeConfig),
}

impl NodeConfig {
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::ApplicationCapture(_) => NodeKind::ApplicationCapture,
            Self::AudioDeviceInput(_) => NodeKind::AudioDeviceInput,
            Self::AudioDeviceOutput(_) => NodeKind::AudioDeviceOutput,
            Self::Recorder { .. } => NodeKind::Recorder,
            Self::Compressor(_) => NodeKind::Compressor,
            Self::Limiter(_) => NodeKind::Limiter,
            Self::Effect(_) => NodeKind::Effect,
            Self::Mixer(_) => NodeKind::Mixer,
            Self::Gain(_) => NodeKind::Gain,
            Self::VirtualEndpoint(_) | Self::VirtualEndpointTransport(_) => {
                NodeKind::VirtualEndpoint
            }
            Self::Provider(_) => NodeKind::Provider,
            Self::DeviceBus { .. } => NodeKind::DeviceBus,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    ApplicationCapture,
    AudioDeviceInput,
    AudioDeviceOutput,
    Recorder,
    Compressor,
    Limiter,
    Effect,
    Mixer,
    Gain,
    VirtualEndpoint,
    #[serde(skip)]
    DeviceBus,
    Provider,
}

pub use mixrelay_audio_io::DeviceSelector;

/// Graph-side processing for an endpoint. Gain is deliberately not a device property:
/// changing it must not recreate the native endpoint or modify its advertised format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualEndpointConfig {
    #[serde(flatten)]
    pub spec: VirtualEndpointSpec,
    /// Graph-local port selection; it does not alter the system device's streams.
    pub reference: VirtualEndpointReference,
    #[serde(default, skip_serializing_if = "is_unity_gain")]
    pub gain_db: f32,
}

/// Graph-local source/sink visibility for a canonical virtual device.
/// At least one direction must be selected; the compiler validates stream availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VirtualEndpointReference {
    /// Exposes Driver capture: OS playback plus all graph contributions.
    pub render: bool,
    /// Exposes the graph contribution sink feeding Driver render.
    pub capture: bool,
}

// Serde's skip predicate receives a borrowed field, including scalar fields.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_unity_gain(value: &f32) -> bool {
    *value == 0.0
}

impl From<VirtualEndpointSpec> for VirtualEndpointConfig {
    fn from(spec: VirtualEndpointSpec) -> Self {
        let reference = VirtualEndpointReference {
            render: spec.capture.is_some(),
            capture: spec.render.is_some(),
        };
        Self {
            spec,
            reference,
            gain_db: 0.0,
        }
    }
}

impl std::ops::Deref for VirtualEndpointConfig {
    type Target = VirtualEndpointSpec;

    fn deref(&self) -> &Self::Target {
        &self.spec
    }
}

impl std::ops::DerefMut for VirtualEndpointConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.spec
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInputConfig {
    pub selector: DeviceSelector,
    pub output_channels: u16,
    #[serde(default)]
    pub gain_db: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceOutputConfig {
    pub selector: DeviceSelector,
    pub input_channels: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressorConfig {
    pub version: u16,
    #[serde(default)]
    pub bypass: bool,
    pub channels: u16,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_gain_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterConfig {
    pub version: u16,
    #[serde(default)]
    pub bypass: bool,
    pub channels: u16,
    pub ceiling_db: f32,
    pub release_ms: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerConfig {
    pub inputs: Vec<PortId>,
    pub channels: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GainConfig {
    pub channels: u16,
    pub gain_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNodeConfig {
    pub type_id: NodeTypeId,
    /// Last known port contract, retained when the plug-in is not installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<ProviderPorts>,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSpec {
    pub id: EdgeId,
    pub source: AudioEndpoint,
    pub destination: AudioEndpoint,
    #[serde(default)]
    pub channel_mapping: ChannelMapping,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEndpoint {
    pub node_id: NodeId,
    pub port_id: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ChannelMapping {
    #[default]
    Auto,
    Explicit {
        routes: Vec<ChannelRoute>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRoute {
    pub source_channel: u16,
    pub destination_channel: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderFormat {
    pub sample_rate: u32,
    pub block_frames: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    revision: u64,
    format: RenderFormat,
    nodes: Box<[CompiledNode]>,
    edges: Box<[CompiledEdge]>,
}

impl ExecutionPlan {
    pub(crate) fn new(
        revision: u64,
        format: RenderFormat,
        nodes: Vec<CompiledNode>,
        edges: Vec<CompiledEdge>,
    ) -> Self {
        Self {
            revision,
            format,
            nodes: nodes.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
        }
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn format(&self) -> RenderFormat {
        self.format
    }

    #[must_use]
    pub fn nodes(&self) -> &[CompiledNode] {
        &self.nodes
    }

    #[must_use]
    pub fn edges(&self) -> &[CompiledEdge] {
        &self.edges
    }

    pub(crate) fn owned_memory_bytes(&self) -> usize {
        let heap_capacity_bytes = size_of_val(self.nodes.as_ref())
            + size_of_val(self.edges.as_ref())
            + self
                .nodes
                .iter()
                .map(CompiledNode::nested_heap_capacity_bytes)
                .sum::<usize>()
            + self
                .edges
                .iter()
                .map(CompiledEdge::nested_heap_capacity_bytes)
                .sum::<usize>();
        size_of_val(self) + heap_capacity_bytes
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledNode {
    id: NodeId,
    config: NodeConfig,
    control: NodeControl,
    inputs: Box<[CompiledPort]>,
    outputs: Box<[CompiledPort]>,
}

impl CompiledNode {
    pub(crate) fn new(
        id: NodeId,
        config: NodeConfig,
        control: NodeControl,
        inputs: Vec<CompiledPort>,
        outputs: Vec<CompiledPort>,
    ) -> Self {
        Self {
            id,
            config,
            control,
            inputs: inputs.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.config.kind()
    }

    #[must_use]
    pub const fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Returns the controls captured when this immutable execution plan was compiled.
    #[must_use]
    pub const fn control(&self) -> NodeControl {
        self.control
    }

    #[must_use]
    pub fn inputs(&self) -> &[CompiledPort] {
        &self.inputs
    }

    #[must_use]
    pub fn outputs(&self) -> &[CompiledPort] {
        &self.outputs
    }

    fn nested_heap_capacity_bytes(&self) -> usize {
        let encoded_config_bytes = match &self.config {
            NodeConfig::VirtualEndpointTransport(spec) => spec.owned_heap_capacity_bytes(),
            NodeConfig::DeviceBus { .. } => 0,
            _ => serde_json::to_vec(&self.config).map_or(0, |encoded| encoded.len()),
        };
        self.id.0.capacity()
            + encoded_config_bytes
            + size_of_val(self.inputs.as_ref())
            + size_of_val(self.outputs.as_ref())
            + self
                .inputs
                .iter()
                .chain(&self.outputs)
                .map(|port| port.id.0.capacity())
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPort {
    id: PortId,
    direction: PortDirection,
    channels: u16,
}

impl CompiledPort {
    #[must_use]
    pub const fn new(id: PortId, direction: PortDirection, channels: u16) -> Self {
        Self {
            id,
            direction,
            channels,
        }
    }

    #[must_use]
    pub fn id(&self) -> &PortId {
        &self.id
    }

    #[must_use]
    pub const fn direction(&self) -> PortDirection {
        self.direction
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledEdge {
    id: EdgeId,
    source: AudioEndpoint,
    destination: AudioEndpoint,
    routes: Box<[ChannelRoute]>,
}

impl CompiledEdge {
    pub(crate) fn new(
        id: EdgeId,
        source: AudioEndpoint,
        destination: AudioEndpoint,
        routes: Vec<ChannelRoute>,
    ) -> Self {
        Self {
            id,
            source,
            destination,
            routes: routes.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn id(&self) -> &EdgeId {
        &self.id
    }

    #[must_use]
    pub fn source(&self) -> &AudioEndpoint {
        &self.source
    }

    #[must_use]
    pub fn destination(&self) -> &AudioEndpoint {
        &self.destination
    }

    #[must_use]
    pub fn routes(&self) -> &[ChannelRoute] {
        &self.routes
    }

    fn nested_heap_capacity_bytes(&self) -> usize {
        self.id.0.capacity()
            + self.source.node_id.0.capacity()
            + self.source.port_id.0.capacity()
            + self.destination.node_id.0.capacity()
            + self.destination.port_id.0.capacity()
            + size_of_val(self.routes.as_ref())
    }
}

#[cfg(test)]
mod dynamics_compatibility_tests {
    use super::{CompressorConfig, LimiterConfig};

    #[test]
    fn legacy_dynamics_presets_default_to_processing_and_roundtrip_explicit_bypass() {
        let mut compressor: CompressorConfig = serde_json::from_value(serde_json::json!({
            "version":1,"channels":2,"thresholdDb":-18,"ratio":4,"attackMs":10,"releaseMs":120,"makeupGainDb":0,
        })).unwrap();
        let mut limiter: LimiterConfig = serde_json::from_value(serde_json::json!({
            "version":1,"channels":2,"ceilingDb":-1,"releaseMs":80,
        }))
        .unwrap();
        assert!(!compressor.bypass);
        assert!(!limiter.bypass);
        compressor.bypass = true;
        limiter.bypass = true;
        assert_eq!(
            serde_json::from_value::<CompressorConfig>(serde_json::to_value(&compressor).unwrap())
                .unwrap(),
            compressor
        );
        assert_eq!(
            serde_json::from_value::<LimiterConfig>(serde_json::to_value(&limiter).unwrap())
                .unwrap(),
            limiter
        );
    }
}
