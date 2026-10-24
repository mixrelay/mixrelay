use crate::{
    AUDIO_PORT_ID, ApplicationCaptureRequest, ApplicationSelector, AudioDeviceInputConfig,
    AudioDeviceOutputConfig, AudioEndpoint, BuiltInNodeResolver, ChannelLayout, ChannelMapping,
    ChannelRoute, CompileContext, CompileError, CompileLimits, CompiledPort, DeviceSelector,
    EdgeId, EdgeSpec, FROM_OS_PORT_ID, GainConfig, GraphCompiler, GraphDocument, GraphIssue,
    GraphIssueCode, GraphResource, MixerConfig, NodeConfig, NodeId, NodeKind, NodeResolutionError,
    NodeResolver, NodeSpec, NodeTypeId, PortDirection, PortId, ProviderNodeConfig, RenderFormat,
    ResolvedNode, SourcePlaybackPolicy, StreamSpec, TO_OS_PORT_ID, VirtualEndpointId,
    VirtualEndpointReference, VirtualEndpointSpec, validate_virtual_endpoint_gain,
};
use mixrelay_audio_io::{
    ApplicationCaptureCapabilities, ApplicationSelectorKind, VirtualEndpointCapabilities,
};
use std::collections::BTreeSet;

mod dormant_provider_tests;

fn context() -> CompileContext {
    CompileContext {
        format: RenderFormat {
            sample_rate: 96_000,
            block_frames: 256,
        },
        limits: CompileLimits::default(),
    }
}

#[test]
fn installed_stereo_acceptance_uses_a_real_source_reference_and_recording_route() {
    let document: GraphDocument =
        serde_json::from_str(include_str!("fixtures/qualification-stereo.json")).unwrap();
    let plan = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format: RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 128,
                },
                ..context()
            },
        )
        .unwrap();
    let source = plan
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == "qa-source")
        .unwrap();
    assert!(source.inputs().is_empty());
    assert_eq!(source.outputs()[0].id().as_str(), FROM_OS_PORT_ID);
    assert_eq!(source.outputs()[0].channels(), 2);
    let recorder = plan
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == "qa-recorder")
        .unwrap();
    assert_eq!(recorder.inputs()[0].channels(), 2);
}

#[test]
fn node_control_defaults_round_trip_and_survives_compilation() {
    let mut document: GraphDocument =
        serde_json::from_str(include_str!("fixtures/stereo-stream.json")).unwrap();
    assert!(
        document
            .nodes
            .iter()
            .all(|node| node.control == crate::NodeControl::default())
    );
    document.nodes[0].control = crate::NodeControl {
        enabled: false,
        muted: true,
    };
    let encoded = serde_json::to_string(&document).unwrap();
    assert_eq!(
        serde_json::from_str::<GraphDocument>(&encoded).unwrap(),
        document
    );
    let plan = GraphCompiler::new(BuiltInNodeResolver)
        .compile(&document, context())
        .unwrap();
    let compiled = plan
        .nodes()
        .iter()
        .find(|node| node.id() == &document.nodes[0].id)
        .unwrap();
    assert_eq!(compiled.control(), document.nodes[0].control);
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
        control: crate::NodeControl::default(),
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
        control: crate::NodeControl::default(),
        config,
    }
}

