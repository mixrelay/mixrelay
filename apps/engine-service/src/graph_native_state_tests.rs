use super::*;
use mixrelay_processor_host::ProcessorStateError;

#[test]
fn native_editor_checkpoint_replaces_stale_overlays_and_survives_restart_without_audio_prepare() {
    let fixture = Fixture::new();
    let (engine, prepares) = parameter_fixture(parameter_document());
    let engine = engine.with_graph_catalog_path(fixture.path());
    engine
        .apply_processor_parameter_update("effect", 7, 0.25, |_, _| Ok(()))
        .unwrap();
    let before = engine.graph_catalog().unwrap();
    let native = serde_json::json!({"stateBase64": "AgM="});
    let mutation = engine.mutation.lock();
    engine
        .checkpoint_processor_state_with(Some("effect"), |id, timeout| {
            assert_eq!(
                *id,
                crate::graph_composition::runtime_node_id("graph-1", &NodeId::from("effect"))
            );
            assert!(!timeout.is_zero() && timeout <= std::time::Duration::from_secs(5));
            Ok(native.clone())
        })
        .unwrap();
    drop(mutation);
    assert_eq!(provider_parameters(&engine.graph()), native);
    assert_eq!(prepares.load(Ordering::Relaxed), 0);
    let after = engine.graph_catalog().unwrap();
    assert_eq!(after.catalog_revision, before.catalog_revision);
    assert_eq!(after.document.revision, before.document.revision);
    assert!(after.persistence_error.is_none());
    drop(engine);
    let (restarted, _) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let restarted = restarted.with_graph_catalog_path(fixture.path());
    assert_eq!(provider_parameters(&restarted.graph()), native);
}

#[test]
fn native_editor_snapshot_failure_preserves_every_provider_and_existing_disk() {
    let fixture = Fixture::new();
    let mut document = parameter_document();
    let mut second = document.nodes[0].clone();
    second.id = NodeId::from("second");
    document.nodes.push(second);
    let (engine, _) = parameter_fixture(document.clone());
    let engine = engine.with_graph_catalog_path(fixture.path());
    let disk = fs::read(fixture.path()).unwrap();
    for failure in [
        ProcessorStateError::Unsupported,
        ProcessorStateError::Timeout,
        ProcessorStateError::NativeFailure,
        ProcessorStateError::Unavailable,
    ] {
        let mutation = engine.mutation.lock();
        let error = engine
            .checkpoint_processor_state_with(None, |id, _| {
                if *id
                    == crate::graph_composition::runtime_node_id("graph-1", &NodeId::from("effect"))
                {
                    Ok(serde_json::json!({"stateBase64": "Ag=="}))
                } else {
                    Err(failure)
                }
            })
            .unwrap_err();
        drop(mutation);
        assert_eq!(error.code, ErrorCode::ControlUnavailable);
        assert_eq!(engine.graph(), document);
        assert_eq!(fs::read(fixture.path()).unwrap(), disk);
        assert!(engine.graph_catalog().unwrap().persistence_error.is_some());
    }
}

