use super::*;

fn authored(selector: DeviceSelector, gain_db: f32) -> GraphDocument {
    GraphDocument::new(
        17,
        vec![
            NodeSpec {
                id: NodeId::from("mic"),
                label: Some("Microphone view".into()),
                config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector,
                    output_channels: 2,
                    gain_db,
                }),
            },
            gain("processor", 0.0),
            NodeSpec {
                id: NodeId::from("output"),
                label: Some("My output".into()),
                config: NodeConfig::VirtualEndpoint(device("X").into()),
            },
        ],
        vec![
            edge("route1", "mic", AUDIO_PORT_ID, "processor", AUDIO_PORT_ID),
            edge(
                "route2",
                "processor",
                AUDIO_PORT_ID,
                "output",
                TO_OS_PORT_ID,
            ),
        ],
    )
}

#[test]
fn explicit_default_alias_resolution_preserves_documents_and_shares_one_pcm_owner() {
    let registry = mic_registry(vec![device("X")]);
    let a = authored(DeviceSelector::DefaultInput, 0.0);
    let b = authored(
        DeviceSelector::PersistentUid {
            uid: "macbook-mic".into(),
        },
        -6.020_6,
    );
    let original = a.clone();
    let resolved = ResolvedNodeIdentities {
        sources: BTreeMap::from([(NodeId::from("mic"), "macbook-mic".into())]),
        outputs: BTreeMap::new(),
    };
    assert!(lower_document(&a, &registry, &ResolvedNodeIdentities::default()).is_err());
    let lowered_a = lower_document(&a, &registry, &resolved).unwrap();
    let lowered_b = lower_document(&b, &registry, &resolved).unwrap();
    assert_eq!(a, original);
    assert_eq!(lowered_a.document.edges, a.edges);
    assert_eq!(lowered_a.document.revision, a.revision);
    assert_eq!(lowered_a.document.nodes, vec![a.nodes[1].clone()]);
    assert_eq!(
        lowered_a.reference_labels[&NodeId::from("mic")],
        a.nodes[0].label
    );
    assert_eq!(
        lowered_b.source_references[0].gain_db.to_bits(),
        (-6.020_6_f32).to_bits()
    );
    let composed = compose(
        &registry,
        &[lowered_a.as_fragment("A"), lowered_b.as_fragment("B")],
        22,
        context(),
        &GraphCompiler::default(),
    )
    .unwrap();
    assert_eq!(composed.native_sources.len(), 1);
    assert_frame(
        &pcm(&composed)[&composed.native_devices[&VirtualEndpointId::from("X")]],
        [1.1, 1.2],
    );
}

#[test]
fn persisted_document_and_device_rename_keep_native_identity_and_authored_routes() {
    let old = authored(DeviceSelector::DefaultInput, 0.0);
    let saved = serde_json::to_vec(&old).unwrap();
    let restored: GraphDocument = serde_json::from_slice(&saved).unwrap();
    let resolved = ResolvedNodeIdentities {
        sources: BTreeMap::from([(NodeId::from("mic"), "macbook-mic".into())]),
        outputs: BTreeMap::new(),
    };
    let before = mic_registry(vec![device("X")]);
    let mut renamed = device("X");
    renamed.display_name = "Broadcast send".into();
    let after = mic_registry(vec![renamed]);
    let initial = lower_document(&old, &before, &resolved).unwrap();
    let later = lower_document(&restored, &after, &resolved).unwrap();
    assert_eq!(initial, later);
    let compile = |registry: &DeviceRegistry, graph: &AuthoredGraph| {
        compose(
            registry,
            &[graph.as_fragment("A")],
            1,
            context(),
            &GraphCompiler::default(),
        )
        .unwrap()
    };
    let a = compile(&before, &initial);
    let b = compile(&after, &later);
    assert_eq!(a.native_devices, b.native_devices);
    assert_eq!(a.native_sources, b.native_sources);
    assert_eq!(pcm(&a), pcm(&b));
}

