use std::{fs::File, path::Path};

use super::*;
use crate::{
    controller_test_support::settle,
    endpoint_settings_tests::{controller, controller_with_inventory},
};
use mixrelay_audio_graph::{NodeId, NodeSpec, ProviderNodeConfig, ProviderPorts};
use mixrelay_control_server::ControlBackend;
use mixrelay_processor_state::{StateRef, StateStore, read_bundle, write_bundle};
use sha2::{Digest, Sha256};

fn request(engine: &EngineGraphController, operation: WorkspaceAction) -> WorkspaceRequest {
    let catalog = engine.read_catalog().unwrap();
    WorkspaceRequest {
        file_digest: None,
        expected_revision: catalog.document.revision,
        expected_catalog_revision: catalog.catalog_revision,
        operation,
    }
}

fn write_bundle_fixture(
    path: &Path,
    catalog: Catalog,
    references: &[StateRef],
    store: &StateStore,
    version: u32,
) {
    let manifest = serde_json::to_vec(&WorkspaceFile {
        format: "MixRelay".into(),
        version,
        catalog,
    })
    .unwrap();
    let mut file = File::create(path).unwrap();
    write_bundle(&mut file, &manifest, references, store).unwrap();
}

fn add_provider(catalog: &mut Catalog, id: &str, parameters: serde_json::Value) {
    let mut ports = ProviderPorts::canonical_stereo("fixture-eight");
    for bus in ports.inputs.iter_mut().chain(&mut ports.outputs) {
        bus.channels = 8;
        bus.channel_format = mixrelay_audio_graph::ProviderChannelFormat::Surround7_1;
    }
    let document = &mut catalog.graphs[0].document;
    document.nodes.push(NodeSpec {
        control: mixrelay_audio_graph::NodeControl::default(),
        id: NodeId::from(id),
        label: Some("Saved Effect".into()),
        config: NodeConfig::Provider(ProviderNodeConfig {
            type_id: "vst3:not-installed".into(),
            parameters,
            ports: Some(ports),
        }),
    });
    catalog.graphs[0].layout.positions.insert(
        id.into(),
        mixrelay_control_protocol::GraphNodePosition { x: 150.0, y: 220.0 },
    );
}

fn with_state(reference: &StateRef) -> serde_json::Value {
    serde_json::json!({
        "stateRef": reference,
        "parameterValues": {"7": 0.75}
    })
}

fn free_controller(document: GraphDocument) -> EngineGraphController {
    let mut engine = controller(document);
    engine.paid_access = std::sync::Arc::new(|| false);
    engine
}

fn add_unavailable_provider(document: &mut GraphDocument) {
    document.nodes.push(NodeSpec {
        id: "missing-plugin".into(),
        label: Some("Unavailable effect".into()),
        control: mixrelay_audio_graph::NodeControl::default(),
        config: NodeConfig::Provider(ProviderNodeConfig {
            type_id: "vst3:not-installed".into(),
            parameters: serde_json::json!({"parameterValues":{"7":0.75}}),
            ports: None,
        }),
    });
}

fn five_route_mapping() -> mixrelay_audio_graph::ChannelMapping {
    mixrelay_audio_graph::ChannelMapping::Explicit {
        routes: (0..5)
            .map(|channel| mixrelay_audio_graph::ChannelRoute {
                source_channel: channel,
                destination_channel: channel,
            })
            .collect(),
    }
}

