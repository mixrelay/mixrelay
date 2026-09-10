use super::*;
use tonenari_control_protocol::GraphProcessorParameterChange;
use tonenari_processor_host::ProcessorStateError;

fn change(id: u32, value: f64) -> GraphProcessorParameterChange {
    GraphProcessorParameterChange {
        node_id: "effect".into(),
        type_id: "fixture:parameters".into(),
        parameter_id: id,
        value,
    }
}

#[test]
fn pending_startup_preserves_desired_provider_state_without_snapshotting_missing_worker() {
    let (engine, _) = parameter_fixture(parameter_document());
    engine.defer_catalog_restore(ProtocolError {
        code: ErrorCode::AudioDeviceDisconnected,
        parameters: serde_json::json!({"reason": "injected startup timeout"}),
    });
    let mut proposed = engine.graph();
    proposed.revision += 1;
    let original = proposed.clone();
    let NodeConfig::Provider(provider) = &mut proposed.nodes[0].config else {
        panic!("fixture must contain the saved provider");
    };
    provider.parameters = serde_json::json!({"stateBase64":"STALE"});
    engine
        .preserve_live_processor_state_with(
            &mut proposed,
            &[],
            |_, _| panic!("an unstarted worker must not be snapshotted"),
            |_, _| true,
        )
        .unwrap();
    assert_eq!(proposed, original);
    let NodeConfig::Provider(provider) = &mut proposed.nodes[0].config else {
        panic!("fixture must contain the saved provider");
    };
    provider.parameters = serde_json::json!({"stateBase64":"STALE"});
    engine
        .preserve_live_processor_state_with(
            &mut proposed,
            &[change(7, 0.25)],
            |_, _| panic!("an unstarted worker must not be snapshotted"),
            |_, _| true,
        )
        .unwrap();
    assert_eq!(provider_parameters(&proposed)["parameterValues"]["7"], 0.25);
    let mut expected = original;
    let NodeConfig::Provider(provider) = &mut expected.nodes[0].config else {
        panic!("fixture must contain the saved provider");
    };
    crate::persist_processor_parameter(provider, 7, 0.25);
    assert_eq!(proposed, expected);
}

#[test]
fn structural_edits_preserve_native_preset_and_only_explicit_undo_deltas_overlay_it() {
    let (engine, _) = parameter_fixture(parameter_document());
    for changes in [vec![], vec![change(7, 0.25)], vec![change(7, 0.75)]] {
        let mut proposed = engine.graph();
        proposed.revision += 1;
        if let NodeConfig::Provider(provider) = &mut proposed.nodes[0].config {
            provider.parameters = serde_json::json!({
                "stateBase64":"STALE", "parameterValues":{"7":0.1,"9":0.3}
            });
        }
        let mutation = engine.mutation.lock();
        engine
            .preserve_live_processor_state_with(
                &mut proposed,
                &changes,
                |_, _| Ok(serde_json::json!({"stateBase64":"Ag=="})),
                |_, _| true,
            )
            .unwrap();
        drop(mutation);
        let parameters = provider_parameters(&proposed);
        assert_eq!(parameters["stateBase64"], "Ag==");
        assert!(parameters["parameterValues"].get("9").is_none());
        if let Some(edit) = changes.first() {
            assert_eq!(parameters["parameterValues"]["7"], edit.value);
        } else {
            assert!(parameters.get("parameterValues").is_none());
        }
        assert_eq!(engine.graph(), parameter_document());
    }
}

#[test]
fn new_or_replaced_provider_uses_requested_initial_state_without_capturing_old_instance() {
    let (engine, _) = parameter_fixture(parameter_document());
    for replace_type in [false, true] {
        let mut proposed = engine.graph();
        proposed.revision += 1;
        if replace_type {
            let NodeConfig::Provider(provider) = &mut proposed.nodes[0].config else {
                panic!()
            };
            provider.type_id = "fixture:new".into();
        } else {
            proposed.nodes[0].id = NodeId::from("new");
        }
        let original = proposed.clone();
        let mutation = engine.mutation.lock();
        engine
            .preserve_live_processor_state_with(
                &mut proposed,
                &[],
                |_, _| panic!("new instance captured"),
                |_, _| true,
            )
            .unwrap();
        drop(mutation);
        assert_eq!(proposed, original);
    }
}

