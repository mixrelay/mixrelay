use super::*;
use crate::ControlBackend;
use mixrelay_audio_graph::{
    AudioEndpoint, ChannelLayout, ChannelMapping, EdgeSpec, NodeSpec, PortId, StreamSpec,
    VirtualEndpointConfig, VirtualEndpointId, VirtualEndpointReference,
};
use mixrelay_control_protocol::{GraphCatalogAction, GraphCatalogRequest};

const DEVICE_A: &str = "31ab4e5a-44e0-4bab-89dd-906e07751ea1";
const DEVICE_B: &str = "31ab4e5a-44e0-4bab-89dd-906e07751ea2";

fn device(id: &str, node: &str, reference: VirtualEndpointReference) -> NodeSpec {
    let stream = StreamSpec {
        channels: 2,
        channel_layout: ChannelLayout::Stereo,
    };
    NodeSpec {
        control: mixrelay_audio_graph::NodeControl::default(),
        id: NodeId::from(node),
        label: None,
        config: NodeConfig::VirtualEndpoint(VirtualEndpointConfig {
            spec: VirtualEndpointSpec {
                id: VirtualEndpointId::from(id),
                display_name: id.to_owned(),
                render: Some(stream),
                capture: Some(stream),
                supported_sample_rates: vec![48_000],
                default_sample_rate: 48_000,
            },
            reference,
            gain_db: 0.0,
        }),
    }
}

fn route(source: (&str, &str), sink: (&str, &str)) -> EdgeSpec {
    EdgeSpec {
        id: "cable".into(),
        source: AudioEndpoint {
            node_id: source.0.into(),
            port_id: PortId::from(source.1),
        },
        destination: AudioEndpoint {
            node_id: sink.0.into(),
            port_id: PortId::from(sink.1),
        },
        channel_mapping: ChannelMapping::Auto,
    }
}

fn graph_a(revision: u64) -> GraphDocument {
    GraphDocument::new(
        revision,
        vec![device(
            DEVICE_A,
            "local",
            VirtualEndpointReference {
                render: true,
                capture: true,
            },
        )],
        vec![],
    )
}

fn graph_b(revision: u64) -> GraphDocument {
    GraphDocument::new(
        revision,
        vec![
            device(
                DEVICE_A,
                "local",
                VirtualEndpointReference {
                    render: true,
                    capture: false,
                },
            ),
            device(
                DEVICE_B,
                "output",
                VirtualEndpointReference {
                    render: false,
                    capture: true,
                },
            ),
        ],
        vec![route(("local", "from_os"), ("output", "to_os"))],
    )
}

fn controller_with_driver_devices() -> EngineGraphController {
    crate::endpoint_settings_tests::controller_with_inventory(
        GraphDocument::new(1, vec![], vec![]),
        &graph_b(1),
    )
}

fn select(engine: &EngineGraphController, action: GraphCatalogAction) {
    let catalog = engine.graph_catalog().unwrap();
    engine
        .mutate_graph_catalog(GraphCatalogRequest {
            expected_revision: engine.graph().revision,
            expected_catalog_revision: catalog.catalog_revision,
            action,
        })
        .unwrap();
}

