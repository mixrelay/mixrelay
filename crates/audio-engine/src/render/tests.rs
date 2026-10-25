mod builtin_effects;
mod compensation;
mod controls;
mod dynamics_telemetry;
mod history_policy;
mod memory;
mod memory_pressure;
mod nonfinite;
mod provider_buses;
mod recorder;
mod scaling;
use super::*;
use mixrelay_audio_graph::{AUDIO_PORT_ID, FROM_OS_PORT_ID, TO_OS_PORT_ID};
use mixrelay_audio_graph::{
    ApplicationCaptureRequest, ApplicationSelector, AudioDeviceInputConfig,
    AudioDeviceOutputConfig, BuiltInNodeResolver, ChannelLayout, ChannelMapping, ChannelRoute,
    CompileContext, CompileLimits, CompiledPort, DeviceSelector, EdgeId, EdgeSpec, GainConfig,
    GraphCompiler, GraphDocument, MixerConfig, NodeResolutionError, NodeResolver, NodeSpec,
    PortDirection, ProviderNodeConfig, ResolvedNode, SourcePlaybackPolicy, StreamSpec,
    VirtualEndpointId, VirtualEndpointSpec,
};
use mixrelay_audio_io::process::{ProcessCycle, ProcessReport, TransferOutcome};

const FORMAT: RenderFormat = RenderFormat {
    sample_rate: 48_000,
    block_frames: 4,
};

// Thread-local instrumentation keeps parallel tests out of the measured
// callback window. All native/plugin preparation stays outside that window.
std::thread_local! {
    static HEAP_OPERATIONS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static HEAP_BALANCE: std::cell::Cell<Option<HeapBalance>> = const { std::cell::Cell::new(None) };
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HeapBalance {
    allocated_bytes: usize,
    freed_bytes: usize,
    allocations: usize,
    frees: usize,
}
fn account_heap_bytes(allocated: Option<usize>, freed: Option<usize>) {
    let _ = HEAP_BALANCE.try_with(|count| {
        if let Some(mut balance) = count.get() {
            if let Some(bytes) = allocated {
                balance.allocated_bytes += bytes;
                balance.allocations += 1;
            }
            if let Some(bytes) = freed {
                balance.freed_bytes += bytes;
                balance.frees += 1;
            }
            count.set(Some(balance));
        }
    });
}
struct CountedAllocator;
fn count_heap_operation() {
    let _ = HEAP_OPERATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}
// SAFETY: ownership, alignment and sizes are delegated unchanged to System.
unsafe impl std::alloc::GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_heap_operation();
        let pointer = unsafe { std::alloc::System.alloc(layout) };
        if !pointer.is_null() {
            account_heap_bytes(Some(layout.size()), None);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        count_heap_operation();
        account_heap_bytes(None, Some(layout.size()));
        unsafe { std::alloc::System.dealloc(pointer, layout) };
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_heap_operation();
        let pointer = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            account_heap_bytes(Some(layout.size()), None);
        }
        pointer
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_heap_operation();
        let replacement = unsafe { std::alloc::System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            account_heap_bytes(Some(size), Some(layout.size()));
        }
        replacement
    }
}
#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;

struct CallbackIo {
    read_frames: u32,
    discontinuity: bool,
    activity: mixrelay_audio_io::process::InputActivity,
    write_frames: u32,
    value: f32,
    output: [f32; 8],
    reads: usize,
    writes: usize,
    observed: Option<ProcessCycle>,
}

impl ExternalIo for CallbackIo {
    fn read_source(
        &mut self,
        _: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        self.reads += 1;
        self.observed = Some(*cycle);
        destination.fill(self.value);
        TransferOutcome {
            activity: self.activity,
            discontinuity: self.discontinuity,
            ..TransferOutcome::complete(self.read_frames)
        }
    }
    fn write_sink(
        &mut self,
        _: &ExternalBinding,
        source: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        self.writes += 1;
        assert_eq!(self.observed, Some(*cycle));
        self.output[..source.len()].copy_from_slice(source);
        TransferOutcome::complete(self.write_frames)
    }
}

fn callback_fixture() -> (BlockProcessor, CallbackIo, ProcessCycle) {
    let execution = compile(
        vec![virtual_endpoint("io")],
        vec![edge("loop", "io", FROM_OS_PORT_ID, "io", TO_OS_PORT_ID)],
    );
    let processor =
        BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())).unwrap();
    let io = CallbackIo {
        read_frames: 4,
        discontinuity: false,
        activity: mixrelay_audio_io::process::InputActivity::Active,
        write_frames: 4,
        value: 0.0,
        output: [99.0; 8],
        reads: 0,
        writes: 0,
        observed: None,
    };
    let cycle = ProcessCycle {
        clock: mixrelay_audio_io::process::ClockDomain {
            authority: mixrelay_audio_io::session::ClockAuthority::Device,
            id: 1,
            epoch: 1,
        },
        frames: 4,
        sample_rate: 48_000,
        first_frame: 0,
        monotonic_ns: Some(123),
        discontinuity: false,
    };
    (processor, io, cycle)
}

#[test]
fn device_snapshot_never_replays_pcm_from_a_discontinued_input_timeline() {
    for (clock_restart, source_restart) in [(false, false), (true, false), (false, true)] {
        let execution = compile(
            vec![
                device_input("in"),
                node("bus", NodeConfig::DeviceBus { channels: 2 }),
                device_output("out"),
            ],
            vec![
                edge("publish", "in", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
                edge("read", "bus", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
            ],
        );
        let mut processor =
            BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())).unwrap();
        let (_, mut io, mut cycle) = callback_fixture();
        io.value = 1.0;
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        cycle.first_frame += 4;
        if clock_restart {
            cycle.clock.epoch += 1;
            cycle.discontinuity = true;
        }
        io.value = 0.0;
        io.discontinuity = source_restart;
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        let expected = if clock_restart || source_restart {
            0.0_f32
        } else {
            1.0
        };
        assert_eq!(io.output.map(f32::to_bits), [expected; 8].map(f32::to_bits));
    }
}

