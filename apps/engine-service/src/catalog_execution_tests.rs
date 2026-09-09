use super::*;
use std::{cell::Cell, sync::Arc};
use tonenari_audio_engine::{BlockProcessor, RenderPlan};
use tonenari_audio_graph::{
    AUDIO_PORT_ID, AudioEndpoint, ChannelMapping, CompileLimits, EdgeId, EdgeSpec, NodeId,
    RenderFormat, VirtualEndpointConfig, VirtualEndpointReference,
};
use tonenari_audio_io::{ApplicationSelector, ChannelLayout, SourcePlaybackPolicy, StreamSpec};

struct Apps(Cell<usize>);
impl ApplicationIdentityResolver for Apps {
    fn resolve(
        &self,
        request: &ApplicationCaptureRequest,
    ) -> Result<SharedSource, CompositionError> {
        self.0.set(self.0.get() + 1);
        if request.requires_reselection
            || matches!(&request.selector,
            ApplicationSelector::ExecutablePath { path } if path == "/unavailable")
        {
            return Err(invalid("application requires native reselection"));
        }
        let mut canonical = request.clone();
        canonical.selector = ApplicationSelector::BundleId {
            bundle_id: "verified.app".into(),
            include_descendants: true,
        };
        canonical.gain_db = 0.0;
        Ok(SharedSource {
            id: "verified-native-app".into(),
            config: NodeConfig::ApplicationCapture(canonical),
        })
    }
}
fn inventory() -> Vec<AudioDeviceDescriptor> {
    vec![AudioDeviceDescriptor {
        uid: "physical-one".into(),
        display_name: "Hardware".into(),
        input_channels: 2,
        output_channels: 2,
        is_default_input: true,
        is_default_output: true,
    }]
}
fn compile(
    documents: &[(String, GraphDocument)],
    devices: &[AudioDeviceDescriptor],
    apps: &Apps,
) -> Result<ComposedGraph, CompositionError> {
    compose_catalog(
        documents,
        CatalogExecutionContext {
            revision: 9,
            compile: CompileContext {
                format: RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 4,
                },
                limits: CompileLimits::default(),
            },
            compiler: &GraphCompiler::default(),
            devices,
            applications: apps,
        },
    )
}
fn node(id: &str, config: NodeConfig) -> NodeSpec {
    NodeSpec {
        id: id.into(),
        label: None,
        config,
    }
}
fn route(id: &str, source: &str, source_port: &str, sink: &str, sink_port: &str) -> EdgeSpec {
    EdgeSpec {
        id: EdgeId::from(id),
        source: AudioEndpoint {
            node_id: source.into(),
            port_id: source_port.into(),
        },
        destination: AudioEndpoint {
            node_id: sink.into(),
            port_id: sink_port.into(),
        },
        channel_mapping: ChannelMapping::default(),
    }
}
fn mic(selector: DeviceSelector, gain_db: f32) -> NodeSpec {
    node(
        "input",
        NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
            selector,
            output_channels: 2,
            gain_db,
        }),
    )
}
fn output(selector: DeviceSelector) -> NodeSpec {
    node(
        "output",
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector,
            input_channels: 2,
        }),
    )
}
fn physical_graph(selector: DeviceSelector, gain: f32) -> GraphDocument {
    GraphDocument::new(
        1,
        vec![mic(selector, gain), output(DeviceSelector::DefaultOutput)],
        vec![route(
            "link",
            "input",
            AUDIO_PORT_ID,
            "output",
            AUDIO_PORT_ID,
        )],
    )
}
fn device(id: &str, read: bool, capture: bool) -> VirtualEndpointConfig {
    VirtualEndpointConfig {
        spec: VirtualEndpointSpec {
            id: id.into(),
            display_name: format!("Device {id}"),
            render: Some(StreamSpec {
                channels: 2,
                channel_layout: ChannelLayout::Stereo,
            }),
            capture: Some(StreamSpec {
                channels: 2,
                channel_layout: ChannelLayout::Stereo,
            }),
            supported_sample_rates: vec![44_100, 48_000],
            default_sample_rate: 48_000,
        },
        reference: VirtualEndpointReference {
            render: read,
            capture,
        },
        gain_db: 0.0,
    }
}
fn pcm(composed: &ComposedGraph) -> BTreeMap<NodeId, Vec<f32>> {
    let plan = Arc::new(RenderPlan::prepare(&composed.execution).unwrap());
    let sources = plan
        .source_bindings()
        .iter()
        .map(|binding| {
            let sample = if composed
                .native_sources
                .values()
                .any(|id| id == binding.node_id())
            {
                0.25
            } else if composed.native_devices.get(&VirtualEndpointId::from("X"))
                == Some(binding.node_id())
            {
                0.8
            } else {
                0.0
            };
            vec![sample; usize::from(binding.channels()) * 4]
        })
        .collect::<Vec<_>>();
    let mut sinks = plan
        .sink_bindings()
        .iter()
        .map(|binding| vec![0.0; usize::from(binding.channels()) * 4])
        .collect::<Vec<_>>();
    let mut processor = BlockProcessor::new(plan.clone());
    for _ in 0..2 {
        processor
            .process_block(
                4,
                &sources.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                &mut sinks.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>(),
            )
            .unwrap();
    }
    plan.sink_bindings()
        .iter()
        .map(|binding| binding.node_id().clone())
        .zip(sinks)
        .collect()
}
fn samples_equal(samples: &[f32], value: f32) {
    assert!(
        samples
            .iter()
            .all(|sample| (*sample - value).abs() < 0.00001)
    );
}

