use super::*;
use crate::macos::{
    NativeAudioConfig, NativeClockSnapshot,
    callback_source::RenderSnapshot,
    endpoint_boundaries::{BoundaryConfig, BoundaryWork, Directions, EndpointBoundary},
};
use mixrelay_audio_graph::{
    CompileContext, CompileLimits, GraphCompiler, GraphDocument, NodeConfig, RenderFormat,
};
use std::sync::Arc;

const IDENTITY: &str = "01010101-0101-0101-0101-010101010101";

struct Transport {
    through: u64,
    reads: u64,
    writes: u64,
    invalidations: u64,
}

impl CaptureTransport for Transport {
    fn clock(&mut self) -> Option<NativeClockSnapshot> {
        Some(NativeClockSnapshot {
            epoch: 1,
            sample_rate: 44_100,
            running: 1,
            capture_through: self.through,
            ..NativeClockSnapshot::default()
        })
    }
    fn write(&mut self, _: u64, _: &[f32], _: u32) -> bool {
        self.writes += 1;
        true
    }
    fn invalidate_capture(&mut self) -> bool {
        self.invalidations += 1;
        true
    }
}

impl RenderTransport for Transport {
    fn render_snapshot(&mut self) -> Option<RenderSnapshot> {
        Some(RenderSnapshot {
            quantum: 512,
            clock: self.clock().unwrap(),
            stream_epoch: 1,
            through: self.through,
            activity: InputActivity::Active,
            channels: 2,
        })
    }
    fn read_render(&mut self, _: u64, samples: &mut [f32], frames: u32) -> u32 {
        self.reads += 1;
        samples.fill(0.25);
        frames
    }
}

fn processor(enabled: bool) -> Box<BlockProcessor> {
    let mut document: GraphDocument = serde_json::from_value(serde_json::json!({
        "schemaVersion": 2, "revision": 1, "nodes": [{
            "id": "virtual", "type": "virtual_endpoint",
            "control": {"enabled": enabled, "muted": false},
            "config": {"id": IDENTITY,"displayName":"Fixture",
                "reference":{"render":true,"capture":true},
                "render":{"channels":2,"channelLayout":"stereo"},
                "capture":{"channels":2,"channelLayout":"stereo"},
                "supportedSampleRates":[44100],"defaultSampleRate":44100}
        }], "edges": []
    }))
    .unwrap();
    if let NodeConfig::VirtualEndpoint(endpoint) = &document.nodes[0].config {
        document.nodes[0].config = NodeConfig::VirtualEndpointTransport(endpoint.spec.clone());
    }
    let execution = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format: RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 32,
                },
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    Box::new(BlockProcessor::new(Arc::new(
        RenderPlan::prepare(&execution).unwrap(),
    )))
}

fn endpoint() -> EndpointBoundary<Transport> {
    EndpointBoundary::new(
        BoundaryConfig {
            identity: IDENTITY.into(),
            id: [1; 16],
            audio: NativeAudioConfig {
                capacity_frames: 16_384,
                render_channels: 2,
                capture_channels: 2,
            },
            graph_rate: 48_000,
            device_rate: 44_100,
            directions: Directions {
                source: true,
                sink: true,
            },
        },
        Transport {
            through: 6_000_000,
            reads: 0,
            writes: 0,
            invalidations: 0,
        },
    )
    .unwrap()
}

