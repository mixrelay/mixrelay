use super::*;
use mixrelay_test_support::FixtureDir;

#[test]
fn worker_configuration_restores_large_state_from_a_small_reference() {
    let fixture = FixtureDir::new("processor-state-startup").unwrap();
    let store = StateStore::new(fixture.join("states"));
    let bytes = vec![0xa5; 9 * 1024 * 1024];
    let reference = store.put(&bytes).unwrap();
    let parameters = serde_json::json!({"stateRef":reference,"parameterValues":{"7":0.4}});
    let path = fixture.join("worker.json");
    let ports = mixrelay_audio_graph::ProviderPorts::canonical_stereo("fixture-stereo");
    write_worker_config(&path, 48_000, &parameters, Some(&store), &ports).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() < 1024);
    let prepared = read_worker_configuration(path.to_str().unwrap()).unwrap();
    assert_eq!(prepared.sample_rate.to_bits(), 48_000.0_f64.to_bits());
    assert_eq!(prepared.ports, ports);
    assert_eq!(prepared.initial.native.unwrap(), bytes);
    assert_eq!(prepared.initial.parameters, parameters);
}

#[test]
fn missing_state_or_unconfigured_store_cannot_start_with_default_state() {
    let fixture = FixtureDir::new("processor-state-missing").unwrap();
    let store = StateStore::new(fixture.join("states"));
    let reference = store.put(&[1, 2, 3]).unwrap();
    let parameters = serde_json::json!({"stateRef":reference});
    assert!(InitialState::resolve(parameters.clone(), None).is_err());
    let empty = StateStore::new(fixture.join("empty"));
    assert!(InitialState::resolve(parameters, Some(&empty)).is_err());
    for value in [
        Value::Null,
        serde_json::json!({"sha256":"../escape","byteLength":3}),
    ] {
        assert!(
            InitialState::resolve(serde_json::json!({"stateRef":value}), Some(&store)).is_err()
        );
    }
}

#[test]
fn capture_interns_before_reuse_matching_and_keeps_json_small() {
    let fixture = StateCaptureFixture::new("processor-state-checkpoint");
    let transaction = fixture.registry.reuse_transaction();
    let parameters = fixture.capture(vec![0x5a; 9 * 1024 * 1024]);
    assert!(serde_json::to_vec(&parameters).unwrap().len() < 200);
    fixture.registry.verify_state(&parameters).unwrap();
    let candidate = fixture.candidate(parameters);
    assert_eq!(
        fixture.reuse_identity(&candidate),
        Some(fixture.instance_id())
    );
    transaction.commit();
}

struct StateCaptureFixture {
    control: Option<std::sync::Arc<HostedProcessorControl>>,
    worker: std::sync::Arc<crate::shared_transport::WorkerControl>,
    registration: Option<crate::ProcessorControlRegistration>,
    _audio: crate::shared_transport::ParentAudio,
    registry: ProcessorControlRegistry,
    node: crate::NodeId,
    config: crate::ProviderNodeConfig,
    format: crate::RenderFormat,
    _directory: FixtureDir,
}

impl StateCaptureFixture {
    fn new(label: &str) -> Self {
        use crate::{AudioLayout, NodeId, ProviderNodeConfig, RenderFormat};
        use std::sync::{Arc, atomic::Ordering};

        let directory = FixtureDir::new(label).unwrap();
        let path = directory.join("audio");
        let (audio, mailbox) = crate::shared_transport::create(
            &path,
            AudioLayout {
                inputs: 2,
                outputs: 2,
                block_frames: 128,
            },
        )
        .unwrap();
        let control = Arc::new(HostedProcessorControl::new(mailbox));
        control.active_owner.store(true, Ordering::Release);
        control
            .shared
            .header()
            .worker_status
            .store(crate::WORKER_READY, Ordering::Release);
        let node = NodeId::from("effect");
        let registry = ProcessorControlRegistry::default();
        registry.configure_state_store(directory.join("states"));
        let registration = registry.register(&node, &control);
        let config = ProviderNodeConfig {
            type_id: "vst3:fixture".into(),
            ports: None,
            parameters: serde_json::json!({"nativeToken":"original"}),
        };
        let format = RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        };
        let request = crate::HostedProcessorPrepareRequest {
            node_id: &node,
            config: &config,
            format,
            input_channels: 2,
            output_channels: 2,
        };
        *control.reuse.lock().unwrap() = Some(crate::reuse::ReuseSignature::new(&request));
        let (_, worker) = crate::shared_transport::worker(&path).unwrap();

