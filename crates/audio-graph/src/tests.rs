use crate::{
    AUDIO_PORT_ID, ApplicationCaptureConfig, ApplicationSelector, AudioDeviceInputConfig,
    AudioDeviceOutputConfig, AudioEndpoint, BuiltInNodeResolver, ChannelMapping, ChannelRoute,
    CompileContext, CompileError, CompileLimits, CompiledPort, DeviceSelector, EdgeId, EdgeSpec,
    GainConfig, GraphCompiler, GraphDocument, GraphIssue, GraphIssueCode, GraphResource,
    MixerConfig, NodeConfig, NodeId, NodeKind, NodeResolutionError, NodeResolver, NodeSpec,
    NodeTypeId, PortDirection, PortId, ProviderNodeConfig, RenderFormat, ResolvedNode,
    VirtualDeviceConfig,
};

fn context() -> CompileContext {
    CompileContext {
        format: RenderFormat {
            sample_rate: 96_000,
            block_frames: 256,
        },
        limits: CompileLimits::default(),
    }
}

fn endpoint(node_id: &str) -> AudioEndpoint {
    AudioEndpoint {
        node_id: NodeId::from(node_id),
        port_id: PortId::from(AUDIO_PORT_ID),
    }
}

fn gain_node(id: &str) -> NodeSpec {
    NodeSpec {
        id: NodeId::from(id),
        label: None,
        config: NodeConfig::Gain(GainConfig {
            channels: 2,
            gain_db: 0.0,
        }),
    }
}

fn configured_node(id: &str, config: NodeConfig) -> NodeSpec {
    NodeSpec {
        id: NodeId::from(id),
        label: None,
        config,
    }
}

fn edge(id: &str, source: &str, destination: &str) -> EdgeSpec {
    EdgeSpec {
        id: EdgeId::from(id),
        source: endpoint(source),
        destination: endpoint(destination),
        channel_mapping: ChannelMapping::Auto,
    }
}

fn compile_error(document: &GraphDocument) -> CompileError {
    GraphCompiler::default()
        .compile(document, context())
        .expect_err("graph should be rejected")
}

fn issue_codes(error: &CompileError) -> Vec<GraphIssueCode> {
    error.issues().iter().map(GraphIssue::code).collect()
}

#[test]
fn compiles_fixture_into_a_deterministic_plan() {
    let document: GraphDocument = serde_json::from_str(include_str!("fixtures/stereo-stream.json"))
        .expect("fixture should deserialize");

    let plan = GraphCompiler::default()
        .compile(&document, context())
        .expect("fixture should compile");

    assert_eq!(plan.revision(), 7);
    assert_eq!(plan.format().sample_rate, 96_000);
    assert_eq!(plan.format().block_frames, 256);
    assert_eq!(plan.edges().len(), 4);
    assert!(plan.edges().iter().all(|edge| edge.routes().len() == 2));

    let first_edge = &plan.edges()[0];
    assert_eq!(first_edge.id().as_str(), "chrome-to-mix");
    assert_eq!(first_edge.source().node_id.as_str(), "chrome");
    assert_eq!(first_edge.destination().node_id.as_str(), "broadcast-mix");

    let ordered_ids: Vec<_> = plan.nodes().iter().map(|node| node.id().as_str()).collect();
    assert_eq!(
        ordered_ids,
        [
            "chrome",
            "microphone",
            "broadcast-mix",
            "monitor",
            "tonenari-stream"
        ]
    );
    assert_eq!(plan.nodes()[0].kind(), NodeKind::ApplicationCapture);
    assert!(plan.nodes()[0].inputs().is_empty());
    assert_eq!(
        plan.nodes()[0].outputs()[0].direction(),
        PortDirection::Output
    );
}

#[test]
fn rejects_cycles_before_the_runtime_sees_them() {
    let document = GraphDocument::new(
        1,
        vec![gain_node("first"), gain_node("second")],
        vec![
            edge("first-to-second", "first", "second"),
            edge("second-to-first", "second", "first"),
        ],
    );

    let error = GraphCompiler::default()
        .compile(&document, context())
        .expect_err("cycle must be rejected");

    assert!(matches!(
        error.issues(),
        [GraphIssue::CycleDetected { node_ids }] if node_ids == &vec![NodeId::from("first"), NodeId::from("second")]
    ));
}