#[test]
fn failed_second_snapshot_does_not_partially_rewrite_proposed_graph() {
    let mut document = parameter_document();
    let mut second = document.nodes[0].clone();
    second.id = NodeId::from("second");
    document.nodes.push(second);
    let (engine, _) = parameter_fixture(document);
    let mut proposed = engine.graph();
    proposed.revision += 1;
    let original = proposed.clone();
    let mutation = engine.mutation.lock();
    assert!(
        engine
            .preserve_live_processor_state_with(
                &mut proposed,
                &[],
                |id, _| {
                    if *id
                        == crate::graph_composition::runtime_node_id(
                            "session",
                            &NodeId::from("effect"),
                        )
                    {
                        Ok(serde_json::json!({"stateBase64":"Ag=="}))
                    } else {
                        Err(ProcessorStateError::Timeout)
                    }
                },
                |_, _| true
            )
            .is_err()
    );
    drop(mutation);
    assert_eq!(proposed, original);
}

#[test]
fn invalid_explicit_deltas_are_rejected_before_snapshot() {
    let (engine, _) = parameter_fixture(parameter_document());
    let mut missing = change(7, 0.5);
    missing.node_id = "missing".into();
    let mut changed_type = change(7, 0.5);
    changed_type.type_id = "fixture:other".into();
    for changes in [
        vec![change(7, f64::NAN)],
        vec![change(7, -1.0)],
        vec![change(7, 2.0)],
        vec![change(7, 0.2), change(7, 0.3)],
        vec![missing],
        vec![changed_type],
        vec![change(7, 0.5); 4097],
    ] {
        let mut proposed = engine.graph();
        proposed.revision += 1;
        let original = proposed.clone();
        let mutation = engine.mutation.lock();
        assert!(
            engine
                .preserve_live_processor_state_with(
                    &mut proposed,
                    &changes,
                    |_, _| panic!("invalid edit captured"),
                    |_, _| true
                )
                .is_err()
        );
        drop(mutation);
        assert_eq!(proposed, original);
    }
    let mut proposed = engine.graph();
    proposed.revision += 1;
    let mutation = engine.mutation.lock();
    assert!(
        engine
            .preserve_live_processor_state_with(
                &mut proposed,
                &[change(7, 0.5)],
                |_, _| panic!("readonly edit captured"),
                |_, _| false
            )
            .is_err()
    );
    drop(mutation);
}

#[test]
fn stale_or_pending_publication_never_snapshots_an_unconfirmed_instance() {
    let (engine, _) = parameter_fixture(parameter_document());
    for revision in [0, 1] {
        let mut proposed = engine.graph();
        proposed.revision = revision;
        let mutation = engine.mutation.lock();
        assert_eq!(
            engine
                .preserve_live_processor_state_with(
                    &mut proposed,
                    &[],
                    |_, _| panic!("stale capture"),
                    |_, _| true
                )
                .unwrap_err()
                .code,
            ErrorCode::GraphRevisionStale
        );
        drop(mutation);
    }
    let mut published = engine.graph();
    published.revision += 1;
    engine.apply_graph_locked(published.clone(), None).unwrap();
    published.revision += 1;
    let mutation = engine.mutation.lock();
    assert!(
        engine
            .preserve_live_processor_state_with(
                &mut published,
                &[],
                |_, _| panic!("pending capture"),
                |_, _| true
            )
            .is_err()
    );
    drop(mutation);
}

#[test]
fn reselecting_graph_does_not_snapshot_or_prepare_but_explicit_checkpoint_still_rejects_unavailable_worker()
 {
    let fixture = Fixture::new();
    let (engine, prepares) = parameter_fixture(parameter_document());
    let engine = engine.with_graph_catalog_path(fixture.path());
    let original = fs::read(fixture.path()).unwrap();
    let request = request(
        &engine,
        GraphCatalogAction::Select {
            id: "graph-1".into(),
        },
    );
    let revision = engine.exchange.revisions();
    let before_prepares = prepares.load(Ordering::Relaxed);
    engine.mutate_catalog(request).unwrap();
    assert_eq!(engine.exchange.revisions(), revision);
    assert_eq!(prepares.load(Ordering::Relaxed), before_prepares);
    assert!(engine.checkpoint_processor_state(None).is_err());
    assert_eq!(engine.graph(), parameter_document());
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    assert!(engine.graph_catalog().unwrap().persistence_error.is_some());
}