fn assert_shared_pcm(processor: &mut BlockProcessor) {
    let frames = processor.plan().format().block_frames;
    let samples = frames as usize * 2;
    let signal: Vec<f32> = [0.25, -0.5, 0.75, -0.125]
        .into_iter()
        .cycle()
        .take(samples)
        .collect();
    let inputs: Vec<Vec<f32>> = processor
        .plan()
        .source_bindings()
        .iter()
        .map(|binding| {
            if binding
                .virtual_endpoint_id()
                .is_some_and(|id| id.as_str() == DEVICE_A)
            {
                signal.clone()
            } else {
                vec![0.0; samples]
            }
        })
        .collect();
    let mut outputs = vec![vec![0.0; samples]; processor.plan().sink_bindings().len()];
    // A device bus delays by its prepared quantum, not by the number of calls.
    for _ in 0..2 {
        processor
            .process_block(
                frames,
                &inputs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                &mut outputs
                    .iter_mut()
                    .map(Vec::as_mut_slice)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    assert_eq!(outputs.len(), 2);
    for (binding, output) in processor.plan().sink_bindings().iter().zip(outputs) {
        match binding.virtual_endpoint_id().unwrap().as_str() {
            // A has no graph contribution route. Its basic OS loopback belongs
            // to Driver and must not be copied to graph output a second time.
            DEVICE_A => assert_eq!(output, vec![0.0; samples]),
            DEVICE_B => assert_eq!(output, signal),
            _ => panic!("unexpected Driver sink"),
        }
    }
}

#[test]
fn controller_runs_both_graphs_and_view_switches_do_not_submit_or_interrupt_shared_pcm() {
    let directory = mixrelay_test_support::FixtureDir::new("composed-controller").unwrap();
    let engine =
        controller_with_driver_devices().with_graph_catalog_path(directory.join("graphs.json"));
    let mut processor = engine
        .prepare_and_validate(&engine.graph(), *engine.context.read())
        .unwrap()
        .processor;
    engine.apply_graph_request(graph_a(2).into()).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    let revision = engine.exchange.revisions();
    select(&engine, GraphCatalogAction::Create { name: "B".into() });
    let graph_b_id = engine.view_graph_id();
    assert_eq!(engine.exchange.revisions(), revision);
    engine.apply_graph_request(graph_b(3).into()).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_shared_pcm(&mut processor);
    let revision = engine.exchange.revisions();
    for id in ["graph-1", &graph_b_id, "graph-1", &graph_b_id] {
        select(&engine, GraphCatalogAction::Select { id: id.into() });
        assert_eq!(engine.exchange.revisions(), revision);
        assert!(!engine.exchange.activate_pending(&mut processor));
        assert_shared_pcm(&mut processor);
        assert!(
            engine
                .meters()
                .nodes
                .iter()
                .all(|meter| ["local", "output"].contains(&meter.node_id.as_str()))
        );
    }
    assert_eq!(engine.endpoint_settings().unwrap().devices.len(), 2);
}

#[test]
fn meters_respond_with_pcm_while_native_mutation_is_blocked() {
    use std::{sync::mpsc, time::Duration};

    let engine = controller_with_driver_devices();
    engine.apply_graph_request(graph_b(2).into()).unwrap();
    let mut processor = crate::controller_test_support::settle(&engine);
    assert_shared_pcm(&mut processor);
    let snapshot = std::thread::scope(|scope| {
        let mutation = engine.mutation.lock();
        let (sender, receiver) = mpsc::channel();
        let started = std::time::Instant::now();
        let engine = &engine;
        let reader = scope.spawn(move || sender.send(engine.meters()).unwrap());
        let result = receiver.recv_timeout(Duration::from_secs(2));
        eprintln!(
            "blocked-mutation meter read: {:?}, responded={}",
            started.elapsed(),
            result.is_ok()
        );
        drop(mutation);
        reader.join().unwrap();
        result.expect("meters must not wait for a native graph mutation")
    });
    assert_eq!(snapshot.revision, processor.plan().revision());
    assert!(!snapshot.nodes.is_empty());
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.peaks.iter().any(|peak| *peak > 0.0))
    );
    assert_shared_pcm(&mut processor);
}

