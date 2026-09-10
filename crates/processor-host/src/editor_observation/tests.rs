use super::*;
use crate::shared_transport::WorkerControl;
use crate::*;
use mixrelay_test_support::FixtureDir;

struct Fixture {
    control: Arc<HostedProcessorControl>,
    worker: Arc<WorkerControl>,
    _directory: FixtureDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = FixtureDir::new("editor-observation").unwrap();
        let path = directory.join("audio");
        let (audio, mailbox) = shared_transport::create(
            &path,
            AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 4,
            },
        )
        .unwrap();
        drop(audio);
        let (audio, worker) = shared_transport::worker(&path).unwrap();
        drop(audio);
        let control = Arc::new(HostedProcessorControl::new(mailbox));
        worker
            .header()
            .worker_status
            .store(WORKER_READY, Ordering::Release);
        Self {
            control,
            worker: Arc::new(worker),
            _directory: directory,
        }
    }

    fn control(&self) -> &Arc<HostedProcessorControl> {
        &self.control
    }

    fn observed(&self, open: bool) {
        publish_observed_editor_snapshot(
            self.worker.header(),
            rack_formats::EditorSnapshot {
                open,
                ..rack_formats::EditorSnapshot::default()
            },
        );
    }
}

#[test]
fn failed_open_survives_idle_ticks_until_an_explicit_editor_command() {
    let fixture = Fixture::new();
    let control = fixture.control();
    let header = fixture.worker.header();
    let node = NodeId::from("effect");
    for failure in [EDITOR_FAILURE_PLUGIN_REJECTED, EDITOR_FAILURE_HOST_WINDOW] {
        publish_editor_failure(header, failure);
        for _ in 0..20 {
            fixture.observed(false);
            assert_eq!(
                control.editor_status(&node).state,
                ProcessorEditorState::Failed
            );
            assert!(control.editor_status(&node).failure.is_some());
        }
        control.set_editor_open(true);
        fixture.observed(true);
        assert_eq!(
            control.editor_status(&node).state,
            ProcessorEditorState::Open
        );
        assert!(control.editor_status(&node).failure.is_none());
        publish_editor_failure(header, failure);
        control.set_editor_open(false);
        fixture.observed(false);
        assert_eq!(
            control.editor_status(&node).state,
            ProcessorEditorState::Closed
        );
        assert!(control.editor_status(&node).failure.is_none());
    }
}

#[test]
fn requested_close_is_not_an_observation_and_native_close_is_not_consumed_by_readers() {
    let fixture = Fixture::new();
    let registry = ProcessorControlRegistry::default();
    let node = NodeId::from("effect");
    let _registration = registry.register(&node, fixture.control());
    assert_eq!(
        registry
            .editor_close_observation(&node)
            .unwrap()
            .close_generation,
        0
    );
    fixture.observed(false);
    fixture.observed(true);
    registry.set_editor_open(&node, false).unwrap();
    assert_eq!(
        registry.editor_status(&node).unwrap().state,
        ProcessorEditorState::Open
    );
    assert_eq!(
        registry
            .editor_close_observation(&node)
            .unwrap()
            .close_generation,
        0
    );
    fixture.observed(false);
    let closed = registry.editor_close_observation(&node).unwrap();
    assert_eq!(closed.close_generation, 1);
    assert_eq!(registry.editor_close_observation(&node).unwrap(), closed);
    fixture.observed(false);
    assert_eq!(registry.editor_close_observation(&node).unwrap(), closed);
    fixture.observed(true);
    fixture.observed(false);
    assert_eq!(
        registry
            .editor_close_observation(&node)
            .unwrap()
            .close_generation,
        2
    );
    fixture
        .worker
        .header()
        .worker_status
        .store(WORKER_FAILED, Ordering::Release);
    assert!(registry.editor_close_observation(&node).is_err());
    assert_eq!(
        registry.editor_status(&node).unwrap().state,
        ProcessorEditorState::Failed
    );
}

#[test]
fn stale_close_tokens_cannot_snapshot_replaced_workers_or_newer_events() {
    let first = Fixture::new();
    let second = Fixture::new();
    let registry = ProcessorControlRegistry::default();
    let node = NodeId::from("effect");
    let _first = registry.register(&node, first.control());
    first.observed(true);
    first.observed(false);
    let old = registry.editor_close_observation(&node).unwrap();
    first.observed(true);
    first.observed(false);
    assert_eq!(
        registry.snapshot_state_for_editor_close(&node, old, Duration::ZERO),
        Err(ProcessorStateError::Unavailable)
    );
    let latest = registry.editor_close_observation(&node).unwrap();
    let _second = registry.register(&node, second.control());
    second.observed(true);
    second.observed(false);
    assert_ne!(
        latest.instance_id,
        registry
            .editor_close_observation(&node)
            .unwrap()
            .instance_id
    );
    assert_eq!(
        registry.snapshot_state_for_editor_close(&node, latest, Duration::ZERO),
        Err(ProcessorStateError::Unavailable)
    );
    assert!(second.worker.pending_request().is_none());
}

#[test]
fn matching_close_token_exports_native_state_and_stays_retryable() {
    let fixture = Fixture::new();
    let registry = ProcessorControlRegistry::default();
    let node = NodeId::from("effect");
    let _registration = registry.register(&node, fixture.control());
    fixture.observed(true);
    fixture.observed(false);
    let observation = registry.editor_close_observation(&node).unwrap();
    let control = Arc::clone(&fixture.worker);
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while control.pending_request().is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let (request, kind) = control.pending_request().unwrap();
        assert_eq!(kind, crate::editor_surface::NATIVE_STATE);
        control.publish(request, Ok(vec![1, 2, 3]));
    });
    let state =
        registry.snapshot_state_for_editor_close(&node, observation, Duration::from_secs(1));
    worker.join().unwrap();
    assert_eq!(state.unwrap(), serde_json::json!({"stateBase64":"AQID"}));
    assert_eq!(
        registry.editor_close_observation(&node).unwrap(),
        observation
    );
}

#[test]
fn replacement_during_export_cannot_publish_state_into_the_new_instance() {
    let first = Fixture::new();
    let replacement = Fixture::new();
    let registry = ProcessorControlRegistry::default();
    let node = NodeId::from("effect");
    let _first_registration = registry.register(&node, first.control());
    first.observed(true);
    first.observed(false);
    let observation = registry.editor_close_observation(&node).unwrap();
    let first_control = Arc::clone(&first.worker);
    let replacement_control = Arc::clone(replacement.control());
    let worker_registry = registry.clone();
    let worker_node = node.clone();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while first_control.pending_request().is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let _new_registration = worker_registry.register(&worker_node, &replacement_control);
        let (request, kind) = first_control.pending_request().unwrap();
        assert_eq!(kind, crate::editor_surface::NATIVE_STATE);
        first_control.publish(request, Ok(vec![7]));
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let result =
        registry.snapshot_state_for_editor_close(&node, observation, Duration::from_secs(1));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(result, Err(ProcessorStateError::Unavailable));
}