#[test]
fn only_explicit_active_demand_counts_missing_input_over_375_cycles() {
    use mixrelay_audio_io::process::InputActivity;
    for (activity, read_frames, expected) in [
        (InputActivity::Unknown, 0, 0),
        (InputActivity::Inactive, 0, 0),
        (InputActivity::Priming, 0, 0),
        (InputActivity::Active, 0, 375),
        (InputActivity::Active, 4, 0),
    ] {
        let (mut processor, mut io, cycle) = callback_fixture();
        io.activity = activity;
        io.read_frames = read_frames;
        let mut before = 0;
        let mut after = 0;
        for index in 0..375 {
            let report = processor
                .process_external_cycle(
                    ProcessCycle {
                        first_frame: index * u64::from(cycle.frames),
                        ..cycle
                    },
                    &mut io,
                )
                .unwrap()
                .expect("fixture admits every valid cycle");
            before += u64::from(read_frames < cycle.frames);
            after += report.input_starvations;
            assert_eq!(report.output_overflows, 0);
            assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        }
        assert_eq!(after, expected, "activity={activity:?}");
        assert_eq!(before, if read_frames == 0 { 375 } else { 0 });
        eprintln!("activity={activity:?} frames={read_frames}: previous={before}, current={after}");
    }
}

#[test]
fn silent_callback_is_success_but_short_transfers_are_reported() {
    let (mut processor, mut io, cycle) = callback_fixture();
    assert_eq!(
        processor.process_external_cycle(cycle, &mut io).unwrap(),
        Some(ProcessReport::default())
    );
    assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
    io.read_frames = 2;
    io.write_frames = 3;
    io.value = 1.0;
    let next = ProcessCycle {
        first_frame: 4,
        ..cycle
    };
    let report = processor
        .process_external_cycle(next, &mut io)
        .unwrap()
        .expect("fixture admits every valid cycle");
    assert_eq!(report.input_starvations, 1);
    assert_eq!(report.output_overflows, 1);
    assert_eq!(
        io.output.map(f32::to_bits),
        [1.0_f32, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0].map(f32::to_bits)
    );
}

#[test]
fn bad_clock_or_format_never_touches_native_io() {
    let (mut processor, mut io, cycle) = callback_fixture();
    for bad in [
        ProcessCycle {
            sample_rate: 44_100,
            ..cycle
        },
        ProcessCycle { frames: 8, ..cycle },
        ProcessCycle { frames: 0, ..cycle },
    ] {
        assert!(processor.process_external_cycle(bad, &mut io).is_err());
    }
    assert_eq!((io.reads, io.writes), (0, 0));
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert!(processor.process_external_cycle(cycle, &mut io).is_err());
    assert_eq!((io.reads, io.writes), (1, 1));
}

#[test]
fn oversized_transfer_is_an_adapter_contract_failure() {
    let (mut processor, mut io, cycle) = callback_fixture();
    io.read_frames = 5;
    assert_eq!(
        processor.process_external_cycle(cycle, &mut io),
        Err(ProcessBlockError::InvalidTransfer)
    );
    assert_eq!(io.writes, 0);
}

#[test]
fn validated_audio_cycles_do_not_allocate_or_free() {
    let (mut processor, mut io, mut cycle) = callback_fixture();
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    for _ in 0..10_000 {
        processor.process_external_cycle(cycle, &mut io).unwrap();
        cycle.first_frame += u64::from(cycle.frames);
    }
    let operations = HEAP_OPERATIONS.with(|count| count.replace(None));
    assert_eq!(operations, Some(0));
}

fn compile(nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec>) -> ExecutionPlan {
    GraphCompiler::default()
        .compile(
            &GraphDocument::new(1, nodes, edges),
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .expect("test graph should compile")
}

fn node(id: &str, config: NodeConfig) -> NodeSpec {
    NodeSpec {
        id: NodeId::from(id),
        label: None,
        control: mixrelay_audio_graph::NodeControl::default(),
        config,
    }
}

fn endpoint(node_id: &str, port_id: &str) -> AudioEndpoint {
    AudioEndpoint {
        node_id: NodeId::from(node_id),
        port_id: PortId::from(port_id),
    }
}

fn edge(
    id: &str,
    source_node: &str,
    source_port: &str,
    destination_node: &str,
    destination_port: &str,
) -> EdgeSpec {
    EdgeSpec {
        id: EdgeId::from(id),
        source: endpoint(source_node, source_port),
        destination: endpoint(destination_node, destination_port),
        channel_mapping: ChannelMapping::Auto,
    }
}

fn stereo_stream() -> StreamSpec {
    StreamSpec {
        channels: 2,
        channel_layout: ChannelLayout::Stereo,
    }
}

fn app_source(id: &str) -> NodeSpec {
    node(
        id,
        NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
            selector: ApplicationSelector::BundleId {
                bundle_id: format!("io.mixrelay.{id}"),
                include_descendants: true,
            },
            stream: stereo_stream(),
            source_playback: SourcePlaybackPolicy::Preserve,
            forwarding_endpoint: None,
            gain_db: 0.0,
            requires_reselection: false,
        }),
    )
}

fn device_input(id: &str) -> NodeSpec {
    node(
        id,
        NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
            selector: DeviceSelector::DefaultInput,
            output_channels: 2,
            gain_db: 0.0,
        }),
    )
}

fn device_output(id: &str) -> NodeSpec {
    node(
        id,
        NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
            selector: DeviceSelector::DefaultOutput,
            input_channels: 2,
        }),
    )
}

fn virtual_endpoint(id: &str) -> NodeSpec {
    node(
        id,
        NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
            id: VirtualEndpointId::from(id),
            display_name: id.into(),
            render: Some(stereo_stream()),
            capture: Some(stereo_stream()),
            supported_sample_rates: vec![48_000],
            default_sample_rate: 48_000,
        }),
    )
}

#[test]
fn external_bindings_keep_device_identity_separate_from_graph_node_names() {
    let mut endpoint = virtual_endpoint("canonical-device");
    endpoint.id = NodeId::from("graph-b::local-reference");
    let execution = compile(
        vec![endpoint, app_source("application"), device_input("mic")],
        vec![],
    );
    let plan = RenderPlan::prepare(&execution).expect("valid independent references");
    for binding in plan.source_bindings().iter().chain(plan.sink_bindings()) {
        let expected = (binding.node_id().as_str() == "graph-b::local-reference")
            .then_some("canonical-device");
        assert_eq!(
            binding.virtual_endpoint_id().map(VirtualEndpointId::as_str),
            expected
        );
    }
}