#[test]
fn free_save_preserves_missing_driver_and_plugin_work_without_execution_admission() {
    let dir = mixrelay_test_support::FixtureDir::new("free-unavailable-export").unwrap();
    let mut document = crate::default_duplex_document(1, None);
    add_unavailable_provider(&mut document);
    let empty = GraphDocument::new(1, vec![], vec![]);
    let mut engine = controller_with_inventory(document.clone(), &empty);
    engine.paid_access = std::sync::Arc::new(|| false);
    let engine = engine.with_graph_catalog_path(dir.join("graphs.json"));
    let before = engine.read_catalog().unwrap();
    let candidate = Catalog::imported(document.clone());
    assert!(engine.validate_workspace_feature_usage(&candidate).is_err());
    let path = dir.join("preserved.mix");
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save {
                path: path.to_string_lossy().into_owned(),
            },
        ))
        .unwrap();
    assert_eq!(
        load(&path).unwrap().graphs[0].document.nodes,
        document.nodes
    );
    assert_eq!(
        load(&path).unwrap().graphs[0].document.edges,
        document.edges
    );
    assert_eq!(engine.read_catalog().unwrap(), before);
    assert_eq!(
        Catalog::load(&dir.join("graphs.json"))
            .unwrap()
            .unwrap()
            .graphs[0]
            .document
            .nodes,
        document.nodes
    );
    assert!(
        engine
            .preview_workspace(request(
                &engine,
                WorkspaceAction::Open {
                    path: path.to_string_lossy().into_owned(),
                }
            ))
            .is_err()
    );
    assert_eq!(engine.read_catalog().unwrap(), before);
}

#[test]
fn free_export_retains_unknown_plugin_geometry_but_rejects_known_excess_routes() {
    let mut document = crate::default_duplex_document(1, None);
    add_unavailable_provider(&mut document);
    let engine = free_controller(document.clone());
    assert!(
        engine
            .validate_workspace_feature_usage(&Catalog::imported(document.clone()))
            .is_err()
    );
    assert!(
        engine
            .validate_workspace_export_usage(&Catalog::imported(document.clone()))
            .is_ok()
    );
    document.edges[0].source.node_id = "missing-plugin".into();
    document.edges[0].source.port_id = "audio".into();
    assert!(
        engine
            .validate_workspace_export_usage(&Catalog::imported(document.clone()))
            .is_ok()
    );
    document.edges[0].channel_mapping = five_route_mapping();
    let error = engine
        .validate_workspace_export_usage(&Catalog::imported(document))
        .unwrap_err();
    assert_eq!(
        error.code,
        mixrelay_control_protocol::ErrorCode::FeatureLimit
    );
    assert_eq!(error.parameters["resource"], "driverMappings");
}

#[test]
fn free_export_counts_auto_routes_without_compiling_an_unrelated_missing_plugin() {
    let mut document = crate::default_duplex_document(1, None);
    let NodeConfig::VirtualEndpoint(endpoint) = &mut document.nodes[0].config else {
        panic!("endpoint")
    };
    for stream in [&mut endpoint.spec.render, &mut endpoint.spec.capture] {
        stream.as_mut().unwrap().channels = 64;
        stream.as_mut().unwrap().channel_layout = mixrelay_audio_graph::ChannelLayout::Discrete;
    }
    document.edges.truncate(1);
    document.edges[0].channel_mapping = mixrelay_audio_graph::ChannelMapping::Auto;
    add_unavailable_provider(&mut document);
    let engine = free_controller(document.clone());
    let error = engine
        .validate_workspace_export_usage(&Catalog::imported(document))
        .unwrap_err();
    assert_eq!(
        error.code,
        mixrelay_control_protocol::ErrorCode::FeatureLimit
    );
    assert_eq!(error.parameters["resource"], "driverMappings");
    assert_eq!(error.parameters["used"], 64);
}

#[test]
fn free_save_does_not_require_a_disconnected_hardware_output_to_be_available() {
    let dir = mixrelay_test_support::FixtureDir::new("free-hardware-export").unwrap();
    let document = GraphDocument::new(
        1,
        vec![NodeSpec {
            id: "output".into(),
            label: Some("Disconnected interface".into()),
            control: mixrelay_audio_graph::NodeControl::default(),
            config: NodeConfig::AudioDeviceOutput(mixrelay_audio_graph::AudioDeviceOutputConfig {
                selector: mixrelay_audio_io::DeviceSelector::PersistentUid {
                    uid: "offline-interface".into(),
                },
                input_channels: 2,
            }),
        }],
        vec![],
    );
    let engine = free_controller(document.clone()).with_graph_catalog_path(dir.join("graphs.json"));
    assert!(
        engine
            .validate_workspace_feature_usage(&Catalog::imported(document.clone()))
            .is_err()
    );
    let path = dir.join("hardware.mix");
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save {
                path: path.to_string_lossy().into_owned(),
            },
        ))
        .unwrap();
    assert_eq!(load(&path).unwrap().graphs[0].document, document);
}

