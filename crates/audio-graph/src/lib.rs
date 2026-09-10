mod compiler;
mod model;

#[cfg(test)]
mod tests;

pub use compiler::{
    BuiltInNodeResolver, CompileContext, CompileError, CompileLimits, GraphCompiler, GraphIssue,
    GraphIssueCode, GraphResource, NodeResolutionError, NodeResolver, ResolvedNode,
    validate_virtual_endpoint_gain,
};
pub use mixrelay_audio_io::{
    ApplicationCaptureRequest, ApplicationSelector, ChannelLayout, SourcePlaybackPolicy,
    StreamSpec, VirtualEndpointId, VirtualEndpointSpec,
};
pub use model::{
    AUDIO_PORT_ID, AudioDeviceInputConfig, AudioDeviceOutputConfig, AudioEndpoint,
    CURRENT_GRAPH_SCHEMA_VERSION, ChannelMapping, ChannelRoute, CompiledEdge, CompiledNode,
    CompiledPort, CompressorConfig, DeviceSelector, EdgeId, EdgeSpec, ExecutionPlan,
    FROM_OS_PORT_ID, GainConfig, GraphDocument, LimiterConfig, MixerConfig, NodeConfig,
    NodeControl, NodeId, NodeKind, NodeSpec, NodeTypeId, PortDirection, PortId, ProviderNodeConfig,
    ProviderPorts, RenderFormat, TO_OS_PORT_ID, VirtualEndpointConfig, VirtualEndpointReference,
};