#[test]
fn native_transport_bindings_keep_asymmetric_device_geometry() {
    let mut endpoint = virtual_endpoint("device");
    if let NodeConfig::VirtualEndpointTransport(spec) = &mut endpoint.config {
        spec.capture = Some(StreamSpec {
            channels: 16,
            channel_layout: ChannelLayout::Discrete,
        });
    }
    let plan = RenderPlan::prepare(&compile(vec![endpoint], vec![])).unwrap();
    assert_eq!(plan.source_bindings()[0].channels(), 16);
    assert_eq!(plan.sink_bindings()[0].channels(), 2);
}

#[test]
fn authored_virtual_source_must_be_composed_before_rendering() {
    let mut endpoint = virtual_endpoint("device");
    if let NodeConfig::VirtualEndpointTransport(spec) = &endpoint.config {
        endpoint.config = NodeConfig::VirtualEndpoint(spec.clone().into());
    }
    let execution = compile(vec![endpoint], vec![]);
    assert!(matches!(RenderPlan::prepare(&execution),
        Err(RenderPlanError::UnloweredVirtualEndpoint {node_id}) if node_id.as_str()=="device"));
}

#[test]
fn device_bus_preserves_pcm_across_unequal_callback_lengths() {
    for channels in [1, 2, 8, 32, 64] {
        let mut processor = device_bus_processor(channels);
        let channels = usize::from(channels);
        let mut input = vec![0.0; 4 * channels];
        let mut output = vec![0.0; input.len()];
        let mut position = 0;
        let tag = |sample| f32::from(u16::try_from(sample % 16_384 + 1).unwrap());
        for frames in [1, 2, 4, 1, 3, 2, 2, 1, 4].into_iter().cycle().take(128) {
            for (sample, value) in input.iter_mut().enumerate() {
                *value = tag(position * channels + sample);
            }
            HEAP_OPERATIONS.with(|count| count.set(Some(0)));
            processor
                .process_block(frames, &[&input], &mut [&mut output])
                .unwrap();
            assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
            for (sample, actual) in output.iter().enumerate().take(frames as usize * channels) {
                let absolute = position * channels + sample;
                let expected = absolute.checked_sub(4 * channels).map_or(0.0, tag);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "channels={channels}, position={position}, sample={sample}"
                );
            }
            position += frames as usize;
        }
    }
}

fn device_bus_processor(channels: u16) -> BlockProcessor {
    let execution = compile(
        vec![
            node(
                "in",
                NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector: DeviceSelector::DefaultInput,
                    output_channels: channels,
                    gain_db: 0.0,
                }),
            ),
            node("bus", NodeConfig::DeviceBus { channels }),
            node(
                "out",
                NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                    selector: DeviceSelector::DefaultOutput,
                    input_channels: channels,
                }),
            ),
        ],
        vec![
            edge("publish", "in", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
            edge("read", "bus", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ],
    );
    BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())).unwrap()
}

#[test]
fn device_bus_replacement_retains_unread_tail_and_ring_position() {
    let mut previous = device_bus_processor(1);
    let mut output = [0.0; 4];
    previous
        .process_block(4, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut output])
        .unwrap();
    previous
        .process_block(1, &[&[5.0]], &mut [&mut output])
        .unwrap();
    assert_eq!(output[0].to_bits(), 1.0_f32.to_bits());
    let mut next = device_bus_processor(1);
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    next.retain_device_bus_state(&previous);
    next.process_block(4, &[&[6.0, 7.0, 8.0, 9.0]], &mut [&mut output])
        .unwrap();
    assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    assert_eq!(
        output.map(f32::to_bits),
        [2.0_f32, 3.0, 4.0, 5.0].map(f32::to_bits)
    );
}

struct IndependentBusIo {
    source_a: [f32; 8],
    source_b: [f32; 8],
    output_a: [f32; 8],
    output_b: [f32; 8],
    source_a_discontinuity: bool,
    admission: CycleAdmission,
}

impl ExternalIo for IndependentBusIo {
    fn begin_cycle(
        &mut self,
        _cycle: &ProcessCycle,
        _controls: ExternalControlState<'_>,
    ) -> CycleAdmission {
        self.admission
    }

    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        let (source, discontinuity) = match binding.node_id().as_str() {
            "source-a" => (&self.source_a, self.source_a_discontinuity),
            "source-b" => (&self.source_b, false),
            other => panic!("unexpected test source binding {other}"),
        };
        destination.copy_from_slice(&source[..destination.len()]);
        TransferOutcome {
            discontinuity,
            ..TransferOutcome::complete(cycle.frames)
        }
    }

    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        _cycle: &ProcessCycle,
    ) -> TransferOutcome {
        let destination = match binding.node_id().as_str() {
            "sink-a" => &mut self.output_a,
            "sink-b" => &mut self.output_b,
            other => panic!("unexpected test sink binding {other}"),
        };
        destination[..source.len()].copy_from_slice(source);
        TransferOutcome::complete(u32::try_from(source.len() / 2).unwrap())
    }
}

fn independent_bus_fixture() -> (BlockProcessor, IndependentBusIo, ProcessCycle) {
    let execution = compile(
        vec![
            device_input("source-a"),
            node("bus-a", NodeConfig::DeviceBus { channels: 2 }),
            device_output("sink-a"),
            device_input("source-b"),
            node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
            device_output("sink-b"),
        ],
        vec![
            edge(
                "publish-a",
                "source-a",
                AUDIO_PORT_ID,
                "bus-a",
                AUDIO_PORT_ID,
            ),
            edge("read-a", "bus-a", AUDIO_PORT_ID, "sink-a", AUDIO_PORT_ID),
            edge(
                "publish-b",
                "source-b",
                AUDIO_PORT_ID,
                "bus-b",
                AUDIO_PORT_ID,
            ),
            edge("read-b", "bus-b", AUDIO_PORT_ID, "sink-b", AUDIO_PORT_ID),
        ],
    );
    (
        BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())).unwrap(),
        IndependentBusIo {
            source_a: [1.0; 8],
            source_b: [2.0; 8],
            output_a: [0.0; 8],
            output_b: [0.0; 8],
            source_a_discontinuity: false,
            admission: CycleAdmission::Process,
        },
        ProcessCycle {
            clock: mixrelay_audio_io::process::ClockDomain {
                authority: mixrelay_audio_io::session::ClockAuthority::Device,
                id: 1,
                epoch: 0,
            },
            sample_rate: FORMAT.sample_rate,
            frames: FORMAT.block_frames,
            first_frame: 0,
            monotonic_ns: None,
            discontinuity: false,
        },
    )
}

