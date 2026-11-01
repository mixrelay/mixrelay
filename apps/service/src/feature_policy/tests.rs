use super::*;
use crate::{controller_test_support::settle, endpoint_settings_tests::controller};
use mixrelay_audio_graph::{
    ChannelLayout, ChannelMapping, ChannelRoute, CompileLimits, NodeId, NodeSpec, RenderFormat,
    StreamSpec,
};
use mixrelay_control_protocol::{
    GraphCatalogAction, GraphCatalogRequest, WorkspaceAction, WorkspaceRequest,
};
use mixrelay_control_server::ControlBackend;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const FORMAT: RenderFormat = RenderFormat {
    sample_rate: 48_000,
    block_frames: 128,
};
fn context() -> CompileContext {
    CompileContext {
        format: FORMAT,
        limits: CompileLimits::default(),
    }
}
fn measure(document: &GraphDocument) -> Usage {
    Usage::measure(&[document], &GraphCompiler::default(), context()).unwrap()
}
fn node(id: &str, config: NodeConfig) -> NodeSpec {
    NodeSpec {
        id: NodeId::from(id),
        label: None,
        control: mixrelay_audio_graph::NodeControl::default(),
        config,
    }
}
fn free_controller(document: GraphDocument) -> EngineGraphController {
    let mut value = controller(document);
    value.paid_access = Arc::new(|| false);
    value
}
fn mapping_document(channels: u16) -> GraphDocument {
    let mut document = crate::default_duplex_document(1, None);
    let NodeConfig::VirtualEndpoint(endpoint) = &mut document.nodes[0].config else {
        unreachable!()
    };
    let stream = StreamSpec {
        channels,
        channel_layout: ChannelLayout::Discrete,
    };
    endpoint.spec.render = Some(stream);
    endpoint.spec.capture = Some(stream);
    document
}
#[test]
fn compiled_auto_routes_count_channels_and_explicit_routes_preserve_a_64_channel_driver() {
    let mut document = mapping_document(64);
    assert_eq!(measure(&document).mappings, 2);
    assert!(measure(&document).compare(&Usage::default()).is_ok());
    document.edges.truncate(1);
    document.edges[0].channel_mapping = ChannelMapping::Auto;
    let denied = measure(&document).compare(&Usage::default()).unwrap_err();
    assert_eq!(
        denied.parameters,
        serde_json::json!({"resource":"driverMappings","limit":4,"used":64})
    );
    document.edges[0].channel_mapping = ChannelMapping::Explicit {
        routes: (0..4)
            .map(|channel| ChannelRoute {
                source_channel: channel,
                destination_channel: channel + 60,
            })
            .collect(),
    };
    assert!(measure(&document).compare(&Usage::default()).is_ok());
    let mut extra = document.edges[0].clone();
    extra.id = "extra".into();
    extra.channel_mapping = ChannelMapping::Explicit {
        routes: vec![ChannelRoute {
            source_channel: 5,
            destination_channel: 5,
        }],
    };
    document.edges.push(extra);
    assert_eq!(measure(&document).mappings, 5);
    assert_eq!(
        measure(&document)
            .compare(&Usage::default())
            .unwrap_err()
            .code,
        ErrorCode::FeatureLimit
    );
}
#[test]
fn free_admission_rejects_before_publication_and_paid_admission_accepts_the_same_mapping() {
    let initial = mapping_document(64);
    let paid = Arc::new(AtomicBool::new(false));
    let mut engine = controller(initial.clone());
    engine.paid_access = {
        let paid = Arc::clone(&paid);
        Arc::new(move || paid.load(Ordering::Acquire))
    };
    let mut next = initial.clone();
    next.revision += 1;
    next.edges.truncate(1);
    next.edges[0].channel_mapping = ChannelMapping::Auto;
    assert_eq!(
        engine
            .apply_graph_request(next.clone().into())
            .unwrap_err()
            .code,
        ErrorCode::FeatureLimit
    );
    assert_eq!(engine.graph(), initial);
    assert_eq!(engine.exchange.revisions().submitted, 1);
    paid.store(true, Ordering::Release);
    assert!(engine.validate_feature_usage(&next, None).is_ok());
    // Expiry admits unchanged/reducing controls on existing excess, but not expansion.
    *engine.graph.write() = next.clone();
    paid.store(false, Ordering::Release);
    assert!(engine.validate_feature_usage(&next, None).is_ok());
    let mut extra = next.edges[0].clone();
    extra.id = "extra".into();
    next.edges.push(extra);
    assert_eq!(
        engine.validate_feature_usage(&next, None).unwrap_err().code,
        ErrorCode::FeatureLimit
    );
}
#[test]
fn numerical_boundaries_and_downgrade_allow_reductions_without_silencing_existing_work() {
    let at_limit = Usage {
        graphs: 1,
        inputs: 2,
        outputs: BTreeSet::from([
            OutputIdentity::Driver("driver".into()),
            OutputIdentity::Hardware("monitor".into()),
        ]),
        devices: BTreeSet::from(["driver".into()]),
        mappings: 4,
        plugins: 2,
        recorders: 1,
        recording_channels: 2,
        aggregate_capture: false,
    };
    assert!(at_limit.compare(&Usage::default()).is_ok());
    for (resource, excess) in [
        (
            "graphs",
            Usage {
                graphs: 2,
                ..Usage::default()
            },
        ),
        (
            "inputNodes",
            Usage {
                inputs: 3,
                ..Usage::default()
            },
        ),
        (
            "outputDevices",
            Usage {
                outputs: BTreeSet::from([
                    OutputIdentity::Hardware("a".into()),
                    OutputIdentity::Hardware("b".into()),
                    OutputIdentity::Hardware("c".into()),
                ]),
                ..Usage::default()
            },
        ),
        (
            "virtualDevices",
            Usage {
                devices: BTreeSet::from(["a".into(), "b".into()]),
                ..Usage::default()
            },
        ),
        (
            "externalPlugins",
            Usage {
                plugins: 3,
                ..Usage::default()
            },
        ),
        (
            "recorders",
            Usage {
                recorders: 2,
                ..Usage::default()
            },
        ),
        (
            "recordingChannels",
            Usage {
                recording_channels: 3,
                ..Usage::default()
            },
        ),
    ] {
        assert_eq!(
            excess.compare(&at_limit).unwrap_err().parameters["resource"],
            resource
        );
        assert!(excess.compare(&excess).is_ok());
        assert!(at_limit.compare(&excess).is_ok());
    }
    assert_eq!(FREE_RECORDING_SECONDS, 7_200);
}
#[test]
fn builtin_processors_and_virtual_sources_do_not_use_external_or_input_quotas() {
    let mut document = crate::default_duplex_document(1, Some(-6.0));
    document
        .nodes
        .push(node("recorder", NodeConfig::Recorder { channels: 2 }));
    document.nodes.push(serde_json::from_value(serde_json::json!({"id":"compressor","type":"compressor","config":{"version":1,"bypass":false,"channels":2,"thresholdDb":-12,"ratio":4,"attackMs":10,"releaseMs":100,"makeupGainDb":0}})).unwrap());
    document.nodes.push(serde_json::from_value(serde_json::json!({"id":"limiter","type":"limiter","config":{"version":1,"bypass":false,"channels":2,"ceilingDb":-1,"releaseMs":100}})).unwrap());
    let usage = measure(&document);
    assert_eq!(
        (
            usage.inputs,
            usage.plugins,
            usage.outputs.len(),
            usage.recorders
        ),
        (0, 0, 1, 1)
    );
    assert!(usage.compare(&Usage::default()).is_ok());
}
#[test]
fn free_edits_preserve_an_unreadable_saved_catalog_until_an_explicit_file_operation() {
    let fixture = mixrelay_test_support::FixtureDir::new("free-policy-catalog").unwrap();
    let path = fixture.join("graphs.json");
    std::fs::write(&path, b"existing paid workspace").unwrap();
    let engine = free_controller(crate::default_duplex_document(1, None))
        .with_graph_catalog_path(path.clone());
    let before = engine.read_catalog().unwrap();
    let rejected = engine
        .mutate_graph_catalog(GraphCatalogRequest {
            expected_revision: 1,
            expected_catalog_revision: before.catalog_revision,
            action: GraphCatalogAction::Create {
                name: "extra".into(),
            },
        })
        .unwrap_err();
    assert_eq!(rejected.code, ErrorCode::FeatureLimit);
    let next = crate::default_duplex_document(2, Some(-6.0));
    engine.apply_graph_request(next.clone().into()).unwrap();
    let _processor = settle(&engine);
    assert!(engine.persist_parameter_document(&next).is_none());
    engine
        .update_graph_layout(
            "graph-1",
            mixrelay_control_protocol::GraphLayoutRequest {
                expected_layout_revision: 0,
                layout: mixrelay_control_protocol::GraphLayout::default(),
            },
        )
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"existing paid workspace");
}

