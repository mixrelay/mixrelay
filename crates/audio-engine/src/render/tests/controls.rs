use super::*;
use mixrelay_audio_graph::NodeControl;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Counts {
    prepares: AtomicUsize,
    calls: AtomicUsize,
}

struct CountedProcessor(Arc<Counts>);

impl HostedAudioProcessor for CountedProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool {
        self.0.calls.fetch_add(1, Ordering::Relaxed);
        DoubleProcessor.process(input, output, frames)
    }
}

struct CountedFactory(Arc<Counts>);

impl HostedProcessorFactory for CountedFactory {
    fn prepare(
        &self,
        _: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String> {
        self.0.prepares.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(CountedProcessor(Arc::clone(&self.0))))
    }
}

fn provider_document(control: NodeControl) -> GraphDocument {
    let mut provider = node(
        "effect",
        NodeConfig::Provider(ProviderNodeConfig {
            type_id: mixrelay_audio_graph::NodeTypeId::from("test:double"),
            parameters: serde_json::json!({}),
            ports: None,
        }),
    );
    provider.control = control;
    GraphDocument::new(
        1,
        vec![device_input("in"), provider, device_output("out")],
        vec![
            edge("input", "in", AUDIO_PORT_ID, "effect", AUDIO_PORT_ID),
            edge("output", "effect", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ],
    )
}

fn prepare_provider(document: &GraphDocument, counts: &Arc<Counts>) -> BlockProcessor {
    let execution = GraphCompiler::new(TestProviderResolver)
        .compile(
            document,
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    BlockProcessor::prepare_with_hosted_processors(
        &execution,
        Arc::new(crate::PeakMeterBank::new(8, 8)),
        &CountedFactory(Arc::clone(counts)),
    )
    .unwrap()
}

#[test]
fn processor_off_avoids_factory_and_dsp_while_mute_keeps_processing_without_bypass() {
    for (control, expected_prepares, expected_calls, expected_sample) in [
        (NodeControl::default(), 1, 100, 2.0_f32),
        (
            NodeControl {
                enabled: false,
                muted: false,
            },
            0,
            0,
            0.0,
        ),
        (
            NodeControl {
                enabled: true,
                muted: true,
            },
            1,
            100,
            0.0,
        ),
    ] {
        let counts = Arc::new(Counts::default());
        let mut processor = prepare_provider(&provider_document(control), &counts);
        let mut output = [9.0; 8];
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        for _ in 0..100 {
            processor
                .process_block(4, &[&[1.0; 8]], &mut [&mut output])
                .unwrap();
        }
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        assert_eq!(counts.prepares.load(Ordering::Relaxed), expected_prepares);
        assert_eq!(counts.calls.load(Ordering::Relaxed), expected_calls);
        assert_eq!(
            output.map(f32::to_bits),
            [expected_sample; 8].map(f32::to_bits)
        );
    }
}

#[test]
fn restoring_enabled_node_retains_authored_connections_and_parameters() {
    let counts = Arc::new(Counts::default());
    let mut document = provider_document(NodeControl::default());
    let saved = document.clone();
    for enabled in [true, false, true] {
        document.nodes[1].control.enabled = enabled;
        let mut processor = prepare_provider(&document, &counts);
        let mut output = [9.0; 8];
        processor
            .process_block(4, &[&[1.0; 8]], &mut [&mut output])
            .unwrap();
        let expected = if enabled { 2.0_f32 } else { 0.0 };
        assert_eq!(output.map(f32::to_bits), [expected; 8].map(f32::to_bits));
    }
    assert_eq!(document, saved);
}

#[test]
fn disabled_source_has_no_binding_and_does_not_affect_another_active_route() {
    let mut disabled = device_input("disabled");
    disabled.control.enabled = false;
    let execution = compile(
        vec![disabled, device_input("active"), device_output("out")],
        vec![
            edge("off", "disabled", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
            edge("on", "active", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ],
    );
    let plan = RenderPlan::prepare(&execution).unwrap();
    assert_eq!(plan.source_bindings().len(), 1);
    assert_eq!(plan.source_bindings()[0].node_id(), &NodeId::from("active"));
    let mut processor = BlockProcessor::new(Arc::new(plan));
    let mut output = [0.0; 8];
    processor
        .process_block(4, &[&[0.5; 8]], &mut [&mut output])
        .unwrap();
    assert_eq!(output.map(f32::to_bits), [0.5_f32; 8].map(f32::to_bits));
}

#[test]
fn source_and_sink_mute_preserve_native_bindings_but_silence_the_boundary() {
    for muted_index in [0, 1] {
        let mut nodes = vec![device_input("in"), device_output("out")];
        nodes[muted_index].control.muted = true;
        let execution = compile(
            nodes,
            vec![edge("route", "in", AUDIO_PORT_ID, "out", AUDIO_PORT_ID)],
        );
        let plan = RenderPlan::prepare(&execution).unwrap();
        assert_eq!(plan.source_bindings().len(), 1);
        assert_eq!(plan.sink_bindings().len(), 1);
        let mut processor = BlockProcessor::new(Arc::new(plan));
        let mut output = [9.0; 8];
        processor
            .process_block(4, &[&[1.0; 8]], &mut [&mut output])
            .unwrap();
        assert_eq!(output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    }
}

#[test]
fn disabled_recorder_has_no_tap_or_sink_routes() {
    let mut recorder = node("record", NodeConfig::Recorder { channels: 2 });
    recorder.control.enabled = false;
    let execution = compile(
        vec![device_input("in"), recorder],
        vec![edge("record", "in", AUDIO_PORT_ID, "record", AUDIO_PORT_ID)],
    );
    let plan = RenderPlan::prepare(&execution).unwrap();
    assert!(plan.recorder_bindings.is_empty());
    assert!(plan.sink_routes.is_empty());
}

fn prepare_dynamics(config: &NodeConfig, muted: bool, revision: u64) -> Box<BlockProcessor> {
    let mut effect = node("effect", config.clone());
    effect.control.muted = muted;
    let document = GraphDocument::new(
        revision,
        vec![device_input("in"), effect, device_output("out")],
        vec![
            edge("in", "in", AUDIO_PORT_ID, "effect", AUDIO_PORT_ID),
            edge("out", "effect", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ],
    );
    let execution = GraphCompiler::new(BuiltInNodeResolver)
        .compile(
            &document,
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    Box::new(BlockProcessor::new(Arc::new(
        RenderPlan::prepare(&execution).unwrap(),
    )))
}

#[test]
fn mute_publication_retains_running_dynamics_and_unmute_continues_the_envelope() {
    for config in [
        NodeConfig::Compressor(mixrelay_audio_graph::CompressorConfig {
            version: 1,
            channels: 2,
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            makeup_gain_db: 0.0,
        }),
        NodeConfig::Limiter(mixrelay_audio_graph::LimiterConfig {
            version: 1,
            channels: 2,
            ceiling_db: -6.0,
            release_ms: 100.0,
        }),
    ] {
        let mut current = prepare_dynamics(&config, false, 1);
        let mut reference = prepare_dynamics(&config, false, 1);
        let mut output = [0.0; 8];
        let mut expected = [0.0; 8];
        current
            .process_block(4, &[&[1.0; 8]], &mut [&mut output])
            .unwrap();
        reference
            .process_block(4, &[&[1.0; 8]], &mut [&mut expected])
            .unwrap();
        let exchange = crate::ProcessorExchange::new(1);
        for (revision, muted) in [(2, true), (3, false)] {
            exchange
                .submit(prepare_dynamics(&config, muted, revision))
                .unwrap();
            HEAP_OPERATIONS.with(|count| count.set(Some(0)));
            assert!(exchange.activate_pending(&mut current));
            assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
            assert_eq!(current.dynamics_envelopes, reference.dynamics_envelopes);
            current
                .process_block(4, &[&[0.5; 8]], &mut [&mut output])
                .unwrap();
            reference
                .process_block(4, &[&[0.5; 8]], &mut [&mut expected])
                .unwrap();
            assert_eq!(current.dynamics_envelopes, reference.dynamics_envelopes);
            if muted {
                assert_eq!(output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
            } else {
                assert_eq!(output.map(f32::to_bits), expected.map(f32::to_bits));
            }
        }
    }
}

fn prepare_bus(revision: u64, muted: bool, extra_bus: bool) -> Box<BlockProcessor> {
    let mut unrelated = node(
        "unrelated",
        NodeConfig::Gain(GainConfig {
            channels: 2,
            gain_db: 0.0,
        }),
    );
    unrelated.control.muted = muted;
    let mut nodes = vec![
        device_input("in"),
        node("bus", NodeConfig::DeviceBus { channels: 2 }),
        device_output("out"),
        unrelated,
    ];
    if extra_bus {
        // Changes the snapshot arena offsets without changing the surviving bus identity.
        nodes.push(node("a-new-bus", NodeConfig::DeviceBus { channels: 2 }));
    }
    let document = GraphDocument::new(
        revision,
        nodes,
        vec![
            edge("publish", "in", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
            edge("read", "bus", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ],
    );
    let execution = GraphCompiler::new(BuiltInNodeResolver)
        .compile(
            &document,
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    Box::new(BlockProcessor::new(Arc::new(
        RenderPlan::prepare(&execution).unwrap(),
    )))
}

#[test]
fn unrelated_mute_publication_retains_device_bus_history_without_allocation() {
    for extra_bus in [false, true] {
        let mut current = prepare_bus(1, false, false);
        let mut reference = prepare_bus(1, false, false);
        let (_, mut io, mut cycle) = callback_fixture();
        let (_, mut reference_io, _) = callback_fixture();
        io.value = 0.75;
        reference_io.value = 0.75;
        current.process_external_cycle(cycle, &mut io).unwrap();
        reference
            .process_external_cycle(cycle, &mut reference_io)
            .unwrap();
        let exchange = crate::ProcessorExchange::new(1);
        exchange.submit(prepare_bus(2, true, extra_bus)).unwrap();
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        assert!(exchange.activate_pending(&mut current));
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        cycle.first_frame += 4;
        io.value = 0.25;
        reference_io.value = 0.25;
        current.process_external_cycle(cycle, &mut io).unwrap();
        reference
            .process_external_cycle(cycle, &mut reference_io)
            .unwrap();
        assert_eq!(
            io.output.map(f32::to_bits),
            reference_io.output.map(f32::to_bits)
        );
        assert_eq!(io.output.map(f32::to_bits), [0.75_f32; 8].map(f32::to_bits));
        cycle.first_frame += 4;
        cycle.discontinuity = true;
        current.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    }
}