#[test]
fn recorder_resolves_one_to_eight_input_channels_without_native_output_ports() {
    for channels in 1..=mixrelay_audio_io::formats::MAX_RECORDING_CHANNELS {
        let node = configured_node("recorder", NodeConfig::Recorder { channels });
        let resolved = BuiltInNodeResolver.resolve(&node, &context()).unwrap();
        assert!(resolved.outputs.is_empty());
        assert_eq!(resolved.inputs.len(), 1);
        assert_eq!(resolved.inputs[0].channels(), channels);
    }
    for channels in [0, 9, 64] {
        let node = configured_node("recorder", NodeConfig::Recorder { channels });
        assert!(BuiltInNodeResolver.resolve(&node, &context()).is_err());
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
    assert_eq!(plan.edges().len(), 5);
    let owned_memory_bytes = plan.owned_memory_bytes();
    assert!(owned_memory_bytes > 0);
    eprintln!("fixture plan reserved bytes: {owned_memory_bytes}");
    assert!(plan.edges().iter().all(|edge| edge.routes().len() == 2));

    let first_edge = &plan.edges()[0];
    assert_eq!(first_edge.id().as_str(), "spat-to-mix");
    assert_eq!(first_edge.source().node_id.as_str(), "spat-ingress");
    assert_eq!(first_edge.source().port_id.as_str(), FROM_OS_PORT_ID);
    assert_eq!(first_edge.destination().node_id.as_str(), "broadcast-mix");

    let ordered_ids: Vec<_> = plan.nodes().iter().map(|node| node.id().as_str()).collect();
    assert_eq!(
        ordered_ids,
        [
            "broadcast-mix",
            "chrome",
            "microphone",
            "mixrelay-stream",
            "monitor",
            "spat-ingress"
        ]
    );
    let chrome = plan
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == "chrome")
        .expect("Chrome source should compile");
    assert_eq!(chrome.kind(), NodeKind::ApplicationCapture);
    assert!(matches!(chrome.config(), NodeConfig::ApplicationCapture(_)));
    assert!(chrome.inputs().is_empty());
    assert_eq!(chrome.outputs()[0].direction(), PortDirection::Output);
    let spat = plan
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == "spat-ingress")
        .expect("SPAT endpoint should compile");
    assert_eq!(spat.outputs()[0].id().as_str(), FROM_OS_PORT_ID);
    assert_eq!(spat.outputs()[0].channels(), 64);
    let obs = plan
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == "mixrelay-stream")
        .expect("OBS endpoint should compile");
    assert_eq!(obs.inputs()[0].id().as_str(), TO_OS_PORT_ID);
    assert_eq!(obs.inputs()[0].channels(), 2);
}