#[test]
fn one_source_discontinuity_does_not_clear_unrelated_device_bus_history() {
    let (mut processor, mut io, mut cycle) = independent_bus_fixture();
    processor.process_external_cycle(cycle, &mut io).unwrap();
    cycle.first_frame += u64::from(cycle.frames);
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert_eq!(io.output_a.map(f32::to_bits), [1.0_f32.to_bits(); 8]);
    assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);

    cycle.first_frame += u64::from(cycle.frames);
    io.source_a_discontinuity = true;
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    let report = processor
        .process_external_cycle(cycle, &mut io)
        .unwrap()
        .unwrap();
    assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    assert_eq!(report.discontinuities, 1);
    assert_eq!(io.output_a.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
    assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
}

#[test]
fn lossless_clock_handoff_preserves_unrelated_bus_history_without_allocation() {
    for source_a_discontinuity in [false, true] {
        let (mut processor, mut io, mut cycle) = independent_bus_fixture();
        processor.process_external_cycle(cycle, &mut io).unwrap();
        cycle.first_frame += u64::from(cycle.frames);
        processor.process_external_cycle(cycle, &mut io).unwrap();

        io.source_a_discontinuity = source_a_discontinuity;
        io.admission = CycleAdmission::ClockHandoff;
        cycle.clock.id = 2;
        cycle.first_frame = 960;
        cycle.discontinuity = true;
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
        let expected_a = if source_a_discontinuity {
            0.0_f32
        } else {
            1.0_f32
        };
        assert_eq!(io.output_a.map(f32::to_bits), [expected_a.to_bits(); 8]);

        // Retaining PCM history must still commit the new clock coordinates.
        io.admission = CycleAdmission::Process;
        io.source_a_discontinuity = false;
        cycle.first_frame += u64::from(cycle.frames);
        cycle.discontinuity = false;
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
    }
}