        Self {
            control: Some(control),
            worker: Arc::new(worker),
            registration: Some(registration),
            _audio: audio,
            registry,
            node,
            config,
            format,
            _directory: directory,
        }
    }

    fn instance_id(&self) -> u64 {
        self.control.as_ref().unwrap().shared.header().instance_id
    }

    fn candidate(&self, parameters: Value) -> crate::ProviderNodeConfig {
        let mut candidate = self.config.clone();
        candidate.parameters = parameters;
        candidate
    }

    fn reuse_identity(&self, config: &crate::ProviderNodeConfig) -> Option<u64> {
        let request = crate::HostedProcessorPrepareRequest {
            node_id: &self.node,
            config,
            format: self.format,
            input_channels: 2,
            output_channels: 2,
        };
        self.registry
            .reuse_request(&request)
            .and_then(|processor| processor.reuse_identity())
    }

    fn capture(&self, bytes: Vec<u8>) -> Value {
        let worker = std::sync::Arc::clone(&self.worker);
        let response = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some((request, kind)) = worker.pending_request() {
                    assert_eq!(kind, crate::editor_surface::NATIVE_STATE);
                    worker.publish(request, Ok(bytes));
                    return;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        });
        let parameters = self
            .registry
            .snapshot_state(&self.node, Duration::from_secs(5))
            .unwrap();
        response.join().unwrap();
        parameters
    }
}

#[test]
fn failed_or_export_only_capture_rolls_back_candidate_reuse() {
    let fixture = StateCaptureFixture::new("processor-state-export-only");
    let old_identity = fixture.instance_id();
    let mut failed_candidate = None;
    let mut prepare = || -> Result<(), &str> {
        let _transaction = fixture.registry.reuse_transaction();
        let candidate = fixture.candidate(fixture.capture(vec![4, 5, 6]));
        assert_eq!(fixture.reuse_identity(&candidate), Some(old_identity));
        failed_candidate = Some(candidate);
        Err("forced graph preparation failure")
    };
    let failure = prepare();
    assert_eq!(failure, Err("forced graph preparation failure"));
    let failed_candidate = failed_candidate.unwrap();

    assert_eq!(fixture.reuse_identity(&fixture.config), Some(old_identity));
    assert_eq!(fixture.reuse_identity(&failed_candidate), None);

    let transaction = fixture.registry.reuse_transaction();
    let exported_only = fixture.candidate(fixture.capture(vec![1, 2, 3]));
    assert_eq!(fixture.reuse_identity(&exported_only), Some(old_identity));
    drop(transaction);

    assert_eq!(fixture.reuse_identity(&fixture.config), Some(old_identity));
    assert_eq!(fixture.reuse_identity(&exported_only), None);
}

#[test]
fn committed_capture_promotes_candidate_reuse_signature() {
    let fixture = StateCaptureFixture::new("processor-state-commit");
    let old_identity = fixture.instance_id();
    let transaction = fixture.registry.reuse_transaction();
    let candidate = fixture.candidate(fixture.capture(vec![7, 8, 9]));

    transaction.commit();

    assert_eq!(fixture.reuse_identity(&fixture.config), None);
    assert_eq!(fixture.reuse_identity(&candidate), Some(old_identity));
}

#[test]
fn inner_commit_remains_reversible_by_outer_reuse_transaction() {
    let fixture = StateCaptureFixture::new("processor-state-nested");
    let old_identity = fixture.instance_id();
    let outer = fixture.registry.reuse_transaction();
    let first = fixture.candidate(fixture.capture(vec![1, 2, 3]));
    let inner = fixture.registry.reuse_transaction();
    let second = fixture.candidate(fixture.capture(vec![4, 5, 6]));

    inner.commit();
    assert_eq!(fixture.reuse_identity(&second), Some(old_identity));
    drop(outer);

    assert_eq!(fixture.reuse_identity(&fixture.config), Some(old_identity));
    assert_eq!(fixture.reuse_identity(&first), None);
    assert_eq!(fixture.reuse_identity(&second), None);
}

#[test]
fn reuse_transaction_weak_references_do_not_retain_removed_controls() {
    let mut fixture = StateCaptureFixture::new("processor-state-weak-refs");
    let control = fixture.control.as_ref().unwrap();
    let weak = std::sync::Arc::downgrade(control);
    let transaction = fixture.registry.reuse_transaction();

    drop(fixture.registration.take());
    drop(fixture.control.take());

    assert!(weak.upgrade().is_none());
    drop(transaction);
    assert!(weak.upgrade().is_none());
}