#[test]
fn actual_inventory_resolves_default_and_uid_to_one_capture_and_mixed_output() {
    let apps = Apps(Cell::new(0));
    let documents = vec![
        (
            "A".into(),
            physical_graph(DeviceSelector::DefaultInput, 0.0),
        ),
        (
            "B".into(),
            physical_graph(
                DeviceSelector::PersistentUid {
                    uid: "physical-one".into(),
                },
                -6.0206,
            ),
        ),
    ];
    let before = documents.clone();
    let composition = compile(&documents, &inventory(), &apps).unwrap();
    assert_eq!(documents, before);
    assert_eq!(apps.0.get(), 0);
    assert_eq!(composition.native_sources.len(), 1);
    assert_eq!(composition.native_outputs.len(), 1);
    let output = composition.native_outputs.values().next().unwrap();
    samples_equal(&pcm(&composition)[output], 0.375);
    let remaining = compile(&documents[1..], &inventory(), &apps).unwrap();
    assert_eq!(remaining.native_sources, composition.native_sources);
    assert_eq!(remaining.native_outputs, composition.native_outputs);
    samples_equal(&pcm(&remaining)[output], 0.125);
}

#[test]
fn absent_physical_selection_is_preserved_but_ambiguity_and_wrong_width_are_rejected() {
    let apps = Apps(Cell::new(0));
    let documents = vec![(
        "A".into(),
        physical_graph(DeviceSelector::DefaultInput, 0.0),
    )];
    let offline = compile(&documents, &[], &apps).unwrap();
    assert_eq!(offline.native_sources.len(), 1);
    assert_eq!(offline.native_outputs.len(), 1);
    let mut devices = inventory();
    devices.push(devices[0].clone());
    assert!(compile(&documents, &devices, &apps).is_err());
    devices.pop();
    devices[0].input_channels = 1;
    assert!(compile(&documents, &devices, &apps).is_err());
    assert_eq!(apps.0.get(), 0);
}

#[path = "catalog_offline_tests.rs"]
mod offline_tests;

#[test]
fn conflicting_virtual_definitions_are_rejected_but_local_reference_flags_are_independent() {
    let apps = Apps(Cell::new(0));
    let a = node(
        "virtual",
        NodeConfig::VirtualEndpoint(device("X", false, true)),
    );
    let b = node(
        "virtual",
        NodeConfig::VirtualEndpoint(device("X", true, false)),
    );
    let mut documents = vec![
        ("A".into(), GraphDocument::new(1, vec![a], vec![])),
        ("B".into(), GraphDocument::new(1, vec![b], vec![])),
    ];
    let composed = compile(&documents, &[], &apps).unwrap();
    assert_eq!(composed.native_devices.len(), 1);
    let NodeConfig::VirtualEndpoint(config) = &mut documents[1].1.nodes[0].config else {
        panic!()
    };
    config.spec.display_name = "Conflicting definition".into();
    assert!(compile(&documents, &[], &apps).is_err());
}