fn two_graph_providers() -> (Fixture, EngineGraphController, Arc<AtomicU64>) {
    let fixture = Fixture::new();
    let document = parameter_document();
    fs::write(
        fixture.path(),
        serde_json::to_vec(&serde_json::json!({
            "version":1, "revision":1, "activeId":"alpha", "graphs":[
                {"id":"alpha", "name":"Alpha", "document":document},
                {"id":"beta", "name":"Beta", "document":document},
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let (engine, prepares) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let engine = engine.with_graph_catalog_path(fixture.path());
    settle(&engine);
    (fixture, engine, prepares)
}

#[test]
fn all_graph_checkpoint_uses_distinct_runtime_ids_and_keeps_authored_identity_without_prepare() {
    let (_fixture, engine, prepares) = two_graph_providers();
    let before_prepares = prepares.load(Ordering::Relaxed);
    let before_revision = engine.exchange.revisions();
    let mut captured = Vec::new();
    let mutation = engine.mutation.lock();
    engine
        .checkpoint_processor_state_with(None, |id, _| {
            captured.push(id.clone());
            Ok(serde_json::json!({"stateBase64":id.as_str()}))
        })
        .unwrap();
    drop(mutation);
    let catalog = engine.catalog_candidate(&engine.graph()).unwrap();
    for (graph_id, document) in catalog.documents() {
        let expected = crate::graph_composition::runtime_node_id(graph_id, &NodeId::from("effect"));
        assert!(captured.contains(&expected));
        assert_eq!(document.nodes[0].id.as_str(), "effect");
        assert_eq!(
            provider_parameters(document)["stateBase64"],
            expected.as_str()
        );
    }
    assert_eq!(captured.len(), 2);
    assert_eq!(prepares.load(Ordering::Relaxed), before_prepares);
    assert_eq!(engine.exchange.revisions(), before_revision);
}

#[test]
fn background_editor_checkpoint_targets_its_owner_and_failed_all_graph_export_is_atomic() {
    let (fixture, engine, _) = two_graph_providers();
    let selected = engine.graph();
    let mutation = engine.mutation.lock();
    engine
        .checkpoint_graph_processor_state_with("beta", "effect", |id, _| {
            assert_eq!(
                *id,
                crate::graph_composition::runtime_node_id("beta", &NodeId::from("effect"))
            );
            Ok(serde_json::json!({"stateBase64":"background"}))
        })
        .unwrap();
    drop(mutation);
    assert_eq!(engine.graph(), selected);
    assert_eq!(
        provider_parameters(&engine.read_catalog_document("beta").unwrap())["stateBase64"],
        "background"
    );
    let disk = fs::read(fixture.path()).unwrap();
    let mutation = engine.mutation.lock();
    let result = engine.checkpoint_processor_state_with(None, |id, _| {
        if *id == crate::graph_composition::runtime_node_id("alpha", &NodeId::from("effect")) {
            Ok(serde_json::json!({"stateBase64":"must-not-persist"}))
        } else {
            Err(ProcessorStateError::NativeFailure)
        }
    });
    drop(mutation);
    assert!(result.is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), disk);
    assert_eq!(engine.graph(), selected);
}

#[test]
fn composition_rebuild_preserves_background_state_without_replacing_selected_edit_deltas() {
    let (_fixture, engine, prepares) = two_graph_providers();
    let before_prepares = prepares.load(Ordering::Relaxed);
    let mut selected = engine.graph();
    let NodeConfig::Provider(provider) = &mut selected.nodes[0].config else {
        panic!("fixture")
    };
    provider.parameters =
        serde_json::json!({"stateBase64":"selected", "parameterValues":{"7":0.4}});
    let mut candidate = engine.catalog_candidate(&selected).unwrap();
    let mutation = engine.mutation.lock();
    engine
        .preserve_catalog_provider_state_with(&mut candidate, |id, _| {
            assert_eq!(
                *id,
                crate::graph_composition::runtime_node_id("beta", &NodeId::from("effect"))
            );
            Ok(serde_json::json!({"stateBase64":"background-live"}))
        })
        .unwrap();
    drop(mutation);
    assert_eq!(*candidate.active_document(), selected);
    let (_, background) = candidate.documents().find(|(id, _)| *id == "beta").unwrap();
    assert_eq!(
        provider_parameters(background)["stateBase64"],
        "background-live"
    );
    assert_eq!(prepares.load(Ordering::Relaxed), before_prepares);
}

#[test]
fn native_editor_checkpoint_storage_failure_rolls_back_and_same_revision_retry_succeeds() {
    for commit_failure in [false, true] {
        let fixture = Fixture::new();
        let document = parameter_document();
        let (engine, _) = parameter_fixture(document.clone());
        let engine = engine.with_graph_catalog_path(fixture.path());
        let disk = fs::read(fixture.path()).unwrap();
        let blocked = fixture.0.join("blocked");
        let destination = if commit_failure {
            fs::create_dir(&blocked).unwrap();
            blocked
        } else {
            fs::write(&blocked, b"not a directory").unwrap();
            blocked.join("graphs.json")
        };
        engine.catalog.lock().as_mut().unwrap().path = destination;
        let native = serde_json::json!({"stateBase64": "BA=="});
        let mutation = engine.mutation.lock();
        assert!(
            engine
                .checkpoint_processor_state_with(Some("effect"), |_, _| Ok(native.clone()))
                .is_err()
        );
        drop(mutation);
        assert_eq!(engine.graph(), document);
        assert_eq!(fs::read(fixture.path()).unwrap(), disk);
        assert!(engine.graph_catalog().unwrap().persistence_error.is_some());
        engine.catalog.lock().as_mut().unwrap().path = fixture.path();
        let mutation = engine.mutation.lock();
        engine
            .checkpoint_processor_state_with(Some("effect"), |_, _| Ok(native.clone()))
            .unwrap();
        drop(mutation);
        assert_eq!(engine.graph().revision, document.revision);
        assert_eq!(provider_parameters(&engine.graph()), native);
        assert!(engine.graph_catalog().unwrap().persistence_error.is_none());
        assert_ne!(fs::read(fixture.path()).unwrap(), disk);
    }
}

#[test]
fn native_editor_checkpoint_rejects_missing_nodes_and_pending_graph_before_snapshot() {
    let fixture = Fixture::new();
    let (engine, _) = parameter_fixture(parameter_document());
    let engine = engine.with_graph_catalog_path(fixture.path());
    assert_eq!(
        engine
            .checkpoint_processor_state(Some("missing"))
            .unwrap_err()
            .code,
        ErrorCode::GraphInvalid
    );
    let mut document = engine.graph();
    document.revision += 1;
    engine
        .apply_graph_locked(document.clone(), engine.catalog_candidate(&document))
        .unwrap();
    let disk = fs::read(fixture.path()).unwrap();
    let mutation = engine.mutation.lock();
    assert!(
        engine
            .checkpoint_processor_state_with(None, |_, _| panic!("pending graph captured"))
            .is_err()
    );
    drop(mutation);
    assert_eq!(engine.graph(), document);
    assert_eq!(fs::read(fixture.path()).unwrap(), disk);
}
