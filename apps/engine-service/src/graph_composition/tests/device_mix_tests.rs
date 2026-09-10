use super::*;

fn asymmetric_registry() -> DeviceRegistry {
    let wide = StreamSpec {
        channels: 4,
        channel_layout: ChannelLayout::Discrete,
    };
    let mut x = device("X");
    x.capture = Some(wide);
    let mut y = device("Y");
    y.render = None;
    y.capture = Some(wide);
    DeviceRegistry::new(vec![x, y])
        .unwrap()
        .with_sources(vec![SharedSource {
            id: "input".into(),
            config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                selector: DeviceSelector::DefaultInput,
                output_channels: 4,
                gain_db: 0.0,
            }),
        }])
        .unwrap()
}

fn asymmetric_composition() -> ComposedGraph {
    let registry = asymmetric_registry();
    let source = [SourceReference {
        control: mixrelay_audio_graph::NodeControl::default(),
        node_id: "input".into(),
        source_id: "input".into(),
        channels: 4,
        gain_db: 0.0,
    }];
    let writer = GraphDocument::new(
        1,
        vec![],
        vec![edge("send", "input", AUDIO_PORT_ID, "x", TO_OS_PORT_ID)],
    );
    let reader = GraphDocument::new(
        1,
        vec![],
        vec![edge("read", "x", FROM_OS_PORT_ID, "y", TO_OS_PORT_ID)],
    );
    let write_refs = [DeviceReference {
        control: mixrelay_audio_graph::NodeControl::default(),
        node_id: "x".into(),
        device_id: "X".into(),
        read: None,
        write_channels: Some(4),
        gain_db: 0.0,
    }];
    let read_refs = [
        DeviceReference {
            control: mixrelay_audio_graph::NodeControl::default(),
            node_id: "x".into(),
            device_id: "X".into(),
            read: Some(DeviceReadReference { channels: 4 }),
            write_channels: None,
            gain_db: -6.020_600_3,
        },
        DeviceReference {
            control: mixrelay_audio_graph::NodeControl::default(),
            node_id: "y".into(),
            device_id: "Y".into(),
            read: None,
            write_channels: Some(4),
            gain_db: 0.0,
        },
    ];
    compose(
        &registry,
        &[
            GraphFragment {
                id: "A",
                document: &writer,
                device_references: &write_refs,
                source_references: &source,
                output_references: &[],
            },
            GraphFragment {
                id: "B",
                document: &reader,
                device_references: &read_refs,
                source_references: &[],
                output_references: &[],
            },
        ],
        1,
        context(),
        &GraphCompiler::default(),
    )
    .unwrap()
}

#[test]
fn asymmetric_native_geometry_sums_all_channels_before_per_reference_gain() {
    let composed = asymmetric_composition();
    let plan = Arc::new(RenderPlan::prepare(&composed.execution).unwrap());
    let inputs: Vec<Vec<f32>> = plan
        .source_bindings()
        .iter()
        .map(|binding| {
            if binding.virtual_endpoint_id().is_some() {
                assert_eq!(binding.channels(), 2);
                [0.8, 0.6].repeat(4)
            } else {
                [0.2, 0.4, 0.6, 0.8].repeat(4)
            }
        })
        .collect();
    let mut outputs: Vec<Vec<f32>> = plan
        .sink_bindings()
        .iter()
        .map(|binding| {
            assert_eq!(binding.channels(), 4);
            vec![0.0; 16]
        })
        .collect();
    let mut processor = BlockProcessor::new(Arc::clone(&plan));
    for cycle in 0..2 {
        processor
            .process_block(
                4,
                &inputs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                &mut outputs
                    .iter_mut()
                    .map(Vec::as_mut_slice)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        for (binding, samples) in plan.sink_bindings().iter().zip(&outputs) {
            let expected = if binding.virtual_endpoint_id().unwrap().as_str() == "X" {
                [1.0, 1.0, 0.6, 0.8]
            } else if cycle == 0 {
                [0.0; 4]
            } else {
                [0.5, 0.5, 0.3, 0.4]
            };
            for frame in samples.chunks_exact(4) {
                for (sample, value) in frame.iter().zip(expected) {
                    assert!((sample - value).abs() < 0.000_001);
                }
            }
        }
    }
    assert_eq!(composed.native_devices.len(), 2);
    assert_eq!(composed.device_buses.len(), 2);
}
