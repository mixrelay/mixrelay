use super::*;
use crate::ControlBackend;
use tonenari_audio_graph::{
    AudioEndpoint, ChannelLayout, ChannelMapping, EdgeSpec, NodeSpec, PortId, StreamSpec,
    VirtualEndpointConfig, VirtualEndpointId, VirtualEndpointReference,
};
use tonenari_control_protocol::{GraphCatalogAction, GraphCatalogRequest};

const DEVICE_A: &str = "31ab4e5a-44e0-4bab-89dd-906e07751ea1";
const DEVICE_B: &str = "31ab4e5a-44e0-4bab-89dd-906e07751ea2";

fn device(id: &str, node: &str, reference: VirtualEndpointReference) -> NodeSpec {
    let stream = StreamSpec {
        channels: 2,
        channel_layout: ChannelLayout::Stereo,
    };
    NodeSpec {
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
    let signal = [0.25, -0.5, 0.75, -0.125];
    let inputs: Vec<Vec<f32>> = processor
        .plan()
        .source_bindings()
        .iter()
        .map(|binding| {
            if binding
                .virtual_endpoint_id()
                .is_some_and(|id| id.as_str() == DEVICE_A)
            {
                signal.to_vec()
            } else {
                vec![0.0; 4]
            }
        })
        .collect();
    let mut outputs = vec![vec![0.0; 4]; processor.plan().sink_bindings().len()];
    for _ in 0..2 {
        processor
            .process_block(
                2,
                &inputs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                &mut outputs
                    .iter_mut()
                    .map(Vec::as_mut_slice)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        assert_eq!(output, signal);
    }
}

#[test]
fn controller_runs_both_graphs_and_view_switches_do_not_submit_or_interrupt_shared_pcm() {
    let directory = tonenari_test_support::FixtureDir::new("composed-controller").unwrap();
    let engine = crate::endpoint_settings_tests::controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(directory.join("graphs.json"));
    let mut processor = engine
        .prepare_and_validate(&engine.graph(), *engine.context.read())
        .unwrap()
        .processor;
    engine.apply_graph(graph_a(2)).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    let revision = engine.exchange.revisions();
    select(&engine, GraphCatalogAction::Create { name: "B".into() });
    let graph_b_id = engine.view_graph_id();
    assert_eq!(engine.exchange.revisions(), revision);
    engine.apply_graph(graph_b(3)).unwrap();
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
fn self_device_routing_is_accepted_without_feedback_policy() {
    let directory = tonenari_test_support::FixtureDir::new("composed-feedback").unwrap();
    let engine = crate::endpoint_settings_tests::controller(GraphDocument::new(1, vec![], vec![]))
        .with_graph_catalog_path(directory.join("graphs.json"));
    let mut processor = engine
        .prepare_and_validate(&engine.graph(), *engine.context.read())
        .unwrap()
        .processor;
    engine.apply_graph(graph_a(2)).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    select(&engine, GraphCatalogAction::Create { name: "B".into() });
    let mut feedback = graph_b(3);
    let NodeConfig::VirtualEndpoint(config) = &mut feedback.nodes[1].config else {
        panic!("device");
    };
    config.spec.id = VirtualEndpointId::from(DEVICE_A);
    config.spec.display_name = DEVICE_A.to_owned();
    engine.apply_graph(feedback).unwrap();
    assert!(engine.exchange.activate_pending(&mut processor));
    assert_eq!(engine.endpoint_settings().unwrap().devices.len(), 1);
}