#[test]
fn rejects_cycles_before_the_runtime_sees_them() {
    let acyclic = GraphCompiler::default()
        .compile(
            &GraphDocument::new(
                1,
                vec![gain_node("first"), gain_node("second")],
                vec![edge("first-to-second", "first", "second")],
            ),
            context(),
        )
        .expect("processor dependency should order the destination");
    assert_eq!(
        acyclic
            .nodes()
            .iter()
            .map(|node| node.id().as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let converging = GraphCompiler::default()
        .compile(
            &GraphDocument::new(
                2,
                vec![
                    gain_node("first"),
                    gain_node("second"),
                    configured_node(
                        "mix",
                        NodeConfig::Mixer(MixerConfig {
                            inputs: vec![PortId::from("left"), PortId::from("right")],
                            channels: 2,
                        }),
                    ),
                ],
                vec![
                    EdgeSpec {
                        id: EdgeId::from("first-to-left"),
                        source: endpoint("first"),
                        destination: AudioEndpoint {
                            node_id: NodeId::from("mix"),
                            port_id: PortId::from("left"),
                        },
                        channel_mapping: ChannelMapping::Auto,
                    },
                    EdgeSpec {
                        id: EdgeId::from("second-to-right"),
                        source: endpoint("second"),
                        destination: AudioEndpoint {
                            node_id: NodeId::from("mix"),
                            port_id: PortId::from("right"),
                        },
                        channel_mapping: ChannelMapping::Auto,
                    },
                ],
            ),
            context(),
        )
        .expect("multiple processor dependencies should converge");
    assert_eq!(
        converging.nodes().last().map(|node| node.id().as_str()),
        Some("mix")
    );

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
fn allows_only_virtual_endpoint_pass_through_as_a_self_connection() {
    let endpoint_node = configured_node(
        "virtual",
        virtual_endpoint("virtual", "Virtual", Some(2), Some(2)),
    );
    let pass_through = EdgeSpec {
        id: EdgeId::from("pass-through"),
        source: AudioEndpoint {
            node_id: NodeId::from("virtual"),
            port_id: PortId::from(FROM_OS_PORT_ID),
        },
        destination: AudioEndpoint {
            node_id: NodeId::from("virtual"),
            port_id: PortId::from(TO_OS_PORT_ID),
        },
        channel_mapping: ChannelMapping::Auto,
    };
    let plan = GraphCompiler::default()
        .compile(
            &GraphDocument::new(1, vec![endpoint_node], vec![pass_through]),
            context(),
        )
        .expect("virtual endpoint pass-through is not a feedback cycle");
    assert_eq!(plan.edges().len(), 1);

    let gain_path = GraphDocument::new(
        2,
        vec![
            configured_node(
                "duplex",
                virtual_endpoint("duplex", "Duplex", Some(2), Some(2)),
            ),
            gain_node("gain"),
        ],
        vec![
            EdgeSpec {
                id: EdgeId::from("from-os-to-gain"),
                source: AudioEndpoint {
                    node_id: NodeId::from("duplex"),
                    port_id: PortId::from(FROM_OS_PORT_ID),
                },
                destination: endpoint("gain"),
                channel_mapping: ChannelMapping::Auto,
            },
            EdgeSpec {
                id: EdgeId::from("gain-to-os"),
                source: endpoint("gain"),
                destination: AudioEndpoint {
                    node_id: NodeId::from("duplex"),
                    port_id: PortId::from(TO_OS_PORT_ID),
                },
                channel_mapping: ChannelMapping::Auto,
            },
        ],
    );
    assert!(
        GraphCompiler::default()
            .compile(&gain_path, context())
            .is_ok()
    );

    let error = compile_error(&GraphDocument::new(
        1,
        vec![gain_node("gain")],
        vec![edge("feedback", "gain", "gain")],
    ));
    assert!(matches!(
        error.issues(),
        [GraphIssue::InvalidSelfConnection { edge_id, node_id }]
            if edge_id.as_str() == "feedback" && node_id.as_str() == "gain"
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
        app(ApplicationSelector::ExecutablePath {
            path: "/Applications/Firefox.app/Contents/MacOS/firefox".into(),
        }),
        app(ApplicationSelector::AllExcept {
            bundle_ids: vec!["com.apple.Music".into()],
        }),
        NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: "mic-uid".into(),
            },
            output_channels: 1,
            gain_db: 0.0,
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
        app_with_channels(ApplicationSelector::AllExcept { bundle_ids: vec![] }, 0),
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
        virtual_endpoint("", "MixRelay", None, Some(2)),
        virtual_endpoint("uid", "", None, Some(2)),
        virtual_endpoint("uid", "MixRelay", None, Some(65)),
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
    app_with_channels(selector, 2)
}

fn app_with_channels(selector: ApplicationSelector, channels: u16) -> NodeConfig {
    NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
        selector,
        stream: StreamSpec {
            channels,
            channel_layout: if channels == 1 {
                ChannelLayout::Mono
            } else {
                ChannelLayout::Discrete
            },
        },
        source_playback: SourcePlaybackPolicy::Preserve,
        forwarding_endpoint: None,
        gain_db: 0.0,
        requires_reselection: false,
    })
}

fn device_input(selector: DeviceSelector, output_channels: u16) -> NodeConfig {
    NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
        selector,
        output_channels,
        gain_db: 0.0,
    })
}

fn mixer(inputs: Vec<PortId>, channels: u16) -> NodeConfig {
    NodeConfig::Mixer(MixerConfig { inputs, channels })
}

fn gain(channels: u16, gain_db: f32) -> NodeConfig {
    NodeConfig::Gain(GainConfig { channels, gain_db })
}

fn virtual_endpoint(
    id: &str,
    display_name: &str,
    render_channels: Option<u16>,
    capture_channels: Option<u16>,
) -> NodeConfig {
    NodeConfig::VirtualEndpoint(
        VirtualEndpointSpec {
            id: VirtualEndpointId::from(id),
            display_name: display_name.into(),
            render: render_channels.map(discrete_stream),
            capture: capture_channels.map(discrete_stream),
            supported_sample_rates: vec![48_000, 96_000],
            default_sample_rate: 48_000,
        }
        .into(),
    )
}