#[test]
fn free_single_graph_can_save_open_and_restore_its_routing_and_layout_after_restart() {
    let dir = mixrelay_test_support::FixtureDir::new("free-workspace").unwrap();
    let catalog_path = dir.join("graphs.json");
    let initial = crate::default_duplex_document(9, Some(-6.0));
    let engine = free_controller(initial.clone()).with_graph_catalog_path(catalog_path.clone());
    let catalog = engine.read_catalog().unwrap();
    let mut layout = mixrelay_control_protocol::GraphLayout::default();
    layout.positions.insert(
        initial.nodes[0].id.as_str().into(),
        mixrelay_control_protocol::GraphNodePosition { x: 125.0, y: 250.0 },
    );
    layout.node_labels.insert(
        format!("{}:input", initial.nodes[0].id.as_str()),
        "회의용".into(),
    );
    engine
        .update_graph_layout(
            &catalog.active_id,
            mixrelay_control_protocol::GraphLayoutRequest {
                expected_layout_revision: catalog.layout_revision,
                layout: layout.clone(),
            },
        )
        .unwrap();
    let path = dir.join("free.mix").to_string_lossy().into_owned();
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save { path: path.clone() },
        ))
        .unwrap();
    engine
        .workspace_operation(request(&engine, WorkspaceAction::New))
        .unwrap();
    let _processor = settle(&engine);
    assert!(engine.graph().nodes.is_empty());
    let open = request(&engine, WorkspaceAction::Open { path });
    let preview = engine.preview_workspace(open.clone()).unwrap();
    let mut open = open;
    open.file_digest = preview.file_digest;
    let loaded = engine.workspace_operation(open).unwrap();
    let _processor = settle(&engine);
    assert_eq!(loaded.document.nodes, initial.nodes);
    assert_eq!(loaded.document.edges, initial.edges);
    assert_eq!(loaded.layout, layout);
    drop(engine);
    let mut restarted = controller_with_inventory(GraphDocument::new(1, vec![], vec![]), &initial);
    restarted.paid_access = std::sync::Arc::new(|| false);
    let restarted = restarted.with_graph_catalog_path(catalog_path);
    let _processor = settle(&restarted);
    let restored = restarted.read_catalog().unwrap();
    assert_eq!(restored.graphs.len(), 1);
    assert_eq!(restored.document.nodes, initial.nodes);
    assert_eq!(restored.document.edges, initial.edges);
    assert_eq!(restored.layout, layout);
}