#[test]
fn meters_report_applied_controls_without_changing_the_topology_revision() {
    let directory = mixrelay_test_support::FixtureDir::new("meter-control-revision").unwrap();
    let engine =
        controller_with_driver_devices().with_graph_catalog_path(directory.join("graphs.json"));
    engine.apply_graph_request(graph_b(2).into()).unwrap();
    let mut processor = crate::controller_test_support::settle(&engine);
    let topology = processor.plan().revision();
    let response = engine
        .change_node_control(
            "graph-1",
            "local",
            &mixrelay_control_protocol::NodeControlRequest {
                expected_revision: topology,
                enabled: None,
                muted: None,
                gain_db: Some(-6.0),
            },
        )
        .unwrap();
    assert!(!response.requires_graph_reload);
    let frames = processor.plan().format().block_frames;
    let inputs = vec![vec![0.25; frames as usize * 2]; processor.plan().source_bindings().len()];
    let mut outputs = vec![vec![0.0; frames as usize * 2]; processor.plan().sink_bindings().len()];
    for _ in 0..2 {
        processor
            .process_block(
                frames,
                &inputs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                &mut outputs
                    .iter_mut()
                    .map(Vec::as_mut_slice)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    let snapshot = engine.meters();
    assert_eq!(processor.plan().revision(), topology);
    assert_eq!(snapshot.revision, engine.exchange.revisions().active);
    assert!(snapshot.revision > topology);
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.peaks.iter().any(|peak| *peak > 0.0))
    );
}

#[test]
fn deleted_selection_is_committed_before_the_new_plan_can_activate() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    let directory = mixrelay_test_support::FixtureDir::new("meter-selection-publication").unwrap();
    let engine =
        controller_with_driver_devices().with_graph_catalog_path(directory.join("graphs.json"));
    engine.apply_graph_request(graph_a(2).into()).unwrap();
    let _first = crate::controller_test_support::settle(&engine);
    select(&engine, GraphCatalogAction::Create { name: "B".into() });
    engine.apply_graph_request(graph_b(3).into()).unwrap();
    let mut processor = crate::controller_test_support::settle(&engine);
    select(
        &engine,
        GraphCatalogAction::Select {
            id: "graph-1".into(),
        },
    );
    let document = graph_b(4);
    let candidate = serde_json::from_value(serde_json::json!({
        "version":1,"revision":4,"activeId":"remaining",
        "graphs":[{"id":"remaining","name":"B","document":document}]
    }))
    .unwrap();
    let context = *engine.context.read();
    let PreparedExecution {
        processor: next_processor,
        view,
        ..
    } = engine
        .prepare_execution(&document, Some(&candidate), context)
        .unwrap();
    let bindings = next_processor.plan().meter_bindings().to_vec();
    let (started, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    std::thread::scope(|scope| {
        let engine = &engine;
        let publisher = scope.spawn(move || {
            engine.exchange.submit_configured(next_processor, || {
                started.send(()).unwrap();
                blocked.recv().unwrap();
                engine.publish_execution_metadata(document.revision, bindings, view);
                *engine.graph.write() = document;
                engine.replace_catalog_memory(Some(candidate));
                Ok::<(), ()>(())
            })
        });
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let catalog = engine.catalog.lock();
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let next = 4;
        while !engine.execution_views.read().contains_key(&next) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let metadata_ready = engine.execution_views.read().contains_key(&next);
        let pending_before_selection_commit = engine.exchange.has_pending();
        drop(catalog);
        publisher.join().unwrap().unwrap();
        assert!(metadata_ready);
        assert!(
            !pending_before_selection_commit,
            "new topology cannot precede its selected graph"
        );
    });
    engine.refresh_control_snapshot();
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_ne!(engine.view_graph_id(), "graph-1");
    assert_shared_pcm(&mut processor);
    let snapshot = engine.meters();
    assert!(!snapshot.nodes.is_empty());
    assert_eq!(snapshot.revision, processor.plan().revision());
}

#[test]
fn self_device_routing_is_accepted_without_feedback_policy() {
    let directory = mixrelay_test_support::FixtureDir::new("composed-feedback").unwrap();
    let engine =
        controller_with_driver_devices().with_graph_catalog_path(directory.join("graphs.json"));
    let mut processor = engine
        .prepare_and_validate(&engine.graph(), *engine.context.read())
        .unwrap()
        .processor;
    engine.apply_graph_request(graph_a(2).into()).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    select(&engine, GraphCatalogAction::Create { name: "B".into() });
    let mut feedback = graph_b(3);
    let NodeConfig::VirtualEndpoint(config) = &mut feedback.nodes[1].config else {
        panic!("device");
    };
    config.spec.id = VirtualEndpointId::from(DEVICE_A);
    config.spec.display_name = DEVICE_A.to_owned();
    engine.apply_graph_request(feedback.into()).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_eq!(engine.endpoint_settings().unwrap().devices.len(), 2);
}