fn measure(enabled: bool) -> (BoundaryWork, u64, u64) {
    let mut processor = processor(enabled);
    let status = mixrelay_engine_service::session::AdapterRuntimeStatus::new(
        crate::macos::adapter_profile(),
    );
    let signals = RenderSignals::default();
    let mut renderer = Renderer {
        processor: &mut processor,
        activation: None,
        bindings: &[],
        clock_id: 7,
        epoch: 1,
        native_clock: None,
        status: &status,
        frame: 0,
        progress: &signals.progress,
        failed: &signals.failed,
        invalid: &signals.invalid,
        endpoints: EndpointBoundaries {
            endpoints: vec![endpoint()],
        },
        reconfigure: &signals.reconfigure,
    };
    renderer.endpoints.bind_controls(renderer.processor.plan());
    for callback in 1..=1000_u64 {
        renderer.endpoints.endpoints[0].transport.through =
            6_000_000 + callback * 32 * 44_100 / 48_000;
        assert!(renderer.render_buffers(Buffers::empty(), 32));
    }
    assert_eq!(renderer.frame, 32_000);
    assert_eq!(signals.progress.load(Ordering::Acquire), 1000);
    let endpoint = &renderer.endpoints.endpoints[0];
    (
        endpoint.work,
        endpoint.transport.reads,
        endpoint.transport.writes,
    )
}

#[test]
fn virtual_boundary_operation_count() {
    for enabled in [true, false] {
        let (work, reads, writes) = measure(enabled);
        eprintln!(
            "virtual_boundary enabled={enabled} callbacks=1000 work={work:?} transport_reads={reads} transport_writes={writes}"
        );
    }
}

#[test]
fn disabled_virtual_boundary_performs_no_conversion_or_pcm_transfer() {
    assert_eq!(measure(false), (BoundaryWork::default(), 0, 0));
    assert_eq!(
        measure(true),
        (
            BoundaryWork {
                source_begin: 1000,
                source_read: 1000,
                sink_begin: 1000,
                sink_push: 1000,
                sink_flush: 1000,
            },
            999,
            250
        )
    );
}

#[test]
fn coherent_power_changes_stop_and_resume_existing_virtual_converter_owner() {
    use mixrelay_audio_engine::{ProcessorExchange, RuntimeControlUpdate};
    use mixrelay_audio_graph::NodeControl;
    let mut processor = processor(true);
    let exchange = ProcessorExchange::new(1);
    let status = mixrelay_engine_service::session::AdapterRuntimeStatus::new(
        crate::macos::adapter_profile(),
    );
    let signals = RenderSignals::default();
    let mut renderer = Renderer {
        processor: &mut processor,
        activation: None,
        bindings: &[],
        clock_id: 7,
        epoch: 1,
        native_clock: None,
        status: &status,
        frame: 0,
        progress: &signals.progress,
        failed: &signals.failed,
        invalid: &signals.invalid,
        endpoints: EndpointBoundaries {
            endpoints: vec![endpoint()],
        },
        reconfigure: &signals.reconfigure,
    };
    renderer.endpoints.bind_controls(renderer.processor.plan());
    for (revision, enabled) in [(2, true), (3, false), (4, true)] {
        exchange
            .publish_controls(
                renderer.processor.plan().controls(),
                revision,
                &[RuntimeControlUpdate {
                    node_id: "virtual".into(),
                    control: NodeControl {
                        enabled,
                        muted: false,
                    },
                    gain_db: None,
                }],
            )
            .unwrap();
        let before = renderer.endpoints.endpoints[0].work;
        for _ in 0..100 {
            renderer.endpoints.endpoints[0].transport.through += 29;
            assert!(renderer.render_buffers(Buffers::empty(), 32));
        }
        let after = renderer.endpoints.endpoints[0].work;
        let expected = if enabled { 100 } else { 0 };
        assert_eq!(after.source_begin - before.source_begin, expected);
        assert_eq!(after.source_read - before.source_read, expected);
        assert_eq!(after.sink_begin - before.sink_begin, expected);
        assert_eq!(after.sink_push - before.sink_push, expected);
        assert_eq!(after.sink_flush - before.sink_flush, expected);
        assert_eq!(renderer.epoch, 1);
    }
    assert_eq!(renderer.frame, 9600);
    assert_eq!(renderer.endpoints.endpoints[0].transport.invalidations, 2);
    assert!(!signals.reconfigure.load(Ordering::Acquire));
}