#[test]
fn global_clock_discontinuity_clears_both_independent_bus_snapshots() {
    let (mut processor, mut io, mut cycle) = independent_bus_fixture();
    processor.process_external_cycle(cycle, &mut io).unwrap();
    cycle.first_frame += u64::from(cycle.frames);
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert_eq!(io.output_a.map(f32::to_bits), [1.0_f32.to_bits(); 8]);
    assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);

    cycle.first_frame += u64::from(cycle.frames);
    cycle.clock.epoch += 1;
    cycle.discontinuity = true;
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert_eq!(io.output_a.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
    assert_eq!(io.output_b.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
}

#[test]
fn local_source_restart_retains_other_bus_partial_quantum_position() {
    let (mut processor, mut io, mut cycle) = independent_bus_fixture();
    io.source_b = [2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0];
    processor.process_external_cycle(cycle, &mut io).unwrap();
    cycle.first_frame += 4;
    cycle.frames = 2;
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert_eq!(
        io.output_b.map(f32::to_bits)[..4],
        [2.0_f32, 2.0, 3.0, 3.0].map(f32::to_bits)
    );

    cycle.first_frame += 2;
    io.source_a_discontinuity = true;
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    processor.process_external_cycle(cycle, &mut io).unwrap();
    assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    assert_eq!(
        io.output_b.map(f32::to_bits)[..4],
        [4.0_f32, 4.0, 5.0, 5.0].map(f32::to_bits)
    );
}

#[test]
fn source_history_scope_follows_causal_cycles_without_reaching_other_branches() {
    let execution = compile(
        vec![
            device_input("source-a"),
            device_input("source-b"),
            node("bus-a", NodeConfig::DeviceBus { channels: 2 }),
            node("cycle-a", NodeConfig::DeviceBus { channels: 2 }),
            node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
        ],
        vec![
            edge("a", "source-a", AUDIO_PORT_ID, "bus-a", AUDIO_PORT_ID),
            edge("forward", "bus-a", AUDIO_PORT_ID, "cycle-a", AUDIO_PORT_ID),
            edge("feedback", "cycle-a", AUDIO_PORT_ID, "bus-a", AUDIO_PORT_ID),
            edge("b", "source-b", AUDIO_PORT_ID, "bus-b", AUDIO_PORT_ID),
        ],
    );
    let plan = RenderPlan::prepare(&execution).unwrap();
    for (binding, targets) in plan.source_bindings.iter().zip(&plan.source_history) {
        let expected = match binding.node_id().as_str() {
            "source-a" => (2, 3),
            "source-b" => (1, 1),
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(targets.device_buses.len(), expected.0);
        assert_eq!(targets.delay_routes.len(), expected.1);
    }
}

#[test]
fn source_history_does_not_cross_independent_duplex_transport_directions() {
    let execution = compile(
        vec![
            device_input("source-a"),
            virtual_endpoint("duplex-b"),
            node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
            device_output("sink-b"),
        ],
        vec![
            edge(
                "a-sink",
                "source-a",
                AUDIO_PORT_ID,
                "duplex-b",
                TO_OS_PORT_ID,
            ),
            edge(
                "b-source",
                "duplex-b",
                FROM_OS_PORT_ID,
                "bus-b",
                AUDIO_PORT_ID,
            ),
            edge("b-sink", "bus-b", AUDIO_PORT_ID, "sink-b", AUDIO_PORT_ID),
        ],
    );
    let plan = RenderPlan::prepare(&execution).unwrap();
    for (binding, targets) in plan.source_bindings.iter().zip(&plan.source_history) {
        let expected = match binding.node_id().as_str() {
            "source-a" => (0, 1),
            "duplex-b" => (1, 2),
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(targets.device_buses.len(), expected.0);
        assert_eq!(targets.delay_routes.len(), expected.1);
    }
}

#[test]
fn device_bus_has_one_prepared_quantum_latency_without_delaying_direct_hardware() {
    let nodes = vec![
        device_input("mic"),
        device_output("out"),
        node("bus", NodeConfig::DeviceBus { channels: 2 }),
        node(
            "gain",
            NodeConfig::Gain(GainConfig {
                channels: 2,
                gain_db: -6.020_600_3,
            }),
        ),
    ];
    let edges = vec![
        edge("publish", "mic", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
        edge("snapshot", "bus", AUDIO_PORT_ID, "gain", AUDIO_PORT_ID),
        edge("direct", "mic", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        edge("read", "gain", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
    ];
    let plan = Arc::new(RenderPlan::prepare(&compile(nodes, edges)).unwrap());
    let mut processor = BlockProcessor::new(plan).unwrap();
    let first = [0.2, 0.4, 0.6, 0.8, 0.1, 0.3, 0.5, 0.7];
    let second = [1.0; 8];
    let mut output = [0.0; 8];
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    processor
        .process_block(4, &[&first], &mut [&mut output])
        .unwrap();
    assert_eq!(output.map(f32::to_bits), first.map(f32::to_bits));
    processor
        .process_block(1, &[&second], &mut [&mut output])
        .unwrap();
    assert!((output[0] - 1.1).abs() < 0.000_001);
    assert!((output[1] - 1.2).abs() < 0.000_001);
    processor
        .process_block(3, &[&second], &mut [&mut output])
        .unwrap();
    for (actual, expected) in output.iter().zip([1.3, 1.4, 1.05, 1.15, 1.25, 1.35]) {
        assert!((actual - expected).abs() < 0.000_001);
    }
    assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
}

fn processor_graph() -> ExecutionPlan {
    let nodes = vec![
        app_source("app"),
        device_input("mic"),
        node(
            "mix",
            NodeConfig::Mixer(MixerConfig {
                inputs: vec![PortId::from("app"), PortId::from("mic")],
                channels: 2,
            }),
        ),
        node(
            "gain",
            NodeConfig::Gain(GainConfig {
                channels: 2,
                gain_db: -6.020_600_3,
            }),
        ),
        virtual_endpoint("virtual"),
        device_output("monitor"),
    ];
    let mut swapped = edge(
        "virtual-monitor",
        "virtual",
        FROM_OS_PORT_ID,
        "monitor",
        AUDIO_PORT_ID,
    );
    swapped.channel_mapping = ChannelMapping::Explicit {
        routes: vec![
            ChannelRoute {
                source_channel: 0,
                destination_channel: 1,
            },
            ChannelRoute {
                source_channel: 1,
                destination_channel: 0,
            },
        ],
    };
    compile(
        nodes,
        vec![
            edge("app-mix", "app", AUDIO_PORT_ID, "mix", "app"),
            edge("mic-mix", "mic", AUDIO_PORT_ID, "mix", "mic"),
            edge("mix-gain", "mix", AUDIO_PORT_ID, "gain", AUDIO_PORT_ID),
            edge(
                "gain-virtual",
                "gain",
                AUDIO_PORT_ID,
                "virtual",
                TO_OS_PORT_ID,
            ),
            swapped,
        ],
    )
}

#[test]
fn lowers_bindings_and_processes_gain_mix_and_channel_routes() {
    let plan = Arc::new(RenderPlan::prepare(&processor_graph()).expect("plan should lower"));
    assert_eq!(plan.revision(), 1);
    assert_eq!(plan.format(), FORMAT);
    assert_eq!(plan.arena_samples(), 80);
    assert_eq!(
        plan.source_bindings()
            .iter()
            .map(|binding| (
                binding.node_id().as_str(),
                binding.port_id().as_str(),
                binding.channels()
            ))
            .collect::<Vec<_>>(),
        [
            ("app", AUDIO_PORT_ID, 2),
            ("mic", AUDIO_PORT_ID, 2),
            ("virtual", FROM_OS_PORT_ID, 2),
        ]
    );
    assert_eq!(
        plan.sink_bindings()
            .iter()
            .map(|binding| (binding.node_id().as_str(), binding.port_id().as_str()))
            .collect::<Vec<_>>(),
        [("monitor", AUDIO_PORT_ID), ("virtual", TO_OS_PORT_ID)]
    );

    let app = [1.0, 2.0, 3.0, 4.0];
    let mic = [0.5, 0.5, 0.5, 0.5];
    let from_os = [10.0, 20.0, 30.0, 40.0];
    let sources: [&[f32]; 3] = [&app, &mic, &from_os];
    let mut to_os = [0.0; 4];
    let mut monitor = [0.0; 4];
    let mut sinks: [&mut [f32]; 2] = [&mut monitor, &mut to_os];
    let mut processor = BlockProcessor::new(Arc::clone(&plan)).unwrap();
    assert!(Arc::ptr_eq(processor.plan(), &plan));
    processor
        .process_block(2, &sources, &mut sinks)
        .expect("block should process");

    for (actual, expected) in monitor.into_iter().zip([20.0, 10.0, 40.0, 30.0]) {
        assert!((actual - expected).abs() < 0.000_001);
    }
    for (actual, expected) in to_os.into_iter().zip([0.75, 1.25, 1.75, 2.25]) {
        assert!((actual - expected).abs() < 0.000_001);
    }
}

struct TestProviderResolver;

impl NodeResolver for TestProviderResolver {
    fn resolve(
        &self,
        node: &NodeSpec,
        context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        if matches!(node.config, NodeConfig::Provider(_)) {
            return Ok(ResolvedNode {
                inputs: vec![CompiledPort::new(
                    PortId::from(AUDIO_PORT_ID),
                    PortDirection::Input,
                    2,
                )],
                outputs: vec![CompiledPort::new(
                    PortId::from(AUDIO_PORT_ID),
                    PortDirection::Output,
                    2,
                )],
            });
        }
        BuiltInNodeResolver.resolve(node, context)
    }
}

struct DoubleProcessor;

impl HostedAudioProcessor for DoubleProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool {
        let samples = frames * 2;
        for index in 0..samples {
            output[index] = input[index] * 2.0;
        }
        true
    }
}

struct TestProviderFactory;

impl HostedProcessorFactory for TestProviderFactory {
    fn prepare(
        &self,
        _request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String> {
        Ok(Box::new(DoubleProcessor))
    }
}

#[test]
fn hosted_provider_uses_prepared_instance_in_the_render_plan() {
    let document = GraphDocument::new(
        1,
        vec![
            app_source("app"),
            node(
                "provider",
                NodeConfig::Provider(ProviderNodeConfig {
                    ports: None,
                    type_id: mixrelay_audio_graph::NodeTypeId::from("test:double"),
                    parameters: serde_json::json!({}),
                }),
            ),
            device_output("speaker"),
        ],
        vec![
            edge(
                "app-provider",
                "app",
                AUDIO_PORT_ID,
                "provider",
                AUDIO_PORT_ID,
            ),
            edge(
                "provider-speaker",
                "provider",
                AUDIO_PORT_ID,
                "speaker",
                AUDIO_PORT_ID,
            ),
        ],
    );
    let execution = GraphCompiler::new(TestProviderResolver)
        .compile(
            &document,
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .expect("provider graph should compile");
    let meters = Arc::new(crate::PeakMeterBank::new(8, 8));
    let mut processor =
        BlockProcessor::prepare_with_hosted_processors(&execution, meters, &TestProviderFactory)
            .expect("provider should prepare");
    let source = [1.0, -1.0, 0.25, -0.25];
    let mut sink = [0.0; 4];
    processor
        .process_block(2, &[&source], &mut [&mut sink])
        .expect("provider block should process");
    for (actual, expected) in sink.iter().zip([2.0, -2.0, 0.5, -0.5]) {
        assert!((actual - expected).abs() <= f32::EPSILON);
    }
}

#[test]
fn processes_a_duplex_virtual_endpoint_through_gain_without_a_false_cycle() {
    let execution = compile(
        vec![
            virtual_endpoint("virtual"),
            node(
                "gain",
                NodeConfig::Gain(GainConfig {
                    channels: 2,
                    gain_db: -6.020_600_3,
                }),
            ),
        ],
        vec![
            edge(
                "from-os-to-gain",
                "virtual",
                FROM_OS_PORT_ID,
                "gain",
                AUDIO_PORT_ID,
            ),
            edge(
                "gain-to-os",
                "gain",
                AUDIO_PORT_ID,
                "virtual",
                TO_OS_PORT_ID,
            ),
        ],
    );
    let plan = Arc::new(RenderPlan::prepare(&execution).expect("gain path should lower"));
    let mut processor = BlockProcessor::new(plan).unwrap();
    let source = [1.0, -1.0, 0.5, -0.5];
    let mut sink = [0.0; 4];

    processor
        .process_block(2, &[&source], &mut [&mut sink])
        .expect("gain path should process");

    for (actual, expected) in sink.into_iter().zip([0.5, -0.5, 0.25, -0.25]) {
        assert!((actual - expected).abs() < 0.000_001);
    }
}

#[test]
fn virtual_source_gain_scales_pcm_and_both_meter_banks_once_without_allocations() {
    for channels in [2_u16, 8, 16, 32, 64] {
        let mut endpoint = virtual_endpoint("virtual");
        let NodeConfig::VirtualEndpointTransport(config) = &mut endpoint.config else {
            unreachable!();
        };
        let stream = StreamSpec {
            channels,
            channel_layout: ChannelLayout::Discrete,
        };
        config.render = Some(stream);
        config.capture = Some(stream);
        let execution = compile(
            vec![
                endpoint,
                node(
                    "local-gain",
                    NodeConfig::Gain(GainConfig {
                        channels,
                        gain_db: -6.020_600_3,
                    }),
                ),
            ],
            vec![
                edge(
                    "read",
                    "virtual",
                    FROM_OS_PORT_ID,
                    "local-gain",
                    AUDIO_PORT_ID,
                ),
                edge(
                    "write",
                    "local-gain",
                    AUDIO_PORT_ID,
                    "virtual",
                    TO_OS_PORT_ID,
                ),
            ],
        );
        let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
        let meters = Arc::new(crate::PeakMeterBank::new(plan.meter_bindings().len(), 64));
        let mut processor =
            BlockProcessor::with_peak_meters(Arc::clone(&plan), Arc::clone(&meters)).unwrap();
        let source: Vec<f32> = (0..usize::from(channels) * 2)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let mut sink = vec![0.0; source.len()];
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        for _ in 0..100 {
            processor
                .process_block(2, &[&source], &mut [&mut sink])
                .unwrap();
        }
        let operations = HEAP_OPERATIONS.with(|count| count.replace(None));
        assert_eq!(operations, Some(0));
        for (input, output) in source.iter().zip(sink) {
            assert!((output - input * 0.5).abs() < 0.000_001);
        }
        for (index, binding) in plan.meter_bindings().iter().enumerate() {
            let expected = if binding.port_id().as_str() == FROM_OS_PORT_ID {
                1.0
            } else {
                0.5
            };
            for peak in meters.take_peaks(1, index, channels) {
                assert!((peak - expected).abs() < 0.000_001);
            }
        }
    }
}

#[test]
fn virtual_source_gain_does_not_attenuate_an_independent_application_capture_path() {
    let mut endpoint = virtual_endpoint("virtual");
    let NodeConfig::VirtualEndpointTransport(config) = &mut endpoint.config else {
        unreachable!();
    };
    let _ = config;
    let execution = compile(
        vec![
            endpoint,
            app_source("app"),
            node(
                "local-gain",
                NodeConfig::Gain(GainConfig {
                    channels: 2,
                    gain_db: -12.0,
                }),
            ),
        ],
        vec![
            edge(
                "source-gain",
                "virtual",
                FROM_OS_PORT_ID,
                "local-gain",
                AUDIO_PORT_ID,
            ),
            edge("app-to-os", "app", AUDIO_PORT_ID, "virtual", TO_OS_PORT_ID),
        ],
    );
    let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
    let app = [1.0; 4];
    let virtual_input = [2.0; 4];
    let sources: Vec<&[f32]> = plan
        .source_bindings()
        .iter()
        .map(|binding| {
            if binding.port_id().as_str() == FROM_OS_PORT_ID {
                virtual_input.as_slice()
            } else {
                app.as_slice()
            }
        })
        .collect();
    let mut processor = BlockProcessor::new(plan).unwrap();
    let mut output = [0.0; 4];
    processor
        .process_block(2, &sources, &mut [&mut output])
        .unwrap();
    for sample in output {
        assert!((sample - 1.0).abs() < f32::EPSILON);
    }
}

#[test]
fn rejects_invalid_block_shapes_before_writing_outputs() {
    let plan = Arc::new(RenderPlan::prepare(&processor_graph()).expect("plan should lower"));
    let mut processor = BlockProcessor::new(plan).unwrap();
    let full = [0.0; 8];
    let sources: [&[f32]; 3] = [&full, &full, &full];
    let mut first = [9.0; 8];
    let mut second = [9.0; 8];

    assert_eq!(
        processor.process_block(5, &sources, &mut [&mut first, &mut second]),
        Err(ProcessBlockError::FrameCountExceeded {
            maximum: 4,
            found: 5,
        })
    );
    assert_eq!(
        processor.process_block(2, &sources[..2], &mut [&mut first, &mut second]),
        Err(ProcessBlockError::SourceCountMismatch {
            expected: 3,
            found: 2,
        })
    );
    assert_eq!(
        processor.process_block(2, &sources, &mut [&mut first]),
        Err(ProcessBlockError::SinkCountMismatch {
            expected: 2,
            found: 1,
        })
    );
    let short = [0.0; 3];
    let short_sources: [&[f32]; 3] = [&short, &full, &full];
    assert_eq!(
        processor.process_block(2, &short_sources, &mut [&mut first, &mut second]),
        Err(ProcessBlockError::SourceBufferTooShort {
            index: 0,
            required: 4,
            found: 3,
        })
    );
    let mut short_sink = [0.0; 3];
    assert_eq!(
        processor.process_block(2, &sources, &mut [&mut short_sink, &mut second]),
        Err(ProcessBlockError::SinkBufferTooShort {
            index: 0,
            required: 4,
            found: 3,
        })
    );
    assert!(
        first
            .into_iter()
            .all(|sample| (sample - 9.0).abs() < f32::EPSILON)
    );
    assert!(
        second
            .into_iter()
            .all(|sample| (sample - 9.0).abs() < f32::EPSILON)
    );
}

#[derive(Clone, Copy)]
enum ResolverShape {
    MissingSourceOutput,
    MissingSinkInput,
    MissingMixerInput,
    MissingMixerOutput,
    MissingGainInput,
    MissingVirtualRender,
    MissingVirtualCapture,
    MissingGainOutput,
    DuplicateProviderPort,
    ValidProvider,
}

struct ShapeResolver(ResolverShape);

impl NodeResolver for ShapeResolver {
    fn resolve(
        &self,
        _node: &NodeSpec,
        _context: &CompileContext,
    ) -> Result<ResolvedNode, NodeResolutionError> {
        let input = CompiledPort::new(PortId::from(AUDIO_PORT_ID), PortDirection::Input, 2);
        let output = CompiledPort::new(PortId::from(AUDIO_PORT_ID), PortDirection::Output, 2);
        Ok(match self.0 {
            ResolverShape::MissingSourceOutput | ResolverShape::MissingSinkInput => ResolvedNode {
                inputs: vec![],
                outputs: vec![],
            },
            ResolverShape::MissingMixerInput | ResolverShape::MissingGainInput => ResolvedNode {
                inputs: vec![],
                outputs: vec![output],
            },
            ResolverShape::MissingMixerOutput => ResolvedNode {
                inputs: vec![CompiledPort::new(
                    PortId::from("one"),
                    PortDirection::Input,
                    2,
                )],
                outputs: vec![],
            },
            ResolverShape::MissingVirtualRender => ResolvedNode {
                inputs: vec![CompiledPort::new(
                    PortId::from(TO_OS_PORT_ID),
                    PortDirection::Input,
                    2,
                )],
                outputs: vec![],
            },
            ResolverShape::MissingVirtualCapture => ResolvedNode {
                inputs: vec![],
                outputs: vec![CompiledPort::new(
                    PortId::from(FROM_OS_PORT_ID),
                    PortDirection::Output,
                    2,
                )],
            },
            ResolverShape::MissingGainOutput => ResolvedNode {
                inputs: vec![input],
                outputs: vec![],
            },
            ResolverShape::DuplicateProviderPort => ResolvedNode {
                inputs: vec![input.clone(), input],
                outputs: vec![output],
            },
            ResolverShape::ValidProvider => ResolvedNode {
                inputs: vec![input],
                outputs: vec![output],
            },
        })
    }
}

fn custom_plan(config: NodeConfig, shape: ResolverShape) -> ExecutionPlan {
    GraphCompiler::new(ShapeResolver(shape))
        .compile(
            &GraphDocument::new(1, vec![node("custom", config)], vec![]),
            CompileContext {
                format: FORMAT,
                limits: CompileLimits::default(),
            },
        )
        .expect("custom graph should compile")
}

#[test]
fn rejects_custom_resolver_shapes_that_cannot_render_safely() {
    let missing_port = |config, shape, port_id| {
        assert_eq!(
            RenderPlan::prepare(&custom_plan(config, shape))
                .expect_err("required port must be present"),
            RenderPlanError::MissingPort {
                node_id: NodeId::from("custom"),
                port_id: PortId::from(port_id),
            }
        );
    };

    missing_port(
        app_source("custom").config,
        ResolverShape::MissingSourceOutput,
        AUDIO_PORT_ID,
    );
    missing_port(
        device_output("custom").config,
        ResolverShape::MissingSinkInput,
        AUDIO_PORT_ID,
    );
    missing_port(
        NodeConfig::Mixer(MixerConfig {
            inputs: vec![PortId::from("one")],
            channels: 2,
        }),
        ResolverShape::MissingMixerInput,
        "one",
    );
    missing_port(
        NodeConfig::Mixer(MixerConfig {
            inputs: vec![PortId::from("one")],
            channels: 2,
        }),
        ResolverShape::MissingMixerOutput,
        AUDIO_PORT_ID,
    );
    let gain = NodeConfig::Gain(GainConfig {
        channels: 2,
        gain_db: 0.0,
    });
    missing_port(gain.clone(), ResolverShape::MissingGainInput, AUDIO_PORT_ID);
    missing_port(gain, ResolverShape::MissingGainOutput, AUDIO_PORT_ID);
    missing_port(
        virtual_endpoint("custom").config,
        ResolverShape::MissingVirtualRender,
        FROM_OS_PORT_ID,
    );
    let capture_only = NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
        id: VirtualEndpointId::from("custom"),
        display_name: "custom".into(),
        render: Some(stereo_stream()),
        capture: None,
        supported_sample_rates: vec![48_000],
        default_sample_rate: 48_000,
    });
    missing_port(
        capture_only,
        ResolverShape::MissingVirtualCapture,
        TO_OS_PORT_ID,
    );
}

#[test]
fn rejects_unregistered_or_ambiguous_provider_kernels() {
    let provider = NodeConfig::Provider(ProviderNodeConfig {
        ports: None,
        type_id: "io.mixrelay.test".into(),
        parameters: serde_json::Value::Null,
    });
    assert_eq!(
        RenderPlan::prepare(&custom_plan(
            provider.clone(),
            ResolverShape::DuplicateProviderPort,
        ))
        .expect_err("duplicate ports are ambiguous"),
        RenderPlanError::DuplicatePort {
            node_id: NodeId::from("custom"),
            port_id: PortId::from(AUDIO_PORT_ID),
        }
    );
    assert_eq!(
        RenderPlan::prepare(&custom_plan(provider, ResolverShape::ValidProvider))
            .expect_err("provider kernel must be registered"),
        RenderPlanError::UnsupportedProvider {
            node_id: NodeId::from("custom"),
        }
    );
}

#[test]
fn unconnected_application_and_hardware_sources_are_drained_and_metered() {
    let execution = compile(
        vec![app_source("app"), device_input("mic"), device_output("out")],
        vec![],
    );
    let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
    assert_eq!(plan.source_bindings().len(), 2);
    assert!(plan.sink_bindings().is_empty());
    let meters = Arc::new(crate::PeakMeterBank::new(plan.meter_bindings().len(), 64));
    let mut processor = BlockProcessor::with_peak_meters(plan.clone(), meters.clone()).unwrap();
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    processor
        .process_block(
            2,
            &[&[0.5, -0.25, 0.1, 0.2], &[0.75, -0.5, 0.2, 0.3]],
            &mut [],
        )
        .unwrap();
    assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    for (index, binding) in plan.meter_bindings().iter().enumerate() {
        let peaks = meters.take_peaks(1, index, 2);
        let expected = match binding.node_id().as_str() {
            "app" => [0.5, 0.25],
            "mic" => [0.75, 0.5],
            "out" => [0.0, 0.0],
            _ => unreachable!(),
        };
        assert_eq!(peaks, expected);
    }
}

#[test]
fn supports_render_only_virtual_endpoints() {
    let render_only = node(
        "render-only",
        NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
            id: VirtualEndpointId::from("render-only"),
            display_name: "render-only".into(),
            render: Some(stereo_stream()),
            capture: None,
            supported_sample_rates: vec![48_000],
            default_sample_rate: 48_000,
        }),
    );
    RenderPlan::prepare(&compile(vec![render_only], vec![]))
        .expect("render-only endpoint should lower");
}

#[test]
fn supports_render_only_contribution_sink_with_no_capture_source() {
    let capture_only = NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
        id: VirtualEndpointId::from("capture-only"),
        display_name: "Capture only".into(),
        render: Some(stereo_stream()),
        capture: None,
        supported_sample_rates: vec![48_000],
        default_sample_rate: 48_000,
    });
    let execution = compile(vec![node("capture-only", capture_only)], vec![]);
    let plan = Arc::new(RenderPlan::prepare(&execution).expect("plan should lower"));
    assert!(plan.source_bindings().is_empty());
    assert_eq!(plan.sink_bindings().len(), 1);
    let mut processor = BlockProcessor::new(plan).unwrap();
    let mut sink = [1.0; 4];

    processor
        .process_block(2, &[], &mut [&mut sink])
        .expect("capture-only endpoint should render silence");
    assert!(sink.iter().all(|sample| sample.abs() < f32::EPSILON));
}

#[test]
fn routes_virtual_endpoint_render_to_capture_inside_one_node() {
    let execution = compile(
        vec![virtual_endpoint("pass-through")],
        vec![edge(
            "internal",
            "pass-through",
            FROM_OS_PORT_ID,
            "pass-through",
            TO_OS_PORT_ID,
        )],
    );
    let plan = Arc::new(RenderPlan::prepare(&execution).expect("pass-through should lower"));
    let input = [1.0, -1.0, 0.5, -0.5];
    let mut output = [0.0; 4];
    let mut processor = BlockProcessor::new(plan).unwrap();
    processor
        .process_block(2, &[&input], &mut [&mut output])
        .expect("internal route should process");
    for (actual, expected) in output.into_iter().zip(input) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}

#[test]
fn rejects_render_arena_size_overflow() {
    let stereo = compile(vec![app_source("stereo")], vec![]);
    let multiply_error =
        allocate_slots(stereo.nodes(), usize::MAX).expect_err("stereo slot size must overflow");
    assert_eq!(multiply_error, RenderPlanError::BufferSizeOverflow);

    let mono = |id| {
        node(
            id,
            NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
                selector: ApplicationSelector::BundleId {
                    bundle_id: format!("io.mixrelay.{id}"),
                    include_descendants: true,
                },
                stream: StreamSpec {
                    channels: 1,
                    channel_layout: ChannelLayout::Mono,
                },
                source_playback: SourcePlaybackPolicy::Preserve,
                forwarding_endpoint: None,
                gain_db: 0.0,
                requires_reselection: false,
            }),
        )
    };
    let two_mono = compile(vec![mono("first"), mono("second")], vec![]);
    let add_error = allocate_slots(two_mono.nodes(), usize::MAX)
        .expect_err("second mono slot offset must overflow");
    assert_eq!(add_error, RenderPlanError::BufferSizeOverflow);
}