#[test]
fn catalog_removal_keeps_shared_device_wiring_and_pcm_in_surviving_graph() {
    let directory = mixrelay_test_support::FixtureDir::new("remove-shared-controller").unwrap();
    let engine =
        controller_with_driver_devices().with_graph_catalog_path(directory.join("graphs.json"));
    engine.apply_graph_request(graph_a(2).into()).unwrap();
    let mut processor = crate::controller_test_support::settle(&engine);
    select(
        &engine,
        GraphCatalogAction::Create {
            name: "Keep".into(),
        },
    );
    engine.apply_graph_request(graph_b(3).into()).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_shared_pcm(&mut processor);
    let before = engine.graph();
    select(
        &engine,
        GraphCatalogAction::Delete {
            id: "graph-1".into(),
        },
    );
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_eq!(engine.graph().nodes, before.nodes);
    assert_eq!(engine.graph().edges, before.edges);
    assert_eq!(engine.endpoint_settings().unwrap().devices.len(), 2);
    // Both duplex devices are still referenced by B. Their device-owned loopback
    // survives deletion of A, as does B's explicit route between the devices.
    assert_shared_pcm(&mut processor);
}

#[derive(Default)]
struct ManifestNativeProbe {
    inventories: std::sync::atomic::AtomicUsize,
    application_resolutions: std::sync::atomic::AtomicUsize,
}

impl GraphResourceBackend for ManifestNativeProbe {
    fn device_inventory(
        &self,
        _: &[&mixrelay_audio_io::DeviceSelector],
    ) -> Result<
        Vec<mixrelay_audio_io::AudioDeviceDescriptor>,
        mixrelay_audio_io::HostAudioBackendError,
    > {
        self.inventories
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Vec::new())
    }

    fn resolve_application_capture(
        &self,
        request: &mixrelay_audio_io::ApplicationCaptureRequest,
    ) -> Result<crate::graph_composition::SharedSource, mixrelay_audio_io::HostAudioBackendError>
    {
        self.application_resolutions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut canonical = request.clone();
        canonical.gain_db = 0.0;
        Ok(crate::graph_composition::SharedSource {
            id: "io.mixrelay.manifest-fixture".into(),
            config: NodeConfig::ApplicationCapture(canonical),
        })
    }

    fn reconcile(
        &self,
        _: &GraphDocument,
        _: crate::PlaybackBindings<'_>,
    ) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        Ok(())
    }
}

fn manifest_application_graph(revision: u64) -> GraphDocument {
    GraphDocument::new(
        revision,
        vec![NodeSpec {
            id: "app".into(),
            control: mixrelay_audio_graph::NodeControl::default(),
            label: None,
            config: NodeConfig::ApplicationCapture(mixrelay_audio_io::ApplicationCaptureRequest {
                selector: mixrelay_audio_io::ApplicationSelector::BundleId {
                    bundle_id: "io.mixrelay.manifest-fixture".into(),
                    include_descendants: false,
                },
                stream: StreamSpec {
                    channels: 2,
                    channel_layout: ChannelLayout::Stereo,
                },
                source_playback: mixrelay_audio_io::SourcePlaybackPolicy::Preserve,
                forwarding_endpoint: None,
                gain_db: -6.0,
                requires_reselection: false,
            }),
        }],
        vec![],
    )
}