#[test]
fn free_workspace_files_reject_multiple_graphs_and_excess_mappings_before_mutation() {
    let dir = mixrelay_test_support::FixtureDir::new("free-import-limits").unwrap();
    let catalog_path = dir.join("graphs.json");
    let store = StateStore::new(dir.join("assets"));
    let mut multiple = Catalog::imported(GraphDocument::new(1, vec![], vec![]));
    let mut second = multiple.graphs[0].clone();
    second.id = "graph-2".into();
    multiple.graphs.push(second);
    let mut mappings = crate::default_duplex_document(1, None);
    let NodeConfig::VirtualEndpoint(endpoint) = &mut mappings.nodes[0].config else {
        panic!("endpoint")
    };
    for stream in [&mut endpoint.spec.render, &mut endpoint.spec.capture] {
        stream.as_mut().unwrap().channels = 64;
        stream.as_mut().unwrap().channel_layout = mixrelay_audio_graph::ChannelLayout::Discrete;
    }
    mappings.edges.truncate(1);
    mappings.edges[0].channel_mapping = mixrelay_audio_graph::ChannelMapping::Auto;
    let mut explicit_mappings = mappings.clone();
    explicit_mappings.edges[0].channel_mapping = five_route_mapping();
    let mut plugins = Catalog::imported(GraphDocument::new(1, vec![], vec![]));
    for id in ["one", "two", "three"] {
        add_provider(&mut plugins, id, serde_json::json!({}));
    }
    let recorder = Catalog::imported(GraphDocument::new(
        1,
        vec![NodeSpec {
            id: "record".into(),
            label: None,
            control: mixrelay_audio_graph::NodeControl::default(),
            config: NodeConfig::Recorder { channels: 8 },
        }],
        vec![],
    ));
    let mut engine = controller_with_inventory(GraphDocument::new(1, vec![], vec![]), &mappings);
    engine.paid_access = std::sync::Arc::new(|| false);
    let engine = engine.with_graph_catalog_path(catalog_path.clone());
    let before = engine.read_catalog().unwrap();
    let bytes = std::fs::read(&catalog_path).unwrap();
    let mut allowed_plugins = plugins.clone();
    allowed_plugins.graphs[0].document.nodes.truncate(2);
    allowed_plugins.graphs[0].layout.positions.remove("three");
    assert!(
        engine
            .validate_workspace_feature_usage(&allowed_plugins)
            .is_ok()
    );
    for (name, candidate, resource) in [
        ("multiple", multiple, "graphs"),
        ("mappings", Catalog::imported(mappings), "driverMappings"),
        (
            "explicit-mappings",
            Catalog::imported(explicit_mappings),
            "driverMappings",
        ),
        ("plugins", plugins, "externalPlugins"),
        ("recorder", recorder, "recordingChannels"),
    ] {
        let file = dir.join(format!("{name}.mix"));
        write_bundle_fixture(&file, candidate, &[], &store, 1);
        let open = request(
            &engine,
            WorkspaceAction::Open {
                path: file.to_string_lossy().into_owned(),
            },
        );
        for error in [
            engine.preview_workspace(open.clone()).unwrap_err(),
            engine.workspace_operation(open).unwrap_err(),
        ] {
            assert_eq!(
                error.code,
                mixrelay_control_protocol::ErrorCode::FeatureLimit
            );
            assert_eq!(error.parameters["resource"], resource);
        }
        assert_eq!(engine.read_catalog().unwrap(), before);
        assert_eq!(std::fs::read(&catalog_path).unwrap(), bytes);
        assert_eq!(engine.exchange.revisions().submitted, 1);
    }
}

#[test]
fn saving_after_paid_expiry_cannot_export_a_multiple_graph_workspace_or_replace_a_file() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let dir = mixrelay_test_support::FixtureDir::new("expired-workspace-save").unwrap();
    let path = dir.join("graphs.json");
    let mut catalog = Catalog::imported(GraphDocument::new(1, vec![], vec![]));
    let mut second = catalog.graphs[0].clone();
    second.id = "graph-2".into();
    catalog.graphs.push(second);
    let bytes = serde_json::to_vec(&catalog).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let paid = Arc::new(AtomicBool::new(true));
    let mut engine = controller(GraphDocument::new(1, vec![], vec![]));
    engine.paid_access = {
        let paid = Arc::clone(&paid);
        Arc::new(move || paid.load(Ordering::Acquire))
    };
    let engine = engine.with_graph_catalog_path(path.clone());
    let _processor = settle(&engine);
    let before = engine.read_catalog().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    paid.store(false, Ordering::Release);
    let target = dir.join("existing.mix");
    std::fs::write(&target, b"existing export").unwrap();
    let error = engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save {
                path: target.to_string_lossy().into_owned(),
            },
        ))
        .unwrap_err();
    assert_eq!(
        error.code,
        mixrelay_control_protocol::ErrorCode::FeatureLimit
    );
    assert_eq!(error.parameters["resource"], "graphs");
    assert_eq!(std::fs::read(&target).unwrap(), b"existing export");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(engine.read_catalog().unwrap(), before);
}

