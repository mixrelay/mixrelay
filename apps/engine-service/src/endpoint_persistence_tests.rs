use super::*;
use crate::endpoint_settings_tests::{controller, endpoint_document, settings};
use std::{fs, path::PathBuf};
use tonenari_control_protocol::{GraphCatalogAction, GraphCatalogRequest};

struct Fixture(tonenari_test_support::FixtureDir);

impl Fixture {
    fn new() -> Self {
        Self(tonenari_test_support::FixtureDir::new("endpoint-catalog").unwrap())
    }

    fn path(&self) -> PathBuf {
        self.0.join("graphs.json")
    }

    fn restart(&self) -> EngineGraphController {
        controller(GraphDocument::new(1, vec![], vec![])).with_graph_catalog_path(self.path())
    }
}

fn settle(controller: &EngineGraphController) {
    let mut processor = controller
        .prepare_and_validate(&controller.graph(), *controller.context.read())
        .unwrap()
        .processor;
    assert!(controller.exchange.activate_pending(&mut processor));
    controller.exchange.reclaim_retired();
}

fn select_view(controller: &EngineGraphController, action: GraphCatalogAction) {
    let catalog = controller.graph_catalog().unwrap();
    let before = controller.exchange.revisions();
    controller
        .mutate_graph_catalog(GraphCatalogRequest {
            expected_revision: controller.graph().revision,
            expected_catalog_revision: catalog.catalog_revision,
            action,
        })
        .unwrap();
    assert_eq!(controller.exchange.revisions(), before);
}

#[test]
fn catalog_restart_restores_device_format_name_and_graph_local_reference() {
    let fixture = Fixture::new();
    let first = fixture.restart();
    let mut document = endpoint_document(2, "SPAT send-return");
    let NodeConfig::VirtualEndpoint(config) = &mut document.nodes[0].config else {
        panic!("endpoint fixture")
    };
    config.reference.capture = false;
    config.gain_db = -6.0;
    first.apply_graph(document.clone()).unwrap();
    let expected = settings(&first).devices;
    drop(first);

    let next = fixture.restart();
    assert_eq!(settings(&next).devices, expected);
    assert_eq!(next.graph().nodes, document.nodes);
    assert!(next.graph().edges.is_empty());
    assert_eq!(next.graph().revision, 3);
    assert_eq!(next.graph_status().active_revision, 1);
    settle(&next);
    assert_eq!(next.graph_status().active_revision, 3);
    assert_eq!(fs::read_dir(fixture.0.path()).unwrap().count(), 1);
}

#[test]
fn final_reference_removal_persists_without_recreating_a_device_on_restart() {
    let fixture = Fixture::new();
    let first = fixture.restart();
    let template = settings(&first).creation_template;
    first
        .apply_graph(endpoint_document(2, "Temporary device"))
        .unwrap();
    settle(&first);
    first
        .apply_graph(GraphDocument::new(3, vec![], vec![]))
        .unwrap();
    drop(first);

    let next = fixture.restart();
    assert!(next.graph().nodes.is_empty());
    assert!(settings(&next).devices.is_empty());
    assert!(settings(&next).can_create);
    assert_eq!(settings(&next).creation_template, template);
    settle(&next);
    drop(next);
    assert!(settings(&fixture.restart()).devices.is_empty());
}

#[test]
fn device_lifetime_counts_references_in_every_saved_graph_across_restart() {
    let fixture = Fixture::new();
    let engine = fixture.restart();
    engine
        .apply_graph(endpoint_document(2, "Shared device"))
        .unwrap();
    settle(&engine);
    let first_id = engine.graph_catalog().unwrap().active_id;
    select_view(
        &engine,
        GraphCatalogAction::Create {
            name: "Second view".into(),
        },
    );
    assert!(engine.graph().nodes.is_empty());
    assert_eq!(settings(&engine).devices.len(), 1);
    engine
        .apply_graph(endpoint_document(
            engine.graph().revision + 1,
            "Shared device",
        ))
        .unwrap();
    settle(&engine);
    engine
        .apply_graph(GraphDocument::new(
            engine.graph().revision + 1,
            vec![],
            vec![],
        ))
        .unwrap();
    assert_eq!(settings(&engine).devices.len(), 1);
    drop(engine);

    let restarted = fixture.restart();
    assert!(restarted.graph().nodes.is_empty());
    assert_eq!(settings(&restarted).devices.len(), 1);
    settle(&restarted);
    select_view(&restarted, GraphCatalogAction::Select { id: first_id });
    restarted
        .apply_graph(GraphDocument::new(
            restarted.graph().revision + 1,
            vec![],
            vec![],
        ))
        .unwrap();
    assert!(settings(&restarted).devices.is_empty());
    drop(restarted);
    let final_restart = fixture.restart();
    assert!(settings(&final_restart).devices.is_empty());
    assert!(settings(&final_restart).can_create);
    assert_eq!(final_restart.graph_catalog().unwrap().graphs.len(), 2);
}