#[test]
fn actual_input_output_and_dormant_external_instances_are_counted_independently() {
    use mixrelay_audio_graph::{
        AudioDeviceInputConfig, AudioDeviceOutputConfig, DeviceSelector, ProviderNodeConfig,
    };
    let mut document = crate::default_duplex_document(1, None);
    for id in ["input-a", "input-b"] {
        document.nodes.push(node(
            id,
            NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                selector: DeviceSelector::PersistentUid { uid: id.into() },
                output_channels: 2,
                gain_db: 0.0,
            }),
        ));
    }
    for id in ["effect-a", "effect-b"] {
        let mut effect = node(
            id,
            NodeConfig::Provider(ProviderNodeConfig {
                type_id: "fixture:external".into(),
                ports: None,
                parameters: serde_json::json!({}),
            }),
        );
        effect.control.enabled = false;
        document.nodes.push(effect);
    }
    for id in ["monitor-a", "duplicate-monitor-a"] {
        document.nodes.push(node(
            id,
            NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                selector: DeviceSelector::PersistentUid {
                    uid: "monitor-a".into(),
                },
                input_channels: 2,
            }),
        ));
    }
    let usage = measure(&document);
    assert_eq!(
        (usage.inputs, usage.plugins, usage.outputs.len()),
        (2, 2, 2)
    );
    assert!(usage.compare(&Usage::default()).is_ok());
    let mut third = document
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "effect-a")
        .unwrap()
        .clone();
    third.id = "effect-c".into();
    document.nodes.push(third);
    assert_eq!(
        measure(&document).compare(&usage).unwrap_err().parameters["resource"],
        "externalPlugins"
    );
}