#[test]
fn free_startup_protects_a_saved_multiple_graph_catalog_from_automatic_overwrite() {
    let dir = mixrelay_test_support::FixtureDir::new("free-protected-catalog").unwrap();
    let path = dir.join("graphs.json");
    let mut catalog = Catalog::imported(GraphDocument::new(1, vec![], vec![]));
    let mut second = catalog.graphs[0].clone();
    second.id = "graph-2".into();
    catalog.graphs.push(second);
    let bytes = serde_json::to_vec(&catalog).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let engine = free_controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(path.clone());
    let selected = engine.read_catalog().unwrap();
    assert_eq!(selected.graphs.len(), 1);
    engine
        .update_graph_layout(
            &selected.active_id,
            mixrelay_control_protocol::GraphLayoutRequest {
                expected_layout_revision: selected.layout_revision,
                layout: mixrelay_control_protocol::GraphLayout::default(),
            },
        )
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn save_open_preserves_catalog_ids_and_layout_and_advances_revisions() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace").unwrap();
    let engine = controller(GraphDocument::new(9, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let path = dir.join("session.mix").to_string_lossy().into_owned();
    let before = engine.read_catalog().unwrap();
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save { path: path.clone() },
        ))
        .unwrap();
    let file = load(Path::new(&path)).unwrap();
    assert_eq!(file.active_id, before.active_id);
    engine
        .workspace_operation(request(&engine, WorkspaceAction::New))
        .unwrap();
    let _processor = settle(&engine);
    let result = engine
        .workspace_operation(request(&engine, WorkspaceAction::Open { path }))
        .unwrap();
    assert_eq!(result.graphs, before.graphs);
    assert_eq!(result.layout, before.layout);
    assert!(result.document.revision > before.document.revision);
    assert!(result.catalog_revision > before.catalog_revision);
}

#[test]
fn invalid_binary_manifest_and_json_only_file_leave_workspace_untouched() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace-invalid").unwrap();
    let engine = controller(GraphDocument::new(9, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let before = engine.read_catalog().unwrap();
    let graph_bytes = std::fs::read(dir.join("graphs.json")).unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&graph_bytes).unwrap();
    let empty_store = StateStore::new(dir.join("empty-assets"));
    let unknown_version = dir.join("unknown.mix");
    write_bundle_fixture(
        &unknown_version,
        serde_json::from_value(catalog.clone()).unwrap(),
        &[],
        &empty_store,
        99,
    );
    let json_only = dir.join("json-only.mix");
    std::fs::write(
        &json_only,
        serde_json::json!({"format":"MixRelay","version":1,"catalog":catalog}).to_string(),
    )
    .unwrap();
    let truncated = dir.join("truncated.mix");
    std::fs::write(&truncated, b"MixRelay").unwrap();
    for path in [unknown_version, json_only, truncated] {
        assert!(
            engine
                .workspace_operation(request(
                    &engine,
                    WorkspaceAction::Open {
                        path: path.to_string_lossy().into_owned()
                    }
                ))
                .is_err()
        );
        assert_eq!(engine.read_catalog().unwrap(), before);
        assert_eq!(std::fs::read(dir.join("graphs.json")).unwrap(), graph_bytes);
    }
    assert!(file_path("relative.mix").is_err());
    assert!(file_path("/fixture/wrong.json").is_err());
}