#[test]
fn real_catalog_pcm_automatically_sums_playback_and_produced_bus() {
    let apps = Apps(Cell::new(0));
    let a = GraphDocument::new(
        1,
        vec![
            mic(DeviceSelector::DefaultInput, 0.0),
            node("out", NodeConfig::VirtualEndpoint(device("X", false, true))),
        ],
        vec![route("wire", "input", AUDIO_PORT_ID, "out", "to_os")],
    );
    let reader = |id: &str| {
        GraphDocument::new(
            1,
            vec![
                node("in", NodeConfig::VirtualEndpoint(device("X", true, false))),
                node("out", NodeConfig::VirtualEndpoint(device(id, false, true))),
            ],
            vec![route("wire", "in", "from_os", "out", "to_os")],
        )
    };
    let documents = vec![
        ("A".into(), a),
        ("B".into(), reader("Y")),
        ("C".into(), reader("Z")),
    ];
    let composed = compile(&documents, &inventory(), &apps).unwrap();
    let output = pcm(&composed);
    samples_equal(
        &output[&composed.native_devices[&VirtualEndpointId::from("X")]],
        1.05,
    );
    samples_equal(
        &output[&composed.native_devices[&VirtualEndpointId::from("Y")]],
        1.05,
    );
    samples_equal(
        &output[&composed.native_devices[&VirtualEndpointId::from("Z")]],
        1.05,
    );
    assert_eq!(composed.native_sources.len(), 1);
    let self_route = compile(&[("loop".into(), reader("X"))], &[], &apps).unwrap();
    samples_equal(
        &pcm(&self_route)[&self_route.native_devices[&VirtualEndpointId::from("X")]],
        1.6,
    );
}

#[test]
fn application_owner_comes_only_from_resolver_and_failures_remain_visible() {
    let apps = Apps(Cell::new(0));
    let request = ApplicationCaptureRequest {
        selector: ApplicationSelector::ExecutablePath {
            path: "/verified/app".into(),
        },
        stream: StreamSpec {
            channels: 2,
            channel_layout: ChannelLayout::Stereo,
        },
        source_playback: SourcePlaybackPolicy::Preserve,
        gain_db: 0.0,
        requires_reselection: false,
    };
    let graph = GraphDocument::new(
        1,
        vec![
            node("app", NodeConfig::ApplicationCapture(request)),
            output(DeviceSelector::DefaultOutput),
        ],
        vec![route("wire", "app", AUDIO_PORT_ID, "output", AUDIO_PORT_ID)],
    );
    let mut documents = vec![("A".into(), graph.clone()), ("B".into(), graph)];
    let composed = compile(&documents, &inventory(), &apps).unwrap();
    assert_eq!(apps.0.get(), 2);
    assert_eq!(composed.native_sources.len(), 1);
    let output = composed.native_outputs.values().next().unwrap();
    samples_equal(&pcm(&composed)[output], 0.5);
    let NodeConfig::ApplicationCapture(request) = &mut documents[1].1.nodes[0].config else {
        panic!()
    };
    request.requires_reselection = true;
    let inactive = compile(&documents, &inventory(), &apps).unwrap();
    assert_eq!(apps.0.get(), 3, "inactive source must not be resolved");
    assert_eq!(inactive.native_sources.len(), 1);
    let output = inactive.native_outputs.values().next().unwrap();
    samples_equal(&pcm(&inactive)[output], 0.25);
    let NodeConfig::ApplicationCapture(request) = &mut documents[1].1.nodes[0].config else {
        panic!()
    };
    request.requires_reselection = false;
    request.selector = ApplicationSelector::ExecutablePath {
        path: "/unavailable".into(),
    };
    assert!(compile(&documents, &inventory(), &apps).is_err());
}

#[test]
fn empty_catalog_does_not_resolve_apps_or_create_native_resources() {
    let apps = Apps(Cell::new(0));
    let composed = compile(&[], &[], &apps).unwrap();
    assert!(composed.document.nodes.is_empty());
    assert!(composed.native_devices.is_empty());
    assert!(composed.native_sources.is_empty());
    assert!(composed.native_outputs.is_empty());
    assert_eq!(composed.document.revision, 9);
    assert_eq!(apps.0.get(), 0);
}