#[test]
fn manifest_read_returns_unavailable_without_waiting_for_native_writer() {
    use std::{sync::Arc, sync::mpsc, thread, time::Duration};

    let engine = Arc::new(crate::endpoint_settings_tests::controller(
        GraphDocument::new(1, vec![], vec![]),
    ));
    let _prepared = engine
        .prepare_and_validate(&engine.graph(), *engine.context.read())
        .unwrap();
    let guard = engine.mutation.lock();
    let (sender, receiver) = mpsc::channel();
    let reader = Arc::clone(&engine);
    let worker = thread::spawn(move || sender.send(reader.current_capture_manifest()).unwrap());
    let result = receiver.recv_timeout(Duration::from_secs(1));
    drop(guard);
    worker.join().unwrap();
    let error = result.unwrap().unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlUnavailable);
    assert_eq!(error.parameters["resource"], "application_capture_manifest");
    assert_eq!(error.parameters["reason"], "graph_mutation_in_progress");
}

#[test]
fn unpublished_manifest_never_reprobes_physical_or_application_identity() {
    use std::sync::{Arc, atomic::Ordering};

    let mut document = manifest_application_graph(1);
    document.nodes.push(NodeSpec {
        id: "microphone".into(),
        control: mixrelay_audio_graph::NodeControl::default(),
        label: None,
        config: NodeConfig::AudioDeviceInput(mixrelay_audio_graph::AudioDeviceInputConfig {
            selector: mixrelay_audio_io::DeviceSelector::DefaultInput,
            output_channels: 2,
            gain_db: 0.0,
        }),
    });
    let probe = Arc::new(ManifestNativeProbe::default());
    let mut engine = crate::endpoint_settings_tests::controller(document);
    engine.runtime_status = Arc::new(crate::FixedRuntimeStatus::new(crate::DriverState::Ready));
    engine.graph_resources = probe.clone();
    assert!(engine.execution_views.read().is_empty());
    let error = engine.current_capture_manifest().unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlUnavailable);
    assert_eq!(error.parameters["resource"], "application_capture_manifest");
    assert_eq!(
        error.parameters["reason"],
        "execution_snapshot_not_published"
    );
    assert_eq!(probe.inventories.load(Ordering::Relaxed), 0);
    assert_eq!(probe.application_resolutions.load(Ordering::Relaxed), 0);
}

