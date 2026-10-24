use super::*;

#[test]
fn three_large_state_references_persist_and_reload_before_processor_prepare() {
    const STATE_BYTES: usize = 9 * 1024 * 1024;
    let fixture = Fixture::new();
    let (engine, prepares) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let engine = engine.with_graph_catalog_path(fixture.path());
    let revisions = engine.exchange.revisions();
    let mut document = parameter_document();
    document.revision = revisions.submitted + 1;
    let template = document.nodes[0].clone();
    let store = engine.processor_controls.state_store().unwrap();
    let references =
        [0x35_u8, 0x76, 0xa9].map(|value| store.put(&vec![value; STATE_BYTES]).unwrap());
    document.nodes = references
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            let mut node = template.clone();
            node.id = NodeId::new(format!("effect-{index}"));
            let NodeConfig::Provider(provider) = &mut node.config else {
                panic!("provider fixture")
            };
            provider.parameters = serde_json::json!({"stateRef": reference});
            node
        })
        .collect();
    engine
        .apply_graph_locked(document.clone(), engine.catalog_candidate(&document))
        .unwrap();
    assert_eq!(prepares.load(Ordering::Relaxed), 1);
    assert_eq!(engine.exchange.revisions().submitted, document.revision);
    assert_eq!(engine.graph(), document);
    for reference in &references {
        store.verify(reference).unwrap();
    }
    assert!(fs::metadata(fixture.path()).unwrap().len() < 16 * 1024);

    drop(engine);
    let (restarted, restarted_prepares) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let restarted = restarted.with_graph_catalog_path(fixture.path());
    assert_eq!(restarted_prepares.load(Ordering::Relaxed), 1);
    assert_eq!(restarted.graph().nodes.len(), 3);
    let restarted_store = restarted.processor_controls.state_store().unwrap();
    for reference in &references {
        restarted_store.verify(reference).unwrap();
    }
    for (node, reference) in restarted.graph().nodes.iter().zip(references) {
        let NodeConfig::Provider(provider) = &node.config else {
            panic!("provider fixture")
        };
        assert_eq!(
            provider.parameters["stateRef"],
            serde_json::to_value(reference).unwrap()
        );
    }
}

#[test]
fn unavailable_catalog_storage_rejects_before_prepare_and_same_revision_can_retry() {
    let fixture = Fixture::new();
    let (engine, prepares) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let engine = engine.with_graph_catalog_path(fixture.path());
    let disk = fs::read(fixture.path()).unwrap();
    let revisions = engine.exchange.revisions();
    let blocked = fixture.0.join("occupied");
    fs::write(&blocked, b"not a directory").unwrap();
    engine.catalog.lock().as_mut().unwrap().path = blocked.join("graphs.json");
    let mut document = parameter_document();
    document.revision = revisions.submitted + 1;
    assert!(
        engine
            .apply_graph_locked(document.clone(), engine.catalog_candidate(&document))
            .is_err()
    );
    assert_eq!(engine.exchange.revisions(), revisions);
    assert_eq!(fs::read(fixture.path()).unwrap(), disk);
    assert_eq!(prepares.load(Ordering::Relaxed), 0);

    engine.catalog.lock().as_mut().unwrap().path = fixture.path();
    engine
        .apply_graph_locked(document.clone(), engine.catalog_candidate(&document))
        .unwrap();
    assert_eq!(prepares.load(Ordering::Relaxed), 1);
    assert_eq!(engine.graph(), document);
}

#[test]
fn invalid_graph_after_successful_staging_removes_only_its_temporary_document() {
    let fixture = Fixture::new();
    let (engine, prepares) = parameter_fixture(GraphDocument::new(1, vec![], vec![]));
    let engine = engine.with_graph_catalog_path(fixture.path());
    let disk = fs::read(fixture.path()).unwrap();
    let revisions = engine.exchange.revisions();
    let mut document = parameter_document();
    document.revision = revisions.submitted + 1;
    document.nodes.push(document.nodes[0].clone());
    assert!(
        engine
            .publish_graph_locked(document.clone(), engine.catalog_candidate(&document))
            .is_err()
    );
    assert_eq!(engine.exchange.revisions(), revisions);
    assert_eq!(fs::read(fixture.path()).unwrap(), disk);
    assert_eq!(
        crate::controller_test_support::stored_files(fixture.0.path()),
        1
    );
    assert_eq!(prepares.load(Ordering::Relaxed), 0);
}
