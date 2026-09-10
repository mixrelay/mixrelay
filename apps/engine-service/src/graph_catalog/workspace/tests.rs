use super::*;
use crate::{controller_test_support::settle, endpoint_settings_tests::controller};

fn request(engine: &EngineGraphController, operation: WorkspaceAction) -> WorkspaceRequest {
    let catalog = engine.read_catalog().unwrap();
    WorkspaceRequest {
        file_digest: None,
        expected_revision: catalog.document.revision,
        expected_catalog_revision: catalog.catalog_revision,
        operation,
    }
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
fn invalid_file_and_unknown_version_leave_current_workspace_untouched() {
    let dir = mixrelay_test_support::FixtureDir::new("workspace-invalid").unwrap();
    let engine = controller(GraphDocument::new(9, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let before = engine.read_catalog().unwrap();
    let bytes = std::fs::read(dir.join("graphs.json")).unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for contents in [
        "{".to_owned(),
        serde_json::json!({"format":"Other","version":1,"catalog":catalog}).to_string(),
        serde_json::json!({"format":"MixRelay","version":99,"catalog":catalog}).to_string(),
    ] {
        let path = dir.join("bad.mix");
        std::fs::write(&path, contents).unwrap();
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
        assert_eq!(std::fs::read(dir.join("graphs.json")).unwrap(), bytes);
    }
    assert!(file_path("relative.mix").is_err());
    assert!(file_path("/tmp/wrong.json").is_err());
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
fn workspace_capture_preview_is_read_only_and_changed_file_cannot_commit() {
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
    assert_eq!(engine.read_catalog().unwrap(), before);
    assert!(preview.manifest.sources.is_empty());
    assert!(preview.manifest.revision > before.document.revision);
    open.file_digest = preview.file_digest;
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b' ');
    std::fs::write(&path, &bytes).unwrap();
    assert!(engine.workspace_operation(open).is_err());
    assert_eq!(engine.read_catalog().unwrap(), before);
    let mut retry = request(&engine, WorkspaceAction::Open { path });
    let preview = engine.preview_workspace(retry.clone()).unwrap();
    retry.file_digest = preview.file_digest;
    let accepted = engine.workspace_operation(retry).unwrap();
    assert_eq!(accepted.document.revision, preview.manifest.revision);
}

#[test]
fn shared_devices_connections_and_missing_plugin_state_survive_file_round_trip() {
    use mixrelay_audio_graph::{NodeId, NodeSpec, ProviderNodeConfig, ProviderPorts};
    let dir = mixrelay_test_support::FixtureDir::new("workspace-shared").unwrap();
    let engine = controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(dir.join("graphs.json"));
    let mut catalog = Catalog::imported(crate::default_duplex_document(1, Some(-6.0)));
    let mut second = catalog.graphs[0].clone();
    second.id = "second".into();
    second.name = "Recording".into();
    second.document.nodes.push(NodeSpec {
        control: mixrelay_audio_graph::NodeControl::default(),
        id: NodeId::from("missing-effect"),
        label: Some("Saved Effect".into()),
        config: NodeConfig::Provider(ProviderNodeConfig {
            type_id: "vst3:not-installed".into(),
            parameters: serde_json::json!({"stateBase64":"AQ==","parameterValues":{"7":0.75}}),
            ports: Some(ProviderPorts {
                inputs: 8,
                outputs: 8,
            }),
        }),
    });
    second.layout.positions.insert(
        "missing-effect".into(),
        mixrelay_control_protocol::GraphNodePosition { x: 150.0, y: 220.0 },
    );
    catalog.graphs.push(second);
    let path = dir.join("shared.mix");
    stage_json(
        &path,
        &WorkspaceFile {
            format: "MixRelay".into(),
            version: 1,
            catalog: catalog.clone(),
        },
    )
    .unwrap()
    .commit()
    .unwrap();
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Open {
                path: path.to_string_lossy().into_owned(),
            },
        ))
        .unwrap();
    let _processor = settle(&engine);
    engine.checkpoint_processor_state(None).unwrap();
    engine
        .workspace_operation(request(
            &engine,
            WorkspaceAction::Save {
                path: path.to_string_lossy().into_owned(),
            },
        ))
        .unwrap();
    let restored = load(&path).unwrap();
    assert_eq!(restored.device_specs().unwrap().len(), 1);
    for (saved, original) in restored.graphs.iter().zip(&catalog.graphs) {
        assert_eq!(saved.id, original.id);
        assert_eq!(saved.document.nodes, original.document.nodes);
        assert_eq!(saved.document.edges, original.document.edges);
        assert_eq!(saved.layout, original.layout);
    }
}
