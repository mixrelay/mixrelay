use super::*;
use crate::macos::endpoint_boundaries::{BoundaryConfig, Directions, EndpointBoundary};
use std::sync::Arc;
use tonenari_audio_engine::RenderPlan;
use tonenari_audio_graph::{
    CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
};
use tonenari_engine_service::RuntimeStatusBackend;
const FIRST_DEVICE: &str = "01010101-0101-0101-0101-010101010101";
const SECOND_DEVICE: &str = "02020202-0202-0202-0202-020202020202";

fn processor() -> BlockProcessor {
    prepare_processor(false)
}

fn prepare_processor(second: bool) -> BlockProcessor {
    let mut document: GraphDocument = serde_json::from_value(serde_json::json!({
        "schemaVersion":2,"revision":1,"nodes":[
            {"id":"input","type":"audio_device_input","config":{"selector":{"match":"default_input"},"outputChannels":2,"gainDb":0}},
            {"id":"output","type":"audio_device_output","config":{"selector":{"match":"default_output"},"inputChannels":2}},
            {"id":"default-virtual-io","type":"virtual_endpoint","config":{
                "id":FIRST_DEVICE,"displayName":"Test","render":null,
                "reference":{"render":false,"capture":true},
                "capture":{"channels":2,"channelLayout":"stereo"},
                "supportedSampleRates":[96000],"defaultSampleRate":96000}}
        ],"edges":[
            {"id":"headphone","source":{"nodeId":"input","portId":"audio"},"destination":{"nodeId":"output","portId":"audio"}},
            {"id":"hal","source":{"nodeId":"input","portId":"audio"},"destination":{"nodeId":"default-virtual-io","portId":tonenari_audio_graph::TO_OS_PORT_ID}}
        ]
    })).unwrap();
    if second {
        let mut node = document.nodes[2].clone();
        node.id = "graph-b::other-vio".into();
        if let tonenari_audio_graph::NodeConfig::VirtualEndpoint(endpoint) = &mut node.config {
            endpoint.id = SECOND_DEVICE.into();
            endpoint.default_sample_rate = 44_100;
            endpoint.supported_sample_rates = vec![44_100];
        }
        document.nodes.push(node);
        let mut edge = document.edges[1].clone();
        edge.id = "second-route".into();
        edge.destination.node_id = "graph-b::other-vio".into();
        document.edges.push(edge);
    }
    // This fixture exercises native transport ownership, not authored bus composition.
    for node in &mut document.nodes {
        if let tonenari_audio_graph::NodeConfig::VirtualEndpoint(endpoint) = &node.config {
            node.config =
                tonenari_audio_graph::NodeConfig::VirtualEndpointTransport(endpoint.spec.clone());
        }
    }
    let execution = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format: RenderFormat {
                    sample_rate: 96_000,
                    block_frames: 32,
                },
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()))
}

struct Hal {
    frame: u64,
    written: usize,
    energy: f32,
    running: bool,
    rate: u32,
    generation: u64,
}
impl CaptureTransport for Hal {
    fn invalidate_capture(&mut self) -> bool {
        true
    }
    fn clock(&mut self) -> Option<super::super::NativeClockSnapshot> {
        Some(super::super::NativeClockSnapshot {
            epoch: self.generation,
            capture_through: self.frame,
            sample_rate: self.rate,
            running: u32::from(self.running),
            ..Default::default()
        })
    }
    fn write(&mut self, frame: u64, pcm: &[f32], frames: u32) -> bool {
        assert!(frame > 5_000_000, "never hardware's zero-based frame index");
        self.written += frames as usize;
        self.energy += pcm.iter().map(|v| v.abs()).sum::<f32>();
        true
    }
}
impl RenderTransport for Hal {}

fn assert_passthrough(
    renderer: &mut Renderer<'_, Hal>,
    input: &mut [f32; 64],
    output: &mut [f32; 64],
) {
    output.fill(0.0);
    let source = super::tests::stereo_list(input);
    let mut sink = super::tests::stereo_list(output);
    // SAFETY: these stereo ABLs borrow 32 initialized frames for this synchronous
    // call; no callback retains their pointers after rendering returns.
    assert!(unsafe { renderer.render((&raw const source).cast(), (&raw mut sink).cast(), 32,) });
    assert_eq!(input.map(f32::to_bits), output.map(f32::to_bits));
}

