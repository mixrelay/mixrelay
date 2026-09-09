use super::super::*;
use super::observed_endpoint_rate;
use tonenari_control_server::ControlBackend;

#[test]
#[ignore = "requires owned isolated PipeWire daemon; never run against a user session"]
fn native_pipewire_live_session_format_transaction() {
    let mut document = default_duplex_document(1, None);
    let NodeConfig::VirtualEndpoint(endpoint) = &document.nodes[0].config else {
        panic!()
    };
    let binding = EndpointBinding {
        node_id: document.nodes[0].id.as_str().into(),
        spec: endpoint.spec.clone(),
    };
    let endpoint = PipeWireEndpoint::open_format(&binding, render_format()).unwrap();
    let backend = LinuxVirtualEndpointBackend::new(binding);
    let status = Arc::new(AdapterRuntimeStatus::new(adapter_profile()).with_graph_format_control());
    let meters = default_peak_meter_bank();
    let processor = create_initial_processor(meters.clone(), render_format()).unwrap();
    let exchange = Arc::new(ProcessorExchange::new(1));
    let controller = EngineGraphController::new_with_configuration(
        exchange.clone(),
        EngineGraphControllerConfig {
            format: render_format(),
            meters,
            initial_bindings: processor.plan().meter_bindings().to_vec(),
            endpoint_backend: backend.clone(),
            required_endpoint_node_id: None,
            processor_catalog: ProcessorCatalog::default(),
            graph_resources: backend.clone(),
            runtime_status: status.clone(),
        },
    );
    let worker_status = status.clone();
    let worker_exchange = exchange.clone();
    let worker_backend = backend.clone();
    let worker = thread::spawn(move || {
        run_audio_worker(
            processor,
            worker_exchange,
            worker_backend,
            endpoint,
            &worker_status,
        );
    });
    let guard = tonenari_engine_service::session::AudioWorker::new(status.clone(), worker);
    thread::sleep(Duration::from_millis(100));
    let fixture = attach_fixture(&mut document);
    controller.apply_graph(document.clone()).unwrap();
    wait_for_audio(&controller, &exchange, 2);
    exchange.reclaim_retired();
    backend.reclaim_retired();
    for (sample_rate, block_frames) in [(44_100, 256), (48_000, 1024), (96_000, 128), (44_100, 64)]
    {
        let revision = exchange.revisions().active;
        let response = controller
            .reconfigure_session(tonenari_control_protocol::SessionReconfigureRequest {
                expected_revision: revision,
                format: tonenari_audio_io::session::SessionFormat {
                    sample_rate,
                    block_frames,
                },
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while exchange.revisions().active != response.revision
            || observed_endpoint_rate() != Some(sample_rate)
        {
            assert!(
                Instant::now() < deadline,
                "format activation timed out: {:?}",
                controller.status()
            );
            thread::sleep(Duration::from_millis(5));
        }
        let observed = controller.status();
        assert_eq!(observed.sample_rate, Some(sample_rate));
        assert_eq!(observed.buffer_frames, Some(block_frames));
        wait_for_audio(&controller, &exchange, response.revision);
        assert!(!status.stop_requested());
        assert_eq!(controller.graph().edges, document.edges);
        exchange.reclaim_retired();
        backend.reclaim_retired();
        let quantums = observed_quantums();
        println!(
            "activated_revision={}; native_endpoint_rate={sample_rate}; dsp_rate={sample_rate}; prepared_block={block_frames}; callback_quantums_graph_pcm={quantums:?}; native_worker_alive=true",
            response.revision
        );
    }
    let revision = exchange.revisions().active;
    let before = controller.graph();
    assert!(
        controller
            .reconfigure_session(tonenari_control_protocol::SessionReconfigureRequest {
                expected_revision: revision,
                format: tonenari_audio_io::session::SessionFormat {
                    sample_rate: 12_345,
                    block_frames: 128
                },
            })
            .is_err()
    );
    assert_eq!(controller.graph(), before);
    assert_eq!(exchange.revisions().active, revision);
    drop(guard);
    drop(fixture);
}

fn observed_quantums() -> std::collections::BTreeSet<(u64, u64)> {
    let mut result = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let value = super::OBSERVED_ENDPOINT_QUANTUM.load(Ordering::Acquire);
        if value != 0 {
            result.insert((value >> 32, value & u64::from(u32::MAX)));
        }
        thread::sleep(Duration::from_millis(1));
    }
    result
}

fn attach_fixture(
    document: &mut tonenari_audio_graph::GraphDocument,
) -> applications::tests::FixtureProcess {
    let fixture = applications::tests::FixtureProcess::start(&std::path::PathBuf::from(
        std::env::var("TONENARI_LAB_APPLICATION_FIXTURE").unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !tonenari_audio_io::linux::targets()
        .unwrap()
        .iter()
        .any(|target| target.application_id.as_deref() == Some("org.tonenari.fixture.application"))
    {
        assert!(
            Instant::now() < deadline,
            "owned application fixture did not register"
        );
        thread::sleep(Duration::from_millis(5));
    }
    document.revision = 2;
    document.nodes.push(tonenari_audio_graph::NodeSpec {
        id: "rate-fixture".into(), label: None,
        config: NodeConfig::ApplicationCapture(serde_json::from_value(serde_json::json!({
            "selector": tonenari_audio_io::ApplicationSelector::BundleId { bundle_id: "org.tonenari.fixture.application".into(), include_descendants: false },
            "stream": { "channels": 2, "channelLayout": "stereo" }, "sourcePlayback": "preserve"
        })).unwrap()),
    });
    for edge in &mut document.edges {
        edge.source.node_id = "rate-fixture".into();
        edge.source.port_id = tonenari_audio_graph::AUDIO_PORT_ID.into();
    }
    fixture
}

fn wait_for_audio(controller: &EngineGraphController, exchange: &ProcessorExchange, revision: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let meters = controller.meters();
        if exchange.revisions().active == revision
            && meters.revision == revision
            && [
                "rate-fixture",
                tonenari_engine_service::DEFAULT_ENDPOINT_NODE_ID,
            ]
            .iter()
            .all(|id| {
                meters
                    .nodes
                    .iter()
                    .any(|node| node.node_id == *id && node.peaks.iter().any(|peak| *peak > 0.05))
            })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "live-format PCM not observed: {meters:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