#[test]
fn expired_paid_recording_can_pause_resume_and_stop_but_cannot_start_a_new_multichannel_take() {
    use mixrelay_audio_recorder::{Encoding, RecorderAction, RecorderRequest, RecorderState};
    let fixture = mixrelay_test_support::FixtureDir::new("expired-paid-take").unwrap();
    let graph = GraphDocument::new(
        1,
        vec![node("record", NodeConfig::Recorder { channels: 8 })],
        vec![],
    );
    let paid = Arc::new(AtomicBool::new(true));
    let mut engine = controller(graph);
    engine.paid_access = {
        let paid = Arc::clone(&paid);
        Arc::new(move || paid.load(Ordering::Acquire))
    };
    let engine = engine.with_graph_catalog_path(fixture.join("graphs.json"));
    let id = engine.recorder_session_id("graph-1", "record").unwrap();
    let tap = engine.recorders.registry().prepare(&id, 8).unwrap();
    tap.capture(&[], 48_000);
    let command = |action, path| mixrelay_control_protocol::RecorderControlRequest {
        expected_revision: 1,
        expected_recording: None,
        request: RecorderRequest {
            action,
            path,
            sample_rate: None,
            encoding: Some(Encoding::Float32),
        },
    };
    let path = fixture.join("paid.wav");
    engine
        .control_recorder(
            "graph-1",
            "record",
            command(RecorderAction::Record, Some(path.to_str().unwrap().into())),
        )
        .unwrap();
    tap.capture(&[0.25; 8 * 256], 48_000);
    paid.store(false, Ordering::Release);
    assert_eq!(
        engine
            .control_recorder("graph-1", "record", command(RecorderAction::Pause, None))
            .unwrap()
            .state,
        RecorderState::Paused
    );
    engine
        .control_recorder("graph-1", "record", command(RecorderAction::Record, None))
        .unwrap();
    tap.capture(&[0.5; 8 * 256], 48_000);
    assert_eq!(
        engine
            .control_recorder("graph-1", "record", command(RecorderAction::Stop, None))
            .unwrap()
            .frames,
        512
    );
    let denied = engine
        .control_recorder(
            "graph-1",
            "record",
            command(
                RecorderAction::Record,
                Some(fixture.join("free.wav").to_str().unwrap().into()),
            ),
        )
        .unwrap_err();
    assert_eq!(denied.parameters["resource"], "recordingChannels");
    assert!(!fixture.join("free.wav").exists());
    assert_eq!(
        mixrelay_test_support::rf64::open(path).unwrap().duration(),
        512
    );
}

#[test]
fn stale_reference_width_cannot_hide_authoritative_auto_routes() {
    let initial = mapping_document(2);
    let actual = mapping_document(64);
    let mut engine =
        crate::endpoint_settings_tests::controller_with_inventory(initial.clone(), &actual);
    engine.paid_access = Arc::new(|| false);
    let mut next = initial.clone();
    next.revision += 1;
    next.edges.truncate(1);
    next.edges[0].channel_mapping = ChannelMapping::Auto;
    let denied = engine.apply_graph_request(next.clone().into()).unwrap_err();
    assert_eq!(
        denied.parameters,
        serde_json::json!({"resource":"driverMappings","limit":4,"used":64})
    );
    assert_eq!(engine.graph(), initial);
    next.edges[0].channel_mapping = ChannelMapping::Explicit {
        routes: (0..4)
            .map(|channel| ChannelRoute {
                source_channel: channel,
                destination_channel: channel + 60,
            })
            .collect(),
    };
    engine.apply_graph_request(next.into()).unwrap();
}