const fn discrete_stream(channels: u16) -> StreamSpec {
    StreamSpec {
        channels,
        channel_layout: ChannelLayout::Discrete,
    }
}

fn provider(type_id: &str) -> NodeConfig {
    NodeConfig::Provider(ProviderNodeConfig {
        ports: None,
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
            max_sample_rate: 384_000,
            max_block_frames: 4_096,
            max_plan_bytes: 8 * 1_024 * 1_024,
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

#[test]
fn rejects_unbounded_render_formats_and_compiled_plan_memory() {
    for format in [
        RenderFormat {
            sample_rate: 0,
            block_frames: 128,
        },
        RenderFormat {
            sample_rate: 384_001,
            block_frames: 128,
        },
        RenderFormat {
            sample_rate: 48_000,
            block_frames: 0,
        },
        RenderFormat {
            sample_rate: 48_000,
            block_frames: 4_097,
        },
    ] {
        let invalid_context = CompileContext {
            format,
            limits: CompileLimits::default(),
        };
        let error = GraphCompiler::default()
            .compile(&GraphDocument::new(1, vec![], vec![]), invalid_context)
            .expect_err("an unbounded render format must fail");
        assert_eq!(
            error.issues()[0].code(),
            GraphIssueCode::InvalidRenderFormat
        );
    }

    let mut memory_limited_context = context();
    memory_limited_context.limits.max_plan_bytes = 0;
    let error = GraphCompiler::default()
        .compile(
            &GraphDocument::new(1, vec![], vec![]),
            memory_limited_context,
        )
        .expect_err("a plan above its memory budget must fail");
    assert!(matches!(
        error.issues(),
        [GraphIssue::LimitExceeded {
            resource: GraphResource::PlanBytes,
            limit: 0,
            actual,
        }] if *actual > 0
    ));
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
fn edge_validation_reports_ids_nodes_ports_and_directions_while_allowing_fan_in() {
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
    ] {
        assert!(codes.contains(&expected), "missing {expected:?}");
    }
    assert!(!codes.contains(&GraphIssueCode::InputAlreadyConnected));
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
        GraphIssueCode::InvalidSelfConnection,
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
        GraphIssue::InvalidSelfConnection {
            edge_id: EdgeId::from("edge"),
            node_id: NodeId::from("node"),
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
        NodeTypeId::new("io.mixrelay.compressor").to_string(),
        "io.mixrelay.compressor"
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
        virtual_endpoint("uid", "name", Some(64), Some(2)),
        provider("io.mixrelay.compressor"),
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
            NodeKind::VirtualEndpoint,
            NodeKind::Provider
        ]
    );
}

#[test]
fn host_audio_contracts_remain_valid_when_consumed_by_the_graph_crate() {
    let NodeConfig::VirtualEndpoint(endpoint) = virtual_endpoint("uid", "name", Some(64), Some(2))
    else {
        unreachable!("helper must produce a virtual endpoint");
    };
    endpoint
        .validate(&VirtualEndpointCapabilities {
            max_render_channels: 64,
            max_capture_channels: 64,
            supported_sample_rates: vec![48_000, 96_000],
            supports_duplex: true,
            supports_dynamic_creation: true,
        })
        .expect("endpoint should satisfy host capabilities");

    let request = ApplicationCaptureRequest {
        selector: ApplicationSelector::BundleId {
            bundle_id: "org.mozilla.firefox".into(),
            include_descendants: true,
        },
        stream: StreamSpec {
            channels: 1,
            channel_layout: ChannelLayout::Mono,
        },
        source_playback: SourcePlaybackPolicy::MuteWhileCaptured,
        forwarding_endpoint: Some(VirtualEndpointId::from("io.mixrelay.capture")),
        gain_db: 0.0,
        requires_reselection: false,
    };
    request
        .validate(&ApplicationCaptureCapabilities {
            max_channels: 2,
            supported_selectors: BTreeSet::from([ApplicationSelectorKind::BundleId]),
            supports_mute_while_captured: true,
        })
        .expect("application capture should satisfy host capabilities");
}

#[test]
fn virtual_gain_roundtrips_as_graph_state_without_changing_the_native_spec() {
    let original = virtual_endpoint("uid", "name", Some(64), Some(16));
    let serialized = serde_json::to_value(&original).unwrap();
    assert!(serialized["config"].get("gainDb").is_none());
    let mut restored: NodeConfig = serde_json::from_value(serialized).unwrap();
    let NodeConfig::VirtualEndpoint(config) = &mut restored else {
        unreachable!()
    };
    assert_eq!(config.gain_db.to_bits(), 0.0_f32.to_bits());
    let native = config.spec.clone();
    config.gain_db = -12.0;
    assert_eq!(config.spec, native);
    let saved = serde_json::to_value(&restored).unwrap();
    assert_eq!(saved["config"]["gainDb"], -12.0);
    let reloaded: NodeConfig = serde_json::from_value(saved).unwrap();
    assert_eq!(reloaded, restored);
}

fn resolve_virtual_reference(
    render_channels: Option<u16>,
    capture_channels: Option<u16>,
    reference: VirtualEndpointReference,
) -> Result<ResolvedNode, NodeResolutionError> {
    let mut config = virtual_endpoint("device", "Device", render_channels, capture_channels);
    let NodeConfig::VirtualEndpoint(endpoint) = &mut config else {
        unreachable!()
    };
    endpoint.reference = reference;
    BuiltInNodeResolver.resolve(&configured_node("reference", config), &context())
}

#[test]
fn virtual_references_require_visibility_and_reject_obsolete_signal_selection() {
    let config = virtual_endpoint("uid", "name", Some(64), Some(16));
    let serialized = serde_json::to_value(&config).unwrap();
    assert_eq!(
        serialized["config"]["reference"],
        serde_json::json!({"render":true,"capture":true})
    );
    for field in ["render", "capture"] {
        let mut invalid = serialized.clone();
        invalid["config"]["reference"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<NodeConfig>(invalid).is_err());
    }
    let mut obsolete = serialized;
    obsolete["config"]["reference"]["readSide"] = serde_json::json!("os_playback");
    assert!(serde_json::from_value::<NodeConfig>(obsolete).is_err());
}

#[test]
fn virtual_source_uses_driver_capture_width_and_one_public_port() {
    for (render, capture, channels) in [
        (Some(64), Some(16), 16),
        (Some(2), Some(16), 16),
        (None, Some(32), 32),
    ] {
        let resolved = resolve_virtual_reference(
            render,
            capture,
            VirtualEndpointReference {
                render: true,
                capture: false,
            },
        )
        .unwrap();
        assert!(resolved.inputs.is_empty());
        assert_eq!(
            resolved.outputs,
            vec![CompiledPort::new(
                PortId::from(FROM_OS_PORT_ID),
                PortDirection::Output,
                channels
            )]
        );
    }
}

#[test]
fn runtime_transport_cannot_be_serialized_or_deserialized_as_authored_graph() {
    let NodeConfig::VirtualEndpoint(config) = virtual_endpoint("id", "Name", Some(2), Some(16))
    else {
        unreachable!()
    };
    let transport = NodeConfig::VirtualEndpointTransport(config.spec);
    assert!(serde_json::to_value(&transport).is_err());
    assert!(
        serde_json::from_value::<NodeConfig>(serde_json::json!({
            "type":"virtual_endpoint_transport","config":{}
        }))
        .is_err()
    );
    let resolved = BuiltInNodeResolver
        .resolve(&configured_node("native", transport), &context())
        .unwrap();
    assert_eq!(resolved.outputs[0].channels(), 16);
    assert_eq!(resolved.inputs[0].channels(), 2);
}

#[test]
fn runtime_transport_memory_accounting_includes_reserved_native_spec_capacity() {
    let mut identity = String::with_capacity(128);
    identity.push_str("device-id");
    let mut name = String::with_capacity(256);
    name.push_str("Device");
    let mut rates = Vec::with_capacity(32);
    rates.push(48_000);
    let expected = identity.capacity() + name.capacity() + rates.capacity() * size_of::<u32>();
    let spec = VirtualEndpointSpec {
        id: VirtualEndpointId::new(identity),
        display_name: name,
        render: Some(StreamSpec {
            channels: 2,
            channel_layout: ChannelLayout::Stereo,
        }),
        capture: None,
        supported_sample_rates: rates,
        default_sample_rate: 48_000,
    };
    assert_eq!(spec.owned_heap_capacity_bytes(), expected);
}

#[test]
fn virtual_sink_reference_does_not_expose_unselected_native_playback() {
    let resolved = resolve_virtual_reference(
        Some(64),
        Some(16),
        VirtualEndpointReference {
            render: false,
            capture: true,
        },
    )
    .unwrap();
    assert!(resolved.outputs.is_empty());
    assert_eq!(
        resolved.inputs,
        vec![CompiledPort::new(
            PortId::from(TO_OS_PORT_ID),
            PortDirection::Input,
            64,
        )]
    );
}

#[test]
fn capture_only_device_can_expose_its_produced_mix_without_native_render() {
    let resolved = resolve_virtual_reference(
        None,
        Some(32),
        VirtualEndpointReference {
            render: true,
            capture: false,
        },
    )
    .unwrap();
    assert!(resolved.inputs.is_empty());
    assert_eq!(
        resolved.outputs,
        vec![CompiledPort::new(
            PortId::from(FROM_OS_PORT_ID),
            PortDirection::Output,
            32,
        )]
    );
}

#[test]
fn virtual_reference_rejects_no_direction_or_missing_native_sink() {
    for (render, capture, native_render, native_capture) in [
        (false, false, Some(2), Some(2)),
        (false, true, None, Some(2)),
        (true, false, Some(2), None),
        (true, false, None, None),
    ] {
        assert!(
            resolve_virtual_reference(
                native_render,
                native_capture,
                VirtualEndpointReference { render, capture }
            )
            .is_err()
        );
    }
}

#[test]
fn native_spec_conversion_selects_only_available_directions() {
    for (render, capture) in [(Some(2), None), (None, Some(16)), (Some(64), Some(32))] {
        let NodeConfig::VirtualEndpoint(config) = virtual_endpoint("uid", "name", render, capture)
        else {
            unreachable!()
        };
        assert_eq!(
            config.reference,
            VirtualEndpointReference {
                render: capture.is_some(),
                capture: render.is_some(),
            }
        );
    }
}

#[test]
fn virtual_gain_rejects_invalid_values_before_audio_preparation() {
    for gain in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -60.01,
        12.01,
        1000.0,
    ] {
        assert!(validate_virtual_endpoint_gain(gain).is_err());
        let mut config = virtual_endpoint("uid", "name", Some(2), Some(2));
        let NodeConfig::VirtualEndpoint(endpoint) = &mut config else {
            unreachable!()
        };
        endpoint.gain_db = gain;
        assert!(
            BuiltInNodeResolver
                .resolve(&configured_node("uid", config), &context())
                .is_err()
        );
    }
    for gain in [-60.0, 0.0, 12.0] {
        assert!(validate_virtual_endpoint_gain(gain).is_ok());
        let mut config = virtual_endpoint("uid", "name", Some(2), Some(2));
        let NodeConfig::VirtualEndpoint(endpoint) = &mut config else {
            unreachable!()
        };
        endpoint.gain_db = gain;
        assert!(
            BuiltInNodeResolver
                .resolve(&configured_node("uid", config), &context())
                .is_ok()
        );
    }
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
    let provider_node = configured_node("compressor", provider("io.mixrelay.compressor"));
    let unavailable = BuiltInNodeResolver
        .resolve(&provider_node, &context())
        .expect_err("unregistered provider must be explicit");
    assert!(matches!(
        unavailable,
        NodeResolutionError::CapabilityUnavailable { reason }
            if reason.contains("io.mixrelay.compressor")
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