#[test]
fn failed_save_does_not_destroy_destination_or_current_catalog() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace-write").unwrap();
    let engine = controller(GraphDocument::new(9, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let path = dir.join("directory.mix");
    std::fs::create_dir(&path).unwrap();
    let before = engine.read_catalog().unwrap();
    assert!(
        engine
            .workspace_operation(request(
                &engine,
                WorkspaceAction::Save {
                    path: path.to_string_lossy().into_owned()
                }
            ))
            .is_err()
    );
    assert!(path.is_dir());
    assert_eq!(before, engine.read_catalog().unwrap());
}

#[test]
fn preview_hashes_every_bundle_byte_without_importing_state_and_fences_open() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace-preview").unwrap();
    let engine = controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let path = dir.join("preview.mix").to_string_lossy().into_owned();
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save { path: path.clone() },
        ))
        .unwrap();
    let before = engine.read_catalog().unwrap();
    let mut open = request(&engine, WorkspaceAction::Open { path: path.clone() });
    let preview = engine.preview_workspace(open.clone()).unwrap();
    let file_bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        preview.file_digest,
        Some(format!("{:x}", Sha256::digest(&file_bytes)))
    );
    assert_eq!(engine.read_catalog().unwrap(), before);
    assert!(preview.manifest.sources.is_empty());
    assert!(preview.manifest.revision > before.document.revision);

    let changed = Catalog::imported(GraphDocument::new(12, vec![], vec![]));
    let store = StateStore::new(dir.join("fixture-assets"));
    write_bundle_fixture(Path::new(&path), changed, &[], &store, 1);
    open.file_digest = preview.file_digest;
    assert!(engine.workspace_operation(open).is_err());
    assert_eq!(engine.read_catalog().unwrap(), before);

    let mut retry = request(&engine, WorkspaceAction::Open { path });
    let preview = engine.preview_workspace(retry.clone()).unwrap();
    retry.file_digest = preview.file_digest;
    let accepted = engine.workspace_operation(retry).unwrap();
    assert_eq!(accepted.document.revision, preview.manifest.revision);
}

#[test]
fn missing_unreferenced_and_malformed_state_assets_do_not_change_catalog() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace-assets-invalid").unwrap();
    let engine = controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let before = engine.read_catalog().unwrap();
    let store = engine.processor_controls.state_store().unwrap();
    let reference = store.put(b"fixture processor state").unwrap();

    let mut missing_catalog = Catalog::imported(GraphDocument::new(2, vec![], vec![]));
    add_provider(
        &mut missing_catalog,
        "missing-state",
        with_state(&reference),
    );
    let missing = dir.join("missing-asset.mix");
    write_bundle_fixture(&missing, missing_catalog.clone(), &[], &store, 1);

    let corrupted = dir.join("corrupted-payload.mix");
    write_bundle_fixture(
        &corrupted,
        missing_catalog,
        std::slice::from_ref(&reference),
        &store,
        1,
    );
    let mut corrupted_bytes = std::fs::read(&corrupted).unwrap();
    let final_byte = corrupted_bytes.len() - 1;
    corrupted_bytes[final_byte] ^= 0xff;
    std::fs::write(&corrupted, corrupted_bytes).unwrap();

    let unreferenced = dir.join("unreferenced-asset.mix");
    write_bundle_fixture(
        &unreferenced,
        Catalog::imported(GraphDocument::new(3, vec![], vec![])),
        std::slice::from_ref(&reference),
        &store,
        1,
    );

    let mut malformed_catalog = Catalog::imported(GraphDocument::new(4, vec![], vec![]));
    add_provider(
        &mut malformed_catalog,
        "malformed-state",
        serde_json::json!({"stateRef":{"sha256":"bad","byteLength":7}}),
    );
    let malformed = dir.join("malformed-state.mix");
    write_bundle_fixture(&malformed, malformed_catalog, &[], &store, 1);

    for path in [missing, unreferenced, malformed, corrupted] {
        assert!(
            engine
                .workspace_operation(request(
                    &engine,
                    WorkspaceAction::Open {
                        path: path.to_string_lossy().into_owned()
                    }
                ))
                .is_err()
        );
        assert_eq!(engine.read_catalog().unwrap(), before);
    }
}

