use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use mixrelay_audio_io::ApplicationCaptureRequest;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{
    AUDIO_PORT_ID, AudioEndpoint, CURRENT_GRAPH_SCHEMA_VERSION, ChannelMapping, ChannelRoute,
    CompiledEdge, CompiledNode, CompiledPort, DeviceSelector, EdgeId, ExecutionPlan,
    FROM_OS_PORT_ID, GraphDocument, MixerConfig, NodeConfig, NodeId, NodeSpec, PortDirection,
    PortId, RenderFormat, TO_OS_PORT_ID, VirtualEndpointConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileContext {
    pub format: RenderFormat,
    pub limits: CompileLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileLimits {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_channels_per_port: u16,
    pub max_sample_rate: u32,
    pub max_block_frames: u32,
    pub max_plan_bytes: usize,
}

impl Default for CompileLimits {
    fn default() -> Self {
        Self {
            max_nodes: 256,
            max_edges: 1_024,
            max_channels_per_port: 64,
            max_sample_rate: 384_000,
            max_block_frames: 4_096,
            max_plan_bytes: 8 * 1_024 * 1_024,
        }
    }
}

pub trait NodeResolver: Send + Sync {
    /// Resolves a persisted node into ports available on the current system.
    ///
    /// # Errors
    ///
    /// Returns [`NodeResolutionError`] when configuration is invalid or a
    /// runtime capability required by the node is unavailable.
    fn resolve(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNode {
    pub inputs: Vec<CompiledPort>,
    pub outputs: Vec<CompiledPort>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeResolutionError {
    #[error("invalid node configuration: {reason}")]
    InvalidConfiguration { reason: String },
    #[error("node capability is unavailable: {reason}")]
    CapabilityUnavailable { reason: String },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltInNodeResolver;

impl NodeResolver for BuiltInNodeResolver {
    fn resolve(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        match &node.config {
            NodeConfig::ApplicationCapture(config) => resolve_application_capture(config, context),
            NodeConfig::AudioDeviceInput(config) => {
                validate_device_selector(&config.selector)?;
                validate_channels(config.output_channels, context.limits.max_channels_per_port)?;
                if !config.gain_db.is_finite() {
                    return Err(invalid("gainDb must be finite"));
                }
                Ok(ResolvedNode {
                    inputs: vec![],
                    outputs: vec![audio_port(PortDirection::Output, config.output_channels)],
                })
            }
            NodeConfig::AudioDeviceOutput(config) => {
                validate_device_selector(&config.selector)?;
                validate_channels(config.input_channels, context.limits.max_channels_per_port)?;
                Ok(ResolvedNode {
                    inputs: vec![audio_port(PortDirection::Input, config.input_channels)],
                    outputs: vec![],
                })
            }
            NodeConfig::Recorder { channels } => {
                validate_channels(
                    *channels,
                    context
                        .limits
                        .max_channels_per_port
                        .min(mixrelay_audio_io::formats::MAX_RECORDING_CHANNELS),
                )?;
                Ok(ResolvedNode {
                    inputs: vec![audio_port(PortDirection::Input, *channels)],
                    outputs: vec![],
                })
            }
            NodeConfig::Compressor(config) => {
                validate_channels(config.channels, context.limits.max_channels_per_port)?;
                validate_compressor(config)?;
                Ok(audio_processor(config.channels))
            }
            NodeConfig::Limiter(config) => {
                validate_channels(config.channels, context.limits.max_channels_per_port)?;
                validate_limiter(config)?;
                Ok(audio_processor(config.channels))
            }
            NodeConfig::Mixer(config) => resolve_mixer(config, context),
            NodeConfig::Gain(config) => {
                validate_channels(config.channels, context.limits.max_channels_per_port)?;
                if !config.gain_db.is_finite() {
                    return Err(invalid("gainDb must be finite"));
                }
                Ok(ResolvedNode {
                    inputs: vec![audio_port(PortDirection::Input, config.channels)],
                    outputs: vec![audio_port(PortDirection::Output, config.channels)],
                })
            }
            NodeConfig::VirtualEndpoint(config) => {
                validate_virtual_endpoint_gain(config.gain_db)?;
                resolve_virtual_endpoint(config, context)
            }
            NodeConfig::VirtualEndpointTransport(spec) => resolve_virtual_transport(spec, context),
            NodeConfig::DeviceBus { channels } => {
                validate_channels(*channels, context.limits.max_channels_per_port)?;
                Ok(audio_processor(*channels))
            }
            NodeConfig::Provider(config) => Err(NodeResolutionError::CapabilityUnavailable {
                reason: format!("node provider is unavailable: {}", config.type_id),
            }),
        }
    }
}

/// Validates a virtual-device reference's gain in decibels before DSP lowering.
/// Both direct endpoint nodes and composed references must use this contract;
/// lowering to a generic gain processor must not bypass the endpoint's limits.
///
/// # Errors
/// Returns an invalid-configuration error for non-finite values or values outside
/// the inclusive -60 to +12 dB range.
pub fn validate_virtual_endpoint_gain(gain_db: f32) -> Result<(), NodeResolutionError> {
    validate_parameter("gainDb", gain_db, -60.0, 12.0)
}

fn audio_processor(channels: u16) -> ResolvedNode {
    ResolvedNode {
        inputs: vec![audio_port(PortDirection::Input, channels)],
        outputs: vec![audio_port(PortDirection::Output, channels)],
    }
}

fn validate_compressor(config: &crate::model::CompressorConfig) -> Result<(), NodeResolutionError> {
    if config.version != 1 {
        return Err(invalid("unsupported compressor parameter version"));
    }
    validate_parameter("thresholdDb", config.threshold_db, -60.0, 0.0)?;
    validate_parameter("ratio", config.ratio, 1.0, 20.0)?;
    validate_parameter("attackMs", config.attack_ms, 0.1, 200.0)?;
    validate_parameter("releaseMs", config.release_ms, 5.0, 2_000.0)?;
    validate_parameter("makeupGainDb", config.makeup_gain_db, 0.0, 24.0)
}

fn validate_limiter(config: &crate::model::LimiterConfig) -> Result<(), NodeResolutionError> {
    if config.version != 1 {
        return Err(invalid("unsupported limiter parameter version"));
    }
    validate_parameter("ceilingDb", config.ceiling_db, -24.0, 0.0)?;
    validate_parameter("releaseMs", config.release_ms, 5.0, 2_000.0)
}

fn validate_parameter(
    name: &str,
    value: f32,
    minimum: f32,
    maximum: f32,
) -> Result<(), NodeResolutionError> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(invalid(format!(
            "{name} must be finite and between {minimum} and {maximum}"
        )));
    }
    Ok(())
}

fn resolve_application_capture(
    config: &ApplicationCaptureRequest,
    context: &CompileContext,
) -> Result<ResolvedNode, NodeResolutionError> {
    config
        .validate_contract(context.limits.max_channels_per_port)
        .map_err(|error| invalid(error.to_string()))?;
    Ok(ResolvedNode {
        inputs: vec![],
        outputs: vec![audio_port(PortDirection::Output, config.stream.channels)],
    })
}

fn resolve_mixer(
    config: &MixerConfig,
    context: &CompileContext,
) -> Result<ResolvedNode, NodeResolutionError> {
    validate_channels(config.channels, context.limits.max_channels_per_port)?;
    if config.inputs.is_empty() {
        return Err(invalid("a mixer requires at least one input port"));
    }

    let mut unique_ports = BTreeSet::new();
    let mut inputs = Vec::with_capacity(config.inputs.len());
    for port_id in &config.inputs {
        validate_identifier("mixer input port", port_id.as_str())?;
        if !unique_ports.insert(port_id) {
            return Err(invalid(format!("duplicate mixer input port: {port_id}")));
        }
        inputs.push(CompiledPort::new(
            port_id.clone(),
            PortDirection::Input,
            config.channels,
        ));
    }

    Ok(ResolvedNode {
        inputs,
        outputs: vec![audio_port(PortDirection::Output, config.channels)],
    })
}

fn resolve_virtual_endpoint(
    config: &VirtualEndpointConfig,
    context: &CompileContext,
) -> Result<ResolvedNode, NodeResolutionError> {
    let maximum = context.limits.max_channels_per_port;
    config
        .validate_contract(maximum, maximum)
        .map_err(|error| invalid(error.to_string()))?;
    let reference = config.reference;
    if !reference.render && !reference.capture {
        return Err(invalid(
            "virtual endpoint reference must select a source or sink",
        ));
    }
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if reference.capture {
        let stream = config
            .spec
            .capture
            .ok_or_else(|| invalid("virtual endpoint sink requires a native capture stream"))?;
        inputs.push(CompiledPort::new(
            PortId::from(TO_OS_PORT_ID),
            PortDirection::Input,
            stream.channels,
        ));
    }
    if reference.render {
        let channels = config
            .spec
            .render
            .iter()
            .chain(config.spec.capture.iter())
            .map(|stream| stream.channels)
            .max()
            .ok_or_else(|| {
                invalid("virtual endpoint source requires at least one native stream")
            })?;
        outputs.push(CompiledPort::new(
            PortId::from(FROM_OS_PORT_ID),
            PortDirection::Output,
            channels,
        ));
    }
    Ok(ResolvedNode { inputs, outputs })
}

fn resolve_virtual_transport(
    spec: &mixrelay_audio_io::VirtualEndpointSpec,
    context: &CompileContext,
) -> Result<ResolvedNode, NodeResolutionError> {
    let maximum = context.limits.max_channels_per_port;
    spec.validate_contract(maximum, maximum)
        .map_err(|error| invalid(error.to_string()))?;
    Ok(ResolvedNode {
        inputs: spec
            .capture
            .iter()
            .map(|stream| {
                CompiledPort::new(
                    PortId::from(TO_OS_PORT_ID),
                    PortDirection::Input,
                    stream.channels,
                )
            })
            .collect(),
        outputs: spec
            .render
            .iter()
            .map(|stream| {
                CompiledPort::new(
                    PortId::from(FROM_OS_PORT_ID),
                    PortDirection::Output,
                    stream.channels,
                )
            })
            .collect(),
    })
}

fn audio_port(direction: PortDirection, channels: u16) -> CompiledPort {
    CompiledPort::new(PortId::from(AUDIO_PORT_ID), direction, channels)
}

fn validate_channels(channels: u16, maximum: u16) -> Result<(), NodeResolutionError> {
    if channels == 0 || channels > maximum {
        return Err(invalid(format!(
            "channel count must be between 1 and {maximum}, found {channels}"
        )));
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), NodeResolutionError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{name} cannot be empty")));
    }
    Ok(())
}

fn validate_device_selector(selector: &DeviceSelector) -> Result<(), NodeResolutionError> {
    match selector {
        DeviceSelector::PersistentUid { uid } => validate_identifier("audio device UID", uid),
        DeviceSelector::DefaultInput | DeviceSelector::DefaultOutput => Ok(()),
    }
}

fn invalid(reason: impl Into<String>) -> NodeResolutionError {
    NodeResolutionError::InvalidConfiguration {
        reason: reason.into(),
    }
}

pub struct GraphCompiler {
    resolver: Arc<dyn NodeResolver>,
}

impl Default for GraphCompiler {
    fn default() -> Self {
        Self::new(BuiltInNodeResolver)
    }
}

impl GraphCompiler {
    #[must_use]
    pub fn new(resolver: impl NodeResolver + 'static) -> Self {
        Self {
            resolver: Arc::new(resolver),
        }
    }

    /// Compiles a persisted graph into an immutable runtime plan.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`] containing every validation issue that can be
    /// found without making assumptions about an invalid graph.
    pub fn compile(
        &self,
        document: &GraphDocument,
        context: CompileContext,
    ) -> Result<ExecutionPlan, CompileError> {
        let mut issues = validate_document(document, &context);
        let mut nodes = self.resolve_nodes(document, &context, &mut issues);
        let (compiled_edges, dependencies) = compile_edges(document, &nodes, &mut issues);

        if !issues.is_empty() {
            return Err(CompileError::new(issues));
        }

        let ordered_ids = topological_order(&nodes, &dependencies).map_err(|nodes| {
            CompileError::new(vec![GraphIssue::CycleDetected { node_ids: nodes }])
        })?;
        let compiled_nodes = ordered_ids
            .into_iter()
            .filter_map(|node_id| nodes.remove(&node_id))
            .collect();

        let plan = ExecutionPlan::new(
            document.revision,
            context.format,
            compiled_nodes,
            compiled_edges,
        );
        let actual_plan_bytes = plan.owned_memory_bytes();
        if actual_plan_bytes > context.limits.max_plan_bytes {
            return Err(CompileError::new(vec![GraphIssue::LimitExceeded {
                resource: GraphResource::PlanBytes,
                limit: context.limits.max_plan_bytes,
                actual: actual_plan_bytes,
            }]));
        }
        Ok(plan)
    }

    fn resolve_nodes(
        &self,
        document: &GraphDocument,
        context: &CompileContext,
        issues: &mut Vec<GraphIssue>,
    ) -> BTreeMap<NodeId, CompiledNode> {
        let mut nodes = BTreeMap::new();
        for node in &document.nodes {
            if node.id.as_str().trim().is_empty() {
                issues.push(GraphIssue::InvalidNodeId);
            } else if nodes.contains_key(&node.id) {
                issues.push(GraphIssue::DuplicateNodeId {
                    node_id: node.id.clone(),
                });
            } else {
                self.resolve_node(node, context, &mut nodes, issues);
            }
        }
        nodes
    }

    fn resolve_node(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
        nodes: &mut BTreeMap<NodeId, CompiledNode>,
        issues: &mut Vec<GraphIssue>,
    ) {
        match self.resolver.resolve(node, context) {
            Ok(resolved) => {
                nodes.insert(
                    node.id.clone(),
                    CompiledNode::new(
                        node.id.clone(),
                        node.config.clone(),
                        node.control,
                        resolved.inputs,
                        resolved.outputs,
                    ),
                );
            }
            Err(error) => issues.push(GraphIssue::NodeResolutionFailed {
                node_id: node.id.clone(),
                reason: error.to_string(),
            }),
        }
    }
}

fn validate_document(document: &GraphDocument, context: &CompileContext) -> Vec<GraphIssue> {
    let mut issues = Vec::new();
    if document.schema_version != CURRENT_GRAPH_SCHEMA_VERSION {
        issues.push(GraphIssue::UnsupportedSchemaVersion {
            expected: CURRENT_GRAPH_SCHEMA_VERSION,
            found: document.schema_version,
        });
    }
    if context.format.sample_rate == 0
        || context.format.sample_rate > context.limits.max_sample_rate
        || context.format.block_frames == 0
        || context.format.block_frames > context.limits.max_block_frames
    {
        issues.push(GraphIssue::InvalidRenderFormat {
            sample_rate: context.format.sample_rate,
            block_frames: context.format.block_frames,
        });
    }
    append_limit_issue(
        &mut issues,
        GraphResource::Nodes,
        context.limits.max_nodes,
        document.nodes.len(),
    );
    append_limit_issue(
        &mut issues,
        GraphResource::Edges,
        context.limits.max_edges,
        document.edges.len(),
    );
    let mut endpoint_nodes = BTreeMap::<&str, &NodeId>::new();
    for node in &document.nodes {
        let NodeConfig::VirtualEndpointTransport(endpoint) = &node.config else {
            continue;
        };
        if let Some(first_node_id) = endpoint_nodes.get(endpoint.id.as_str()) {
            issues.push(GraphIssue::DuplicateVirtualEndpointId {
                endpoint_id: endpoint.id.as_str().to_owned(),
                first_node_id: (*first_node_id).clone(),
                duplicate_node_id: node.id.clone(),
            });
        } else {
            endpoint_nodes.insert(endpoint.id.as_str(), &node.id);
        }
    }
    issues
}

fn append_limit_issue(
    issues: &mut Vec<GraphIssue>,
    resource: GraphResource,
    limit: usize,
    actual: usize,
) {
    if actual > limit {
        issues.push(GraphIssue::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
}

fn compile_edges(
    document: &GraphDocument,
    nodes: &BTreeMap<NodeId, CompiledNode>,
    issues: &mut Vec<GraphIssue>,
) -> (Vec<CompiledEdge>, BTreeMap<NodeId, BTreeSet<NodeId>>) {
    let mut state = EdgeCompilationState::new(nodes);
    for edge in &document.edges {
        state.compile(edge, issues);
    }
    (state.edges, state.dependencies)
}

struct EdgeCompilationState<'a> {
    nodes: &'a BTreeMap<NodeId, CompiledNode>,
    edge_ids: BTreeSet<EdgeId>,
    edges: Vec<CompiledEdge>,
    dependencies: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

impl<'a> EdgeCompilationState<'a> {
    fn new(nodes: &'a BTreeMap<NodeId, CompiledNode>) -> Self {
        Self {
            nodes,
            edge_ids: BTreeSet::new(),
            edges: Vec::new(),
            dependencies: nodes
                .keys()
                .cloned()
                .map(|node_id| (node_id, BTreeSet::new()))
                .collect(),
        }
    }

    fn compile(&mut self, edge: &crate::model::EdgeSpec, issues: &mut Vec<GraphIssue>) {
        if !self.validate_id(edge, issues) {
            return;
        }
        let issue_count = issues.len();
        let source = resolve_port(
            self.nodes,
            &edge.id,
            &edge.source,
            PortDirection::Output,
            issues,
        );
        let destination = resolve_port(
            self.nodes,
            &edge.id,
            &edge.destination,
            PortDirection::Input,
            issues,
        );
        self.validate_self_connection(edge, issues);
        let routes = Self::resolve_routes(edge, source, destination, issues);
        if issues.len() == issue_count
            && let Some(routes) = routes
        {
            self.push(edge, routes);
        }
    }

    fn validate_self_connection(
        &self,
        edge: &crate::model::EdgeSpec,
        issues: &mut Vec<GraphIssue>,
    ) {
        if edge.source.node_id != edge.destination.node_id {
            return;
        }
        let is_native_pass_through = self.nodes.get(&edge.source.node_id).is_some_and(|node| {
            matches!(
                node.config(),
                NodeConfig::VirtualEndpoint(_) | NodeConfig::VirtualEndpointTransport(_)
            ) && edge.source.port_id.as_str() == FROM_OS_PORT_ID
                && edge.destination.port_id.as_str() == TO_OS_PORT_ID
        });
        if !is_native_pass_through {
            issues.push(GraphIssue::InvalidSelfConnection {
                edge_id: edge.id.clone(),
                node_id: edge.source.node_id.clone(),
            });
        }
    }

    fn validate_id(&mut self, edge: &crate::model::EdgeSpec, issues: &mut Vec<GraphIssue>) -> bool {
        if edge.id.as_str().trim().is_empty() {
            issues.push(GraphIssue::InvalidEdgeId);
            false
        } else if !self.edge_ids.insert(edge.id.clone()) {
            issues.push(GraphIssue::DuplicateEdgeId {
                edge_id: edge.id.clone(),
            });
            false
        } else {
            true
        }
    }

    fn resolve_routes(
        edge: &crate::model::EdgeSpec,
        source: Option<&CompiledPort>,
        destination: Option<&CompiledPort>,
        issues: &mut Vec<GraphIssue>,
    ) -> Option<Vec<ChannelRoute>> {
        let (Some(source), Some(destination)) = (source, destination) else {
            return None;
        };
        compile_channel_mapping(
            &edge.id,
            source.channels(),
            destination.channels(),
            &edge.channel_mapping,
            issues,
        )
    }

    fn push(&mut self, edge: &crate::model::EdgeSpec, routes: Vec<ChannelRoute>) {
        if edge.source.node_id != edge.destination.node_id && self.edge_orders_processing(edge) {
            self.dependencies
                .entry(edge.source.node_id.clone())
                .or_default()
                .insert(edge.destination.node_id.clone());
        }
        self.edges.push(CompiledEdge::new(
            edge.id.clone(),
            edge.source.clone(),
            edge.destination.clone(),
            routes,
        ));
    }

    fn edge_orders_processing(&self, edge: &crate::model::EdgeSpec) -> bool {
        let source_is_external = self.nodes.get(&edge.source.node_id).is_some_and(|node| {
            matches!(
                node.config(),
                NodeConfig::ApplicationCapture(_)
                    | NodeConfig::AudioDeviceInput(_)
                    | NodeConfig::VirtualEndpointTransport(_)
                    | NodeConfig::VirtualEndpoint(_)
                    | NodeConfig::DeviceBus { .. }
            )
        });
        let destination_is_external =
            self.nodes
                .get(&edge.destination.node_id)
                .is_some_and(|node| {
                    matches!(
                        node.config(),
                        NodeConfig::AudioDeviceOutput(_)
                            | NodeConfig::Recorder { .. }
                            | NodeConfig::VirtualEndpointTransport(_)
                            | NodeConfig::VirtualEndpoint(_)
                            | NodeConfig::DeviceBus { .. }
                    )
                });
        !source_is_external && !destination_is_external
    }
}

fn resolve_port<'a>(
    nodes: &'a BTreeMap<NodeId, CompiledNode>,
    edge_id: &EdgeId,
    endpoint: &AudioEndpoint,
    expected_direction: PortDirection,
    issues: &mut Vec<GraphIssue>,
) -> Option<&'a CompiledPort> {
    let Some(node) = nodes.get(&endpoint.node_id) else {
        issues.push(GraphIssue::UnknownNode {
            edge_id: edge_id.clone(),
            node_id: endpoint.node_id.clone(),
        });
        return None;
    };

    let (expected_ports, opposite_ports) = match expected_direction {
        PortDirection::Input => (node.inputs(), node.outputs()),
        PortDirection::Output => (node.outputs(), node.inputs()),
    };
    if let Some(port) = expected_ports
        .iter()
        .find(|port| port.id() == &endpoint.port_id)
    {
        return Some(port);
    }
    if let Some(port) = opposite_ports
        .iter()
        .find(|port| port.id() == &endpoint.port_id)
    {
        issues.push(GraphIssue::InvalidPortDirection {
            edge_id: edge_id.clone(),
            endpoint: endpoint.clone(),
            expected: expected_direction,
            actual: port.direction(),
        });
        return None;
    }
    issues.push(GraphIssue::UnknownPort {
        edge_id: edge_id.clone(),
        endpoint: endpoint.clone(),
    });
    None
}

fn compile_channel_mapping(
    edge_id: &EdgeId,
    source_channels: u16,
    destination_channels: u16,
    mapping: &ChannelMapping,
    issues: &mut Vec<GraphIssue>,
) -> Option<Vec<ChannelRoute>> {
    match mapping {
        ChannelMapping::Auto if source_channels == destination_channels => Some(
            (0..source_channels)
                .map(|channel| ChannelRoute {
                    source_channel: channel,
                    destination_channel: channel,
                })
                .collect(),
        ),
        ChannelMapping::Auto => {
            issues.push(GraphIssue::ChannelCountMismatch {
                edge_id: edge_id.clone(),
                source_channels,
                destination_channels,
            });
            None
        }
        ChannelMapping::Explicit { routes } => {
            if routes.is_empty() {
                issues.push(GraphIssue::EmptyChannelMapping {
                    edge_id: edge_id.clone(),
                });
                return None;
            }

            let mut unique_routes = BTreeSet::new();
            for route in routes {
                if route.source_channel >= source_channels
                    || route.destination_channel >= destination_channels
                    || !unique_routes.insert((route.source_channel, route.destination_channel))
                {
                    issues.push(GraphIssue::InvalidChannelRoute {
                        edge_id: edge_id.clone(),
                        route: *route,
                        source_channels,
                        destination_channels,
                    });
                }
            }
            Some(routes.clone())
        }
    }
}

fn topological_order(
    nodes: &BTreeMap<NodeId, CompiledNode>,
    dependencies: &BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> Result<Vec<NodeId>, Vec<NodeId>> {
    let mut indegree: BTreeMap<NodeId, usize> =
        nodes.keys().cloned().map(|node_id| (node_id, 0)).collect();
    for destinations in dependencies.values() {
        for destination in destinations {
            *indegree
                .get_mut(destination)
                .expect("dependencies only reference resolved nodes") += 1;
        }
    }

    let mut ready: BTreeSet<NodeId> = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(node_id, _)| node_id.clone())
        .collect();
    let mut ordered = Vec::with_capacity(nodes.len());

    while let Some(node_id) = ready.pop_first() {
        ordered.push(node_id.clone());
        for destination in &dependencies[&node_id] {
            let count = indegree
                .get_mut(destination)
                .expect("dependencies only reference resolved nodes");
            *count -= 1;
            if *count == 0 {
                ready.insert(destination.clone());
            }
        }
    }

    if ordered.len() == nodes.len() {
        Ok(ordered)
    } else {
        Err(indegree
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(node_id, _)| node_id)
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum GraphIssue {
    UnsupportedSchemaVersion {
        expected: u32,
        found: u32,
    },
    InvalidRenderFormat {
        sample_rate: u32,
        block_frames: u32,
    },
    LimitExceeded {
        resource: GraphResource,
        limit: usize,
        actual: usize,
    },
    InvalidNodeId,
    DuplicateNodeId {
        node_id: NodeId,
    },
    NodeResolutionFailed {
        node_id: NodeId,
        reason: String,
    },
    InvalidEdgeId,
    DuplicateEdgeId {
        edge_id: EdgeId,
    },
    UnknownNode {
        edge_id: EdgeId,
        node_id: NodeId,
    },
    UnknownPort {
        edge_id: EdgeId,
        endpoint: AudioEndpoint,
    },
    InvalidPortDirection {
        edge_id: EdgeId,
        endpoint: AudioEndpoint,
        expected: PortDirection,
        actual: PortDirection,
    },
    InputAlreadyConnected {
        edge_id: EdgeId,
        endpoint: AudioEndpoint,
    },
    ChannelCountMismatch {
        edge_id: EdgeId,
        source_channels: u16,
        destination_channels: u16,
    },
    EmptyChannelMapping {
        edge_id: EdgeId,
    },
    InvalidChannelRoute {
        edge_id: EdgeId,
        route: ChannelRoute,
        source_channels: u16,
        destination_channels: u16,
    },
    InvalidSelfConnection {
        edge_id: EdgeId,
        node_id: NodeId,
    },
    DuplicateVirtualEndpointId {
        endpoint_id: String,
        first_node_id: NodeId,
        duplicate_node_id: NodeId,
    },
    CycleDetected {
        node_ids: Vec<NodeId>,
    },
}

impl GraphIssue {
    #[must_use]
    pub const fn code(&self) -> GraphIssueCode {
        match self {
            Self::UnsupportedSchemaVersion { .. } => GraphIssueCode::UnsupportedSchemaVersion,
            Self::InvalidRenderFormat { .. } => GraphIssueCode::InvalidRenderFormat,
            Self::LimitExceeded { .. } => GraphIssueCode::LimitExceeded,
            Self::InvalidNodeId => GraphIssueCode::InvalidNodeId,
            Self::DuplicateNodeId { .. } => GraphIssueCode::DuplicateNodeId,
            Self::NodeResolutionFailed { .. } => GraphIssueCode::NodeResolutionFailed,
            Self::InvalidEdgeId => GraphIssueCode::InvalidEdgeId,
            Self::DuplicateEdgeId { .. } => GraphIssueCode::DuplicateEdgeId,
            Self::UnknownNode { .. } => GraphIssueCode::UnknownNode,
            Self::UnknownPort { .. } => GraphIssueCode::UnknownPort,
            Self::InvalidPortDirection { .. } => GraphIssueCode::InvalidPortDirection,
            Self::InputAlreadyConnected { .. } => GraphIssueCode::InputAlreadyConnected,
            Self::ChannelCountMismatch { .. } => GraphIssueCode::ChannelCountMismatch,
            Self::EmptyChannelMapping { .. } => GraphIssueCode::EmptyChannelMapping,
            Self::InvalidChannelRoute { .. } => GraphIssueCode::InvalidChannelRoute,
            Self::InvalidSelfConnection { .. } => GraphIssueCode::InvalidSelfConnection,
            Self::DuplicateVirtualEndpointId { .. } => GraphIssueCode::DuplicateVirtualEndpointId,
            Self::CycleDetected { .. } => GraphIssueCode::CycleDetected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphIssueCode {
    UnsupportedSchemaVersion,
    InvalidRenderFormat,
    LimitExceeded,
    InvalidNodeId,
    DuplicateNodeId,
    NodeResolutionFailed,
    InvalidEdgeId,
    DuplicateEdgeId,
    UnknownNode,
    UnknownPort,
    InvalidPortDirection,
    InputAlreadyConnected,
    ChannelCountMismatch,
    EmptyChannelMapping,
    InvalidChannelRoute,
    InvalidSelfConnection,
    DuplicateVirtualEndpointId,
    CycleDetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphResource {
    Nodes,
    Edges,
    PlanBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    issues: Vec<GraphIssue>,
}

impl CompileError {
    fn new(issues: Vec<GraphIssue>) -> Self {
        Self { issues }
    }

    #[must_use]
    pub fn issues(&self) -> &[GraphIssue] {
        &self.issues
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "graph compilation failed with {} issue(s)",
            self.issues.len()
        )
    }
}

impl std::error::Error for CompileError {}