#[test]
fn hardware_output_remains_synchronous_while_virtual_sink_crosses_clock_boundary() {
    let mut processor = processor();
    let status = tonenari_engine_service::session::AdapterRuntimeStatus::new(
        crate::macos::adapter_profile(),
    );
    let progress = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    let invalid = AtomicBool::new(false);
    let bindings = super::tests::stereo_bindings();
    let endpoint = EndpointBoundary::new(
        BoundaryConfig {
            identity: FIRST_DEVICE.into(),
            id: [1; 16],
            graph_rate: 96_000,
            device_rate: 96_000,
            directions: Directions {
                source: false,
                sink: true,
            },
            audio: super::super::NativeAudioConfig {
                capacity_frames: 16_384,
                render_channels: 2,
                capture_channels: 2,
            },
        },
        Hal {
            frame: 5_000_000,
            written: 0,
            energy: 0.0,
            running: true,
            rate: 96_000,
            generation: 4,
        },
    )
    .unwrap();
    let mut renderer = Renderer {
        processor: &mut processor,
        bindings: &bindings,
        clock_id: 9,
        epoch: 1,
        native_clock: None,
        status: &status,
        frame: 0,
        progress: &progress,
        failed: &failed,
        invalid: &invalid,
        endpoints: EndpointBoundaries {
            endpoints: vec![endpoint],
        },
        reconfigure: &invalid,
    };
    let mut input = [0.25; 64];
    let mut output = [0.0; 64];
    for _ in 0..5000 {
        renderer.endpoints.endpoints[0].transport.frame += 32;
        assert_passthrough(&mut renderer, &mut input, &mut output);
    }
    let transport = &mut renderer.endpoints.endpoints[0].transport;
    assert!(transport.written > 159_000);
    assert!(transport.energy > 10_000.0);
    let measured = status.status(RenderFormat {
        sample_rate: 96_000,
        block_frames: 32,
    });
    assert_eq!(measured.underruns, 0);
    assert_eq!(measured.overruns, 0);
    assert_eq!(progress.load(Ordering::Acquire), 5000);
    transport.running = false;
    let written = transport.written;
    for _ in 0..100 {
        assert_passthrough(&mut renderer, &mut input, &mut output);
        assert!(renderer.endpoints.synchronized());
    }
    assert_eq!(renderer.endpoints.endpoints[0].transport.written, written);
}

#[test]
fn two_virtual_devices_keep_distinct_clock_coordinates_rates_and_recovery() {
    let mut processor = prepare_processor(true);
    let status = tonenari_engine_service::session::AdapterRuntimeStatus::new(
        crate::macos::adapter_profile(),
    );
    let progress = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    let invalid = AtomicBool::new(false);
    let bindings = super::tests::stereo_bindings();
    let endpoints = [
        (FIRST_DEVICE, 96_000, 6_000_000),
        (SECOND_DEVICE, 44_100, 12_000_000),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (identity, rate, frame))| {
        EndpointBoundary::new(
            BoundaryConfig {
                identity: identity.into(),
                id: [u8::try_from(index + 1).unwrap(); 16],
                audio: super::super::NativeAudioConfig {
                    capacity_frames: 16_384,
                    render_channels: 0,
                    capture_channels: 2,
                },
                graph_rate: 96_000,
                device_rate: rate,
                directions: Directions {
                    source: false,
                    sink: true,
                },
            },
            Hal {
                frame,
                written: 0,
                energy: 0.0,
                running: true,
                rate,
                generation: 4,
            },
        )
        .unwrap()
    })
    .collect();
    let mut renderer = Renderer {
        processor: &mut processor,
        bindings: &bindings,
        clock_id: 9,
        epoch: 1,
        native_clock: None,
        status: &status,
        frame: 0,
        progress: &progress,
        failed: &failed,
        invalid: &invalid,
        endpoints: EndpointBoundaries { endpoints },
        reconfigure: &invalid,
    };
    let mut input = [0.25; 64];
    let mut output = [0.0; 64];
    for callback in 1..=5000_u64 {
        renderer.endpoints.endpoints[0].transport.frame = 6_000_000 + callback * 32;
        renderer.endpoints.endpoints[1].transport.frame =
            12_000_000 + callback * 32 * 44_100 / 96_000;
        if callback == 2500 {
            renderer.endpoints.endpoints[1].transport.generation += 1;
        }
        assert_passthrough(&mut renderer, &mut input, &mut output);
    }
    let first = &renderer.endpoints.endpoints[0].transport;
    let second = &renderer.endpoints.endpoints[1].transport;
    assert!(first.written > 159_000);
    assert!(second.written > 70_000 && second.written < 75_000);
    assert!(first.energy > 10_000.0 && second.energy > 5000.0);
    assert_eq!(first.generation, 4);
    assert_eq!(second.generation, 5);
    assert_eq!(progress.load(Ordering::Acquire), 5000);
}

#[test]
fn capture_retirement_follows_descriptor_drain_even_on_early_error() {
    use std::cell::RefCell;
    let order = RefCell::new(Vec::new());
    let queued_pcm = RefCell::new(vec![0.75_f32; 64]);
    let fail = || -> Result<(), ()> {
        let _capture_retirement = CaptureRetirement(|| {
            order.borrow_mut().push("invalidate");
            queued_pcm.borrow_mut().fill(0.0);
        });
        let _descriptor = CaptureRetirement(|| {
            order.borrow_mut().push("drain");
        });
        Err(())
    };
    assert!(fail().is_err());
    assert_eq!(*order.borrow(), vec!["drain", "invalidate"]);
    assert!(
        queued_pcm
            .borrow()
            .iter()
            .all(|sample| sample.abs() < f32::EPSILON)
    );
}