#[test]
fn reports_stable_machine_readable_issue_codes() {
    let document = GraphDocument::new(
        1,
        vec![gain_node("source"), gain_node("destination")],
        vec![EdgeSpec {
            id: EdgeId::from("bad-direction"),
            source: endpoint("source"),
            destination: AudioEndpoint {
                node_id: NodeId::from("destination"),
                port_id: PortId::from("missing"),
            },
            channel_mapping: ChannelMapping::Auto,
        }],
    );

    let error = GraphCompiler::default()
        .compile(&document, context())
        .expect_err("unknown port must be rejected");
    assert_eq!(error.issues()[0].code(), GraphIssueCode::UnknownPort);

    let json = serde_json::to_value(&error.issues()[0]).expect("issue should serialize");
    assert_eq!(json["code"], "unknown_port");
}

#[test]
fn built_in_resolver_supports_dynamic_selectors_and_all_node_kinds() {
    let configurations = [
        NodeConfig::ApplicationCapture(ApplicationCaptureConfig {
            selector: ApplicationSelector::ExecutablePath {
                path: "/Applications/Firefox.app/Contents/MacOS/firefox".into(),
            },
            output_channels: 2,
        }),
        NodeConfig::ApplicationCapture(ApplicationCaptureConfig {
            selector: ApplicationSelector::AllExcept {
                bundle_ids: vec!["com.apple.Music".into()],
            },
            output_channels: 2,
        }),
        NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: "mic-uid".into(),
            },
            output_channels: 1,
        }),
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::DefaultInput,
            input_channels: 2,
        }),
        NodeConfig::Gain(GainConfig {
            channels: 2,
            gain_db: -6.0,
        }),
    ];

    for (index, config) in configurations.into_iter().enumerate() {
        let resolved = BuiltInNodeResolver
            .resolve(
                &configured_node(&format!("node-{index}"), config),
                &context(),
            )
            .expect("configuration should resolve");
        assert!((1..=2).contains(&(resolved.inputs.len() + resolved.outputs.len())));
    }
}

#[test]
fn built_in_resolver_rejects_every_invalid_configuration_class() {
    let invalid = [
        app(ApplicationSelector::BundleId {
            bundle_id: " ".into(),
            include_descendants: true,
        }),
        NodeConfig::ApplicationCapture(ApplicationCaptureConfig {
            selector: ApplicationSelector::AllExcept { bundle_ids: vec![] },
            output_channels: 0,
        }),
        app(ApplicationSelector::ExecutablePath {
            path: String::new(),
        }),
        app(ApplicationSelector::AllExcept {
            bundle_ids: vec![String::new()],
        }),
        device_input(DeviceSelector::PersistentUid { uid: String::new() }, 2),
        device_input(DeviceSelector::DefaultInput, 0),
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::DefaultOutput,
            input_channels: 0,
        }),
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::PersistentUid { uid: String::new() },
            input_channels: 2,
        }),
        mixer(vec![], 2),
        mixer(vec![PortId::from("input")], 0),
        mixer(vec![PortId::from("")], 2),
        mixer(vec![PortId::from("one"), PortId::from("one")], 2),
        gain(0, 0.0),
        gain(2, f32::INFINITY),
        virtual_device("", "ToneNari", 2),
        virtual_device("uid", "", 2),
        virtual_device("uid", "ToneNari", 65),
    ];

    for (index, config) in invalid.into_iter().enumerate() {
        let error = BuiltInNodeResolver
            .resolve(
                &configured_node(&format!("invalid-{index}"), config),
                &context(),
            )
            .expect_err("configuration should be rejected");
        assert!(matches!(
            error,
            NodeResolutionError::InvalidConfiguration { .. }
        ));
    }
}

fn app(selector: ApplicationSelector) -> NodeConfig {
    NodeConfig::ApplicationCapture(ApplicationCaptureConfig {
        selector,
        output_channels: 2,
    })
}

fn device_input(selector: DeviceSelector, output_channels: u16) -> NodeConfig {
    NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
        selector,
        output_channels,
    })
}

fn mixer(inputs: Vec<PortId>, channels: u16) -> NodeConfig {
    NodeConfig::Mixer(MixerConfig { inputs, channels })
}

fn gain(channels: u16, gain_db: f32) -> NodeConfig {
    NodeConfig::Gain(GainConfig { channels, gain_db })
}

fn virtual_device(uid: &str, name: &str, channels: u16) -> NodeConfig {
    NodeConfig::VirtualDevice(VirtualDeviceConfig {
        persistent_uid: uid.into(),
        name: name.into(),
        channels,
    })
}