fn published_manifest_controller() -> (
    EngineGraphController,
    std::sync::Arc<ManifestNativeProbe>,
    mixrelay_test_support::FixtureDir,
) {
    use std::sync::Arc;
    let directory = mixrelay_test_support::FixtureDir::new("published-capture-manifest").unwrap();
    let path = directory.join("graphs.json");
    let document = manifest_application_graph(1);
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "revision": 1,
            "activeId": "first",
            "graphs": [
                {"id":"first","name":"First","document":document},
                {"id":"second","name":"Second","document":document},
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    let probe = Arc::new(ManifestNativeProbe::default());
    let mut engine =
        crate::endpoint_settings_tests::controller(GraphDocument::new(1, vec![], vec![]));
    engine.runtime_status = Arc::new(crate::FixedRuntimeStatus::new(crate::DriverState::Ready));
    engine.graph_resources = probe.clone();
    (engine.with_graph_catalog_path(path), probe, directory)
}

fn acknowledge_manifest_controls(engine: &EngineGraphController, processor: &mut BlockProcessor) {
    let inputs = vec![vec![0.0; 256]; processor.plan().source_bindings().len()];
    let slices: Vec<_> = inputs.iter().map(Vec::as_slice).collect();
    processor.process_block(128, &slices, &mut []).unwrap();
    assert_eq!(
        engine.exchange.revisions().active,
        engine.exchange.revisions().submitted
    );
}

#[test]
fn published_manifest_preserves_shared_canonical_owner_controls_and_power_revision() {
    use std::sync::atomic::Ordering;

    let (engine, probe, _directory) = published_manifest_controller();
    let mut processor = crate::controller_test_support::settle(&engine);
    let resolved = probe.application_resolutions.load(Ordering::Relaxed);
    let manifest = engine.current_capture_manifest().unwrap();
    assert_eq!(manifest.sources.len(), 1);
    let bundle_id = "io.mixrelay.manifest-fixture";
    let shared_id = format!("application:{}:{bundle_id}", bundle_id.len());
    assert_eq!(
        manifest.sources[0].node_id,
        format!("s:{}:{shared_id}:native", shared_id.len())
    );
    assert_eq!(
        manifest.sources[0].request.gain_db.to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(manifest.revision, engine.exchange.revisions().submitted);
    assert_eq!(
        probe.application_resolutions.load(Ordering::Relaxed),
        resolved
    );

    for (graph_id, remaining) in [("first", 1), ("second", 0)] {
        let response = engine
            .change_node_control(
                graph_id,
                "app",
                &mixrelay_control_protocol::NodeControlRequest {
                    expected_revision: engine.exchange.revisions().submitted,
                    enabled: Some(false),
                    muted: None,
                    gain_db: None,
                },
            )
            .unwrap();
        acknowledge_manifest_controls(&engine, &mut processor);
        let current = engine.current_capture_manifest().unwrap();
        assert_eq!(current.revision, response.application.revision);
        assert_eq!(current.sources.len(), remaining);
        assert_eq!(
            probe.application_resolutions.load(Ordering::Relaxed),
            resolved
        );
    }

    let reenabled = engine
        .change_node_control(
            "first",
            "app",
            &mixrelay_control_protocol::NodeControlRequest {
                expected_revision: engine.exchange.revisions().submitted,
                enabled: Some(true),
                muted: None,
                gain_db: None,
            },
        )
        .unwrap();
    acknowledge_manifest_controls(&engine, &mut processor);
    let running = engine.current_capture_manifest().unwrap();
    assert_eq!(running.revision, reenabled.application.revision);
    assert_eq!(running.sources, manifest.sources);

    let off = engine.change_engine_power(false, None).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    let stopped = engine.current_capture_manifest().unwrap();
    assert_eq!(stopped.revision, off.revision);
    assert!(stopped.sources.is_empty());
}

#[test]
fn playback_manifest_uses_live_masks_and_rejects_conflicting_active_targets() {
    use mixrelay_audio_graph::NodeControl;
    use mixrelay_audio_io::SourcePlaybackPolicy;
    let mut manifest = capture_manifest(&manifest_application_graph(7)).unwrap();
    manifest.sources[0].request.gain_db = 0.0;
    let capture = NodeId::from(manifest.sources[0].node_id.as_str());
    let intents: Vec<_> = [("first", DEVICE_A), ("second", DEVICE_B)]
        .into_iter()
        .map(
            |(mask, endpoint)| crate::graph_composition::ApplicationPlaybackIntent {
                source_id: "shared-app".into(),
                capture_node_id: capture.clone(),
                mask_node_id: mask.into(),
                forwarding_endpoint: endpoint.into(),
            },
        )
        .collect();
    let original = manifest.clone();
    assert!(
        apply_playback_intents(&mut manifest, &intents, |_| Some(NodeControl::default())).is_err()
    );
    let mut manifest = original;
    apply_playback_intents(&mut manifest, &intents, |id| {
        Some(NodeControl {
            enabled: true,
            muted: id.as_str() == "second",
        })
    })
    .unwrap();
    assert_eq!(
        manifest.sources[0].request.source_playback,
        SourcePlaybackPolicy::MuteWhileCaptured
    );
    assert_eq!(
        manifest.sources[0]
            .request
            .forwarding_endpoint
            .as_ref()
            .unwrap()
            .as_str(),
        DEVICE_A
    );
    apply_playback_intents(&mut manifest, &intents, |_| {
        Some(NodeControl {
            enabled: false,
            muted: false,
        })
    })
    .unwrap();
    assert_eq!(
        manifest.sources[0].request.source_playback,
        SourcePlaybackPolicy::Preserve
    );
    assert_eq!(manifest.sources[0].request.forwarding_endpoint, None);
    assert_eq!(
        manifest.sources[0].request.gain_db.to_bits(),
        0.0_f32.to_bits()
    );
}
