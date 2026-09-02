use std::fmt;

use serde::{Deserialize, Serialize};

pub const CURRENT_GRAPH_SCHEMA_VERSION: u32 = 1;
pub const AUDIO_PORT_ID: &str = "audio";

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
    #[serde(flatten)]
    pub config: NodeConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "config", rename_all = "snake_case")]
pub enum NodeConfig {
    ApplicationCapture(ApplicationCaptureConfig),
    AudioDeviceInput(AudioDeviceInputConfig),
    AudioDeviceOutput(AudioDeviceOutputConfig),
    Mixer(MixerConfig),
    Gain(GainConfig),
    VirtualDevice(VirtualDeviceConfig),
    Provider(ProviderNodeConfig),
}

impl NodeConfig {
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::ApplicationCapture(_) => NodeKind::ApplicationCapture,
            Self::AudioDeviceInput(_) => NodeKind::AudioDeviceInput,
            Self::AudioDeviceOutput(_) => NodeKind::AudioDeviceOutput,
            Self::Mixer(_) => NodeKind::Mixer,
            Self::Gain(_) => NodeKind::Gain,
            Self::VirtualDevice(_) => NodeKind::VirtualDevice,
            Self::Provider(_) => NodeKind::Provider,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    ApplicationCapture,
    AudioDeviceInput,
    AudioDeviceOutput,
    Mixer,
    Gain,
    VirtualDevice,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "match", rename_all = "snake_case")]
pub enum ApplicationSelector {
    BundleId {
        bundle_id: String,
        #[serde(default = "default_true")]
        include_descendants: bool,
    },
    ExecutablePath {
        path: String,
    },
    AllExcept {
        bundle_ids: Vec<String>,
    },
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "match", rename_all = "snake_case")]
pub enum DeviceSelector {
    PersistentUid { uid: String },
    DefaultInput,
    DefaultOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationCaptureConfig {
    pub selector: ApplicationSelector,
    pub output_channels: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInputConfig {
    pub selector: DeviceSelector,
    pub output_channels: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceOutputConfig {
    pub selector: DeviceSelector,
    pub input_channels: u16,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDeviceConfig {
    pub persistent_uid: String,
    pub name: String,
    pub channels: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNodeConfig {
    pub type_id: NodeTypeId,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    revision: u64,
    format: RenderFormat,
    nodes: Vec<CompiledNode>,
    edges: Vec<CompiledEdge>,
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
            nodes,
            edges,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledNode {
    id: NodeId,
    kind: NodeKind,
    inputs: Vec<CompiledPort>,
    outputs: Vec<CompiledPort>,
}

impl CompiledNode {
    pub(crate) fn new(
        id: NodeId,
        kind: NodeKind,
        inputs: Vec<CompiledPort>,
        outputs: Vec<CompiledPort>,
    ) -> Self {
        Self {
            id,
            kind,
            inputs,
            outputs,
        }
    }

    #[must_use]
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    #[must_use]
    pub fn inputs(&self) -> &[CompiledPort] {
        &self.inputs
    }

    #[must_use]
    pub fn outputs(&self) -> &[CompiledPort] {
        &self.outputs
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    routes: Vec<ChannelRoute>,
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
            routes,
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
}