fn provider(type_id: &str) -> NodeConfig {
    NodeConfig::Provider(ProviderNodeConfig {
        type_id: NodeTypeId::from(type_id),
        parameters: serde_json::json!({ "thresholdDb": -12.0 }),
    })
}

#[test]
fn document_validation_aggregates_schema_format_limit_and_id_issues() {
    let mut document = GraphDocument::new(
        1,
        vec![gain_node(""), gain_node("same"), gain_node("same")],
        vec![edge("", "missing", "missing")],
    );
    document.schema_version = 99;
    let invalid_context = CompileContext {
        format: RenderFormat {
            sample_rate: 0,
            block_frames: 0,
        },
        limits: CompileLimits {
            max_nodes: 0,
            max_edges: 0,
            max_channels_per_port: 64,
        },
    };

    let error = GraphCompiler::default()
        .compile(&document, invalid_context)
        .expect_err("invalid document should fail");
    let codes = issue_codes(&error);
    for expected in [
        GraphIssueCode::UnsupportedSchemaVersion,
        GraphIssueCode::InvalidRenderFormat,
        GraphIssueCode::LimitExceeded,
        GraphIssueCode::InvalidNodeId,
        GraphIssueCode::DuplicateNodeId,
        GraphIssueCode::InvalidEdgeId,
    ] {
        assert!(codes.contains(&expected), "missing {expected:?}");
    }
    assert_eq!(
        error.to_string(),
        "graph compilation failed with 7 issue(s)"
    );
}

struct UnavailableResolver;

impl NodeResolver for UnavailableResolver {
    fn resolve(
        &self,
        _node: &NodeSpec,
        _context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        Err(NodeResolutionError::CapabilityUnavailable {
            reason: "process tap permission denied".into(),
        })
    }
}

#[test]
fn custom_resolver_failure_is_reported_without_coupling_the_compiler() {
    let document = GraphDocument::new(1, vec![gain_node("capture")], vec![]);
    let error = GraphCompiler::new(UnavailableResolver)
        .compile(&document, context())
        .expect_err("unavailable capability should fail");

    assert!(matches!(
        error.issues(),
        [GraphIssue::NodeResolutionFailed { node_id, reason }]
            if node_id.as_str() == "capture" && reason.contains("permission denied")
    ));
}

#[test]
fn edge_validation_reports_ids_nodes_ports_directions_and_fan_in() {
    let nodes = vec![
        gain_node("source-a"),
        gain_node("source-b"),
        gain_node("destination"),
        configured_node(
            "sink",
            NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                selector: DeviceSelector::DefaultOutput,
                input_channels: 2,
            }),
        ),
        configured_node(
            "capture",
            app(ApplicationSelector::AllExcept { bundle_ids: vec![] }),
        ),
    ];
    let edges = vec![
        edge("", "source-a", "destination"),
        edge("duplicate", "source-a", "destination"),
        edge("duplicate", "source-b", "destination"),
        edge("unknown-nodes", "missing-a", "missing-b"),
        edge("wrong-direction", "sink", "capture"),
        edge("unknown-port", "source-a", "destination").with_destination_port("missing"),
        edge("fan-in", "source-b", "destination"),
    ];
    let codes = issue_codes(&compile_error(&GraphDocument::new(1, nodes, edges)));

    for expected in [
        GraphIssueCode::InvalidEdgeId,
        GraphIssueCode::DuplicateEdgeId,
        GraphIssueCode::UnknownNode,
        GraphIssueCode::InvalidPortDirection,
        GraphIssueCode::UnknownPort,
        GraphIssueCode::InputAlreadyConnected,
    ] {
        assert!(codes.contains(&expected), "missing {expected:?}");
    }
}

trait EdgeTestExt {
    fn with_destination_port(self, port_id: &str) -> Self;
}

impl EdgeTestExt for EdgeSpec {
    fn with_destination_port(mut self, port_id: &str) -> Self {
        self.destination.port_id = PortId::from(port_id);
        self
    }
}