#[test]
fn portable_bundle_reopens_large_states_after_source_store_is_deleted() {
    const STATE_BYTES: usize = 9 * 1024 * 1024;
    let dir = mixrelay_test_support::FixtureDir::new("workspace-portable-state").unwrap();
    let driver_inventory = crate::default_duplex_document(1, None);
    let engine = crate::endpoint_settings_tests::controller_with_inventory(
        GraphDocument::new(1, vec![], vec![]),
        &driver_inventory,
    )
    .with_graph_catalog_path(dir.join("graphs.json"));
    let source_store = engine.processor_controls.state_store().unwrap();
    let first_state = vec![0x35; STATE_BYTES];
    let second_state = vec![0xa7; STATE_BYTES];
    let first_ref = source_store.put(&first_state).unwrap();
    let second_ref = source_store.put(&second_state).unwrap();
    let mut catalog = Catalog::imported(crate::default_duplex_document(1, Some(-6.0)));
    add_provider(&mut catalog, "missing-effect-a", with_state(&first_ref));
    add_provider(&mut catalog, "missing-effect-b", with_state(&second_ref));
    let references = state_assets::referenced(&catalog).unwrap();
    assert!(
        references
            .windows(2)
            .all(|pair| pair[0].sha256() < pair[1].sha256())
    );
    let source = dir.join("portable-source.mix");
    write_bundle_fixture(&source, catalog.clone(), &references, &source_store, 1);
    let source_bytes = std::fs::read(&source).unwrap();
    assert!(source_bytes.len() > 16 * 1024 * 1024);
    let preview_engine = crate::endpoint_settings_tests::controller_with_inventory(
        GraphDocument::new(1, vec![], vec![]),
        &driver_inventory,
    )
    .with_graph_catalog_path(dir.join("preview-graphs.json"));
    let preview_store = preview_engine.processor_controls.state_store().unwrap();
    let preview_request = request(
        &preview_engine,
        WorkspaceAction::Open {
            path: source.to_string_lossy().into_owned(),
        },
    );
    let preview = preview_engine.preview_workspace(preview_request).unwrap();
    assert!(!preview_store.root().exists());
    assert_eq!(
        preview.file_digest,
        Some(format!("{:x}", Sha256::digest(&source_bytes)))
    );
    drop(source_bytes);

    let mut open = request(
        &engine,
        WorkspaceAction::Open {
            path: source.to_string_lossy().into_owned(),
        },
    );

    std::fs::remove_dir_all(source_store.root()).unwrap();
    open.file_digest = preview.file_digest;
    engine.workspace_operation(open).unwrap();
    let _processor = settle(&engine);

    let imported_store = engine.processor_controls.state_store().unwrap();
    assert_eq!(imported_store.read(&first_ref).unwrap(), first_state);
    assert_eq!(imported_store.read(&second_ref).unwrap(), second_state);
    let output = dir.join("portable-output.mix");
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save {
                path: output.to_string_lossy().into_owned(),
            },
        ))
        .unwrap();

    let reopened_store = StateStore::new(dir.join("reopened-assets"));
    let (manifest, bundled_refs) =
        read_bundle(File::open(&output).unwrap(), Some(&reopened_store)).unwrap();
    let reopened: WorkspaceFile = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(
        state_assets::referenced(&reopened.catalog).unwrap(),
        bundled_refs
    );
    assert_eq!(reopened_store.read(&first_ref).unwrap(), first_state);
    assert_eq!(reopened_store.read(&second_ref).unwrap(), second_state);
    assert_saved_provider_overlays(&reopened.catalog);
}

fn assert_saved_provider_overlays(catalog: &Catalog) {
    for id in ["missing-effect-a", "missing-effect-b"] {
        let provider = catalog.graphs[0]
            .document
            .nodes
            .iter()
            .find(|node| node.id.as_str() == id)
            .unwrap();
        let NodeConfig::Provider(provider) = &provider.config else {
            panic!("fixture must remain a provider");
        };
        assert_eq!(provider.parameters["parameterValues"]["7"], 0.75);
    }
}