#[test]
fn channel_projection_preserves_first_channel_without_inventing_native_width() {
    let registry = mic_registry(vec![device("X")]);
    let mut doc = authored(DeviceSelector::DefaultInput, 0.0);
    let NodeConfig::AudioDeviceInput(config) = &mut doc.nodes[0].config else {
        panic!()
    };
    config.output_channels = 1;
    doc.edges[0].channel_mapping = mixrelay_audio_graph::ChannelMapping::Explicit {
        routes: vec![mixrelay_audio_graph::ChannelRoute {
            source_channel: 0,
            destination_channel: 1,
        }],
    };
    let resolved = ResolvedNodeIdentities {
        sources: BTreeMap::from([(NodeId::from("mic"), "macbook-mic".into())]),
        outputs: BTreeMap::new(),
    };
    let lowered = lower_document(&doc, &registry, &resolved).unwrap();
    let composed = compose(
        &registry,
        &[lowered.as_fragment("A")],
        1,
        context(),
        &GraphCompiler::default(),
    )
    .unwrap();
    assert_frame(
        &pcm(&composed)[&composed.native_devices[&VirtualEndpointId::from("X")]],
        [0.8, 0.8],
    );
    let NodeConfig::AudioDeviceInput(config) = &mut doc.nodes[0].config else {
        panic!()
    };
    config.output_channels = 3;
    assert!(lower_document(&doc, &registry, &resolved).is_err());
}

#[test]
fn virtual_local_width_is_projected_against_canonical_device_not_metadata_equality() {
    let registry = DeviceRegistry::new(vec![device("X"), device("Y")]).unwrap();
    let mono = StreamSpec {
        channels: 1,
        channel_layout: ChannelLayout::Mono,
    };
    let mut input = device("X");
    input.render = Some(mono);
    input.capture = None;
    let mut output = device("Y");
    output.render = None;
    output.capture = Some(mono);
    let document = GraphDocument::new(
        1,
        vec![
            NodeSpec {
                id: NodeId::from("input"),
                label: None,
                config: NodeConfig::VirtualEndpoint(input.into()),
            },
            NodeSpec {
                id: NodeId::from("output"),
                label: None,
                config: NodeConfig::VirtualEndpoint(output.into()),
            },
        ],
        vec![edge(
            "route",
            "input",
            FROM_OS_PORT_ID,
            "output",
            TO_OS_PORT_ID,
        )],
    );
    let lowered = lower_document(&document, &registry, &ResolvedNodeIdentities::default()).unwrap();
    let composed = compose(
        &registry,
        &[lowered.as_fragment("A")],
        2,
        context(),
        &GraphCompiler::default(),
    )
    .unwrap();
    assert_frame(
        &pcm(&composed)[&composed.native_devices[&VirtualEndpointId::from("Y")]],
        [0.8, 0.0],
    );
}

#[test]
fn one_source_reference_reads_completed_mix_and_invalid_port_is_rejected() {
    let registry = mic_registry(vec![device("X"), device("Y")]);
    let producer = lower_document(
        &authored(DeviceSelector::DefaultInput, 0.0),
        &registry,
        &ResolvedNodeIdentities {
            sources: BTreeMap::from([(NodeId::from("mic"), "macbook-mic".into())]),
            outputs: BTreeMap::new(),
        },
    )
    .unwrap();
    let mut document = GraphDocument::new(
        1,
        vec![
            NodeSpec {
                id: NodeId::from("input"),
                label: None,
                config: NodeConfig::VirtualEndpoint(device("X").into()),
            },
            NodeSpec {
                id: NodeId::from("output"),
                label: None,
                config: NodeConfig::VirtualEndpoint(device("Y").into()),
            },
        ],
        vec![edge(
            "route",
            "input",
            FROM_OS_PORT_ID,
            "output",
            TO_OS_PORT_ID,
        )],
    );
    let NodeConfig::VirtualEndpoint(reference) = &mut document.nodes[0].config else {
        panic!()
    };
    reference.reference.capture = false;
    let consumer =
        lower_document(&document, &registry, &ResolvedNodeIdentities::default()).unwrap();
    let composed = compose(
        &registry,
        &[producer.as_fragment("A"), consumer.as_fragment("B")],
        2,
        context(),
        &GraphCompiler::default(),
    )
    .unwrap();
    assert_frame(
        &pcm(&composed)[&composed.native_devices[&VirtualEndpointId::from("Y")]],
        [1.0, 1.0],
    );
    document.edges.push(edge(
        "invalid-port",
        "input",
        "unknown",
        "output",
        TO_OS_PORT_ID,
    ));
    let conflicting =
        lower_document(&document, &registry, &ResolvedNodeIdentities::default()).unwrap();
    assert!(
        compose(
            &registry,
            &[conflicting.as_fragment("B")],
            3,
            context(),
            &GraphCompiler::default()
        )
        .is_err()
    );
}