#[test]
fn channel_mapping_supports_explicit_routes_and_rejects_invalid_mappings() {
    let valid_mapping = ChannelMapping::Explicit {
        routes: vec![ChannelRoute {
            source_channel: 1,
            destination_channel: 0,
        }],
    };
    let valid = GraphDocument::new(
        1,
        vec![gain_node("source"), gain_node("destination")],
        vec![EdgeSpec {
            channel_mapping: valid_mapping,
            ..edge("mapped", "source", "destination")
        }],
    );
    assert_eq!(
        GraphCompiler::default()
            .compile(&valid, context())
            .expect("explicit mapping should compile")
            .edges()[0]
            .routes(),
        [ChannelRoute {
            source_channel: 1,
            destination_channel: 0,
        }]
    );

    assert_invalid_mapping(
        ChannelMapping::Auto,
        1,
        2,
        GraphIssueCode::ChannelCountMismatch,
    );
    assert_invalid_mapping(
        ChannelMapping::Explicit { routes: vec![] },
        2,
        2,
        GraphIssueCode::EmptyChannelMapping,
    );
    let bad_routes = vec![
        ChannelRoute {
            source_channel: 2,
            destination_channel: 0,
        },
        ChannelRoute {
            source_channel: 0,
            destination_channel: 2,
        },
        ChannelRoute {
            source_channel: 0,
            destination_channel: 0,
        },
        ChannelRoute {
            source_channel: 0,
            destination_channel: 0,
        },
    ];
    assert_invalid_mapping(
        ChannelMapping::Explicit { routes: bad_routes },
        2,
        2,
        GraphIssueCode::InvalidChannelRoute,
    );
}

fn assert_invalid_mapping(
    mapping: ChannelMapping,
    source_channels: u16,
    destination_channels: u16,
    expected: GraphIssueCode,
) {
    let document = GraphDocument::new(
        1,
        vec![
            configured_node("source", gain(source_channels, 0.0)),
            configured_node("destination", gain(destination_channels, 0.0)),
        ],
        vec![EdgeSpec {
            channel_mapping: mapping,
            ..edge("mapping", "source", "destination")
        }],
    );
    assert!(issue_codes(&compile_error(&document)).contains(&expected));
}

#[test]
fn every_issue_exposes_one_typed_consistent_code() {
    let endpoint = endpoint("node");
    let edge_id = EdgeId::from("edge");
    let issues = all_issue_variants(&endpoint, edge_id);
    let actual: Vec<_> = issues.iter().map(GraphIssue::code).collect();
    assert_eq!(actual, all_issue_codes());

    let encoded =
        serde_json::to_string(&GraphIssueCode::UnknownPort).expect("issue code should serialize");
    assert_eq!(encoded, "\"unknown_port\"");
}

fn all_issue_codes() -> Vec<GraphIssueCode> {
    vec![
        GraphIssueCode::UnsupportedSchemaVersion,
        GraphIssueCode::InvalidRenderFormat,
        GraphIssueCode::LimitExceeded,
        GraphIssueCode::InvalidNodeId,
        GraphIssueCode::DuplicateNodeId,
        GraphIssueCode::NodeResolutionFailed,
        GraphIssueCode::InvalidEdgeId,
        GraphIssueCode::DuplicateEdgeId,
        GraphIssueCode::UnknownNode,
        GraphIssueCode::UnknownPort,
        GraphIssueCode::InvalidPortDirection,
        GraphIssueCode::InputAlreadyConnected,
        GraphIssueCode::ChannelCountMismatch,
        GraphIssueCode::EmptyChannelMapping,
        GraphIssueCode::InvalidChannelRoute,
        GraphIssueCode::CycleDetected,
    ]
}

fn all_issue_variants(endpoint: &AudioEndpoint, edge_id: EdgeId) -> Vec<GraphIssue> {
    vec![
        GraphIssue::UnsupportedSchemaVersion {
            expected: 1,
            found: 2,
        },
        GraphIssue::InvalidRenderFormat {
            sample_rate: 0,
            block_frames: 0,
        },
        GraphIssue::LimitExceeded {
            resource: GraphResource::Nodes,
            limit: 0,
            actual: 1,
        },
        GraphIssue::InvalidNodeId,
        GraphIssue::DuplicateNodeId {
            node_id: NodeId::from("node"),
        },
        GraphIssue::NodeResolutionFailed {
            node_id: NodeId::from("node"),
            reason: "reason".into(),
        },
        GraphIssue::InvalidEdgeId,
        GraphIssue::DuplicateEdgeId {
            edge_id: edge_id.clone(),
        },
        GraphIssue::UnknownNode {
            edge_id: edge_id.clone(),
            node_id: NodeId::from("node"),
        },
        GraphIssue::UnknownPort {
            edge_id: edge_id.clone(),
            endpoint: endpoint.clone(),
        },
        GraphIssue::InvalidPortDirection {
            edge_id: edge_id.clone(),
            endpoint: endpoint.clone(),
            expected: PortDirection::Input,
            actual: PortDirection::Output,
        },
        GraphIssue::InputAlreadyConnected {
            edge_id: edge_id.clone(),
            endpoint: endpoint.clone(),
        },
        GraphIssue::ChannelCountMismatch {
            edge_id: edge_id.clone(),
            source_channels: 1,
            destination_channels: 2,
        },
        GraphIssue::EmptyChannelMapping {
            edge_id: edge_id.clone(),
        },
        GraphIssue::InvalidChannelRoute {
            edge_id,
            route: ChannelRoute {
                source_channel: 2,
                destination_channel: 2,
            },
            source_channels: 2,
            destination_channels: 2,
        },
        GraphIssue::CycleDetected {
            node_ids: vec![NodeId::from("node")],
        },
    ]
}