#[test]
fn activating_paid_access_preserves_saved_work_until_an_explicit_workspace_save() {
    let fixture = mixrelay_test_support::FixtureDir::new("paid-promotion").unwrap();
    let path = fixture.join("graphs.json");
    std::fs::write(&path, b"old paid workspace").unwrap();
    let paid = Arc::new(AtomicBool::new(false));
    let mut engine = controller(crate::default_duplex_document(1, None));
    engine.paid_access = {
        let paid = Arc::clone(&paid);
        Arc::new(move || paid.load(Ordering::Acquire))
    };
    let engine = engine.with_graph_catalog_path(path.clone());
    let next = crate::default_duplex_document(2, Some(-6.0));
    engine.apply_graph_request(next.clone().into()).unwrap();
    let _processor = settle(&engine);
    paid.store(true, Ordering::Release);
    assert!(engine.persist_parameter_document(&next).is_none());
    assert_eq!(std::fs::read(&path).unwrap(), b"old paid workspace");
    let catalog = engine.read_catalog().unwrap();
    let saved = fixture.join("current.mix");
    engine
        .workspace_operation(WorkspaceRequest {
            expected_revision: catalog.document.revision,
            expected_catalog_revision: catalog.catalog_revision,
            operation: WorkspaceAction::Save {
                path: saved.to_str().unwrap().into(),
            },
            file_digest: None,
        })
        .unwrap();
    assert!(saved.exists());
    let restored =
        controller(crate::default_duplex_document(1, None)).with_graph_catalog_path(path);
    assert_eq!(restored.graph().nodes, next.nodes);
}

#[test]
fn whole_system_capture_uses_one_input_slot_with_platform_specific_paid_access() {
    use mixrelay_audio_io::{ApplicationCaptureRequest, ApplicationSelector, SourcePlaybackPolicy};
    let document = GraphDocument::new(
        1,
        vec![node(
            "all",
            NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
                selector: ApplicationSelector::AllExcept { bundle_ids: vec![] },
                stream: StreamSpec {
                    channels: 2,
                    channel_layout: ChannelLayout::Stereo,
                },
                source_playback: SourcePlaybackPolicy::Preserve,
                forwarding_endpoint: None,
                gain_db: 0.0,
                requires_reselection: false,
            }),
        )],
        vec![],
    );
    let usage = measure(&document);
    let engine = free_controller(GraphDocument::new(1, vec![], vec![]));
    let admission = engine.validate_feature_usage(&document, None);
    assert_eq!(usage.inputs, 1);
    if cfg!(target_os = "macos") {
        assert!(admission.is_ok());
        assert!(usage.compare(&Usage::default()).is_ok());
    } else {
        assert_eq!(
            admission.unwrap_err().parameters["resource"],
            "allApplicationsCapture"
        );
    }
    assert!(usage.compare(&usage).is_ok());
}
#[test]
fn hardware_alias_of_driver_counts_routes_and_shares_the_same_output_quota() {
    use mixrelay_audio_graph::{
        AudioDeviceOutputConfig, AudioEndpoint, DeviceSelector, EdgeSpec, PortId,
    };
    let mut graph = mapping_document(8);
    graph.edges.clear();
    graph.nodes.push(node(
        "raw-driver",
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: "driver-uid".into(),
            },
            input_channels: 8,
        }),
    ));
    let source = graph.nodes[0].id.clone();
    graph.edges.push(EdgeSpec {
        id: "raw-route".into(),
        source: AudioEndpoint {
            node_id: source,
            port_id: PortId::from(mixrelay_audio_graph::FROM_OS_PORT_ID),
        },
        destination: AudioEndpoint {
            node_id: "raw-driver".into(),
            port_id: PortId::from(mixrelay_audio_graph::AUDIO_PORT_ID),
        },
        channel_mapping: ChannelMapping::Auto,
    });
    let mut settings = controller(graph.clone()).endpoint_settings().unwrap();
    settings.devices[0]
        .native_device_ids
        .push("driver-uid".into());
    let mut usage = measure(&graph);
    usage
        .count_driver_aliases(&[&graph], &GraphCompiler::default(), context(), &settings)
        .unwrap();
    assert_eq!(usage.outputs.len(), 1);
    assert_eq!(usage.mappings, 8);
    let mut exported = Usage::nodes(&[&graph]);
    exported.count_export_routes(
        &graph,
        &GraphCompiler::default(),
        context(),
        Some(&settings),
    );
    assert_eq!(exported.outputs, usage.outputs);
    assert_eq!(exported.devices, usage.devices);
    assert_eq!(exported.mappings, usage.mappings);
    assert_eq!(
        usage.compare(&Usage::default()).unwrap_err().parameters["resource"],
        "driverMappings"
    );
}

mod recording_admission;