#[test]
fn native_staging_rejection_preserves_catalog_bytes_and_current_device() {
    let fixture = Fixture::new();
    let first = fixture.restart();
    first.apply_graph(endpoint_document(2, "Accepted")).unwrap();
    settle(&first);
    let original = fs::read(fixture.path()).unwrap();
    let expected = first.graph();
    assert!(first.apply_graph(endpoint_document(3, "rejected")).is_err());
    assert_eq!(first.graph(), expected);
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    assert_eq!(fs::read_dir(fixture.0.path()).unwrap().count(), 1);
    drop(first);
    assert_eq!(fixture.restart().graph().nodes, expected.nodes);
}

#[test]
fn catalog_staging_failure_preserves_disk_memory_and_allows_retry() {
    let fixture = Fixture::new();
    let first = fixture.restart();
    first.apply_graph(endpoint_document(2, "Accepted")).unwrap();
    settle(&first);
    let original = fs::read(fixture.path()).unwrap();
    let expected = first.graph();
    let blocked = fixture.0.join("not-a-directory");
    fs::write(&blocked, b"blocking fixture").unwrap();
    first.catalog.lock().as_mut().unwrap().path = blocked.join("graphs.json");

    let error = first
        .apply_graph(endpoint_document(3, "Unsaved"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlUnavailable);
    assert_eq!(first.graph(), expected);
    assert_eq!(first.graph_catalog().unwrap().document, expected);
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    first.catalog.lock().as_mut().unwrap().path = fixture.path();
    first
        .apply_graph(endpoint_document(3, "Recovered"))
        .unwrap();
    drop(first);
    assert_eq!(
        settings(&fixture.restart()).devices[0].spec.display_name,
        "Recovered"
    );
}

#[test]
fn corrupt_catalog_is_untouched_and_reports_an_observable_error() {
    let fixture = Fixture::new();
    let original = b"{broken catalog";
    fs::write(fixture.path(), original).unwrap();
    let engine = fixture.restart();
    assert!(engine.graph().nodes.is_empty());
    let error = engine.graph_catalog().unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlUnavailable);
    assert_eq!(error.parameters["resource"], "graph_catalog");
    assert!(!error.parameters["reason"].as_str().unwrap().is_empty());
    engine
        .apply_graph(endpoint_document(2, "Volatile device"))
        .unwrap();
    assert!(engine.graph_catalog().is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    drop(engine);
    assert!(fixture.restart().graph_catalog().is_err());
    assert_eq!(fs::read_dir(fixture.0.path()).unwrap().count(), 1);
}

#[test]
fn failed_native_restore_keeps_saved_catalog_and_reports_failure() {
    let fixture = Fixture::new();
    let engine = fixture.restart();
    engine
        .apply_graph(endpoint_document(2, "Accepted"))
        .unwrap();
    drop(engine);
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
    value["graphs"][0]["document"]["nodes"][0]["config"]["displayName"] = "rejected".into();
    let original = serde_json::to_vec(&value).unwrap();
    fs::write(fixture.path(), &original).unwrap();
    let restarted = fixture.restart();
    assert!(restarted.graph().nodes.is_empty());
    let error = restarted.graph_catalog().unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlUnavailable);
    assert!(
        error.parameters["reason"]
            .as_str()
            .unwrap()
            .contains("saved graph restore failed")
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
}