#[test]
fn model_helpers_preserve_typed_ids_ports_kinds_and_defaults() {
    assert_eq!(NodeId::new("node").to_string(), "node");
    assert_eq!(EdgeId::new("edge").to_string(), "edge");
    assert_eq!(PortId::new("port").to_string(), "port");
    assert_eq!(
        NodeTypeId::new("io.tonenari.compressor").to_string(),
        "io.tonenari.compressor"
    );

    let selector: ApplicationSelector = serde_json::from_value(serde_json::json!({
        "match": "bundle_id",
        "bundle_id": "org.mozilla.firefox"
    }))
    .expect("selector should deserialize");
    assert!(matches!(
        selector,
        ApplicationSelector::BundleId {
            include_descendants: true,
            ..
        }
    ));

    let port = CompiledPort::new(PortId::from(AUDIO_PORT_ID), PortDirection::Output, 6);
    assert_eq!(port.id().as_str(), AUDIO_PORT_ID);
    assert_eq!(port.direction(), PortDirection::Output);
    assert_eq!(port.channels(), 6);

    let kinds = [
        app(ApplicationSelector::AllExcept { bundle_ids: vec![] }),
        device_input(DeviceSelector::DefaultInput, 2),
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::DefaultOutput,
            input_channels: 2,
        }),
        mixer(vec![PortId::from("input")], 2),
        gain(2, 0.0),
        virtual_device("uid", "name", 2),
        provider("io.tonenari.compressor"),
    ];
    let actual: Vec<_> = kinds.iter().map(NodeConfig::kind).collect();
    assert_eq!(
        actual,
        [
            NodeKind::ApplicationCapture,
            NodeKind::AudioDeviceInput,
            NodeKind::AudioDeviceOutput,
            NodeKind::Mixer,
            NodeKind::Gain,
            NodeKind::VirtualDevice,
            NodeKind::Provider
        ]
    );
}

struct TestProviderResolver;

impl NodeResolver for TestProviderResolver {
    fn resolve(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        if matches!(node.config, NodeConfig::Provider(_)) {
            Ok(ResolvedNode {
                inputs: vec![CompiledPort::new(
                    PortId::from(AUDIO_PORT_ID),
                    PortDirection::Input,
                    2,
                )],
                outputs: vec![CompiledPort::new(
                    PortId::from(AUDIO_PORT_ID),
                    PortDirection::Output,
                    2,
                )],
            })
        } else {
            BuiltInNodeResolver.resolve(node, context)
        }
    }
}

#[test]
fn provider_nodes_extend_the_graph_without_changing_the_compiler() {
    let provider_node = configured_node("compressor", provider("io.tonenari.compressor"));
    let unavailable = BuiltInNodeResolver
        .resolve(&provider_node, &context())
        .expect_err("unregistered provider must be explicit");
    assert!(matches!(
        unavailable,
        NodeResolutionError::CapabilityUnavailable { reason }
            if reason.contains("io.tonenari.compressor")
    ));

    let encoded = serde_json::to_value(&provider_node).expect("provider node should serialize");
    assert_eq!(encoded["type"], "provider");
    assert_eq!(encoded["config"]["parameters"]["thresholdDb"], -12.0);

    let document = GraphDocument::new(1, vec![provider_node], vec![]);
    let plan = GraphCompiler::new(TestProviderResolver)
        .compile(&document, context())
        .expect("registered provider should compile");
    assert_eq!(plan.nodes()[0].kind(), NodeKind::Provider);

    let built_in = TestProviderResolver
        .resolve(&gain_node("gain"), &context())
        .expect("provider resolver should compose with built-ins");
    assert_eq!(built_in.inputs.len(), 1);
}
