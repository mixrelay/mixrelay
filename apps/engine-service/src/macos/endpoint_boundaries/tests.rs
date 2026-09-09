use super::*;
use tonenari_audio_graph::{
    CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
};

struct Disconnected;
impl CaptureTransport for Disconnected {
    fn clock(&mut self) -> Option<super::super::NativeClockSnapshot> {
        None
    }
    fn write(&mut self, _: u64, _: &[f32], _: u32) -> bool {
        false
    }
    fn invalidate_capture(&mut self) -> bool {
        false
    }
}
impl RenderTransport for Disconnected {}

fn idle_boundary(id: u8, rate: u32) -> EndpointBoundary<Disconnected> {
    EndpointBoundary::new(
        BoundaryConfig {
            identity: tonenari_audio_graph::VirtualEndpointId::new(format!(
                "{id:08x}-0000-0000-0000-000000000000"
            )),
            id: [id; 16],
            audio: NativeAudioConfig {
                capacity_frames: 16_384,
                render_channels: 2,
                capture_channels: 2,
            },
            graph_rate: 48_000,
            device_rate: rate,
            directions: Directions {
                source: true,
                sink: true,
            },
        },
        Disconnected,
    )
    .unwrap()
}

#[test]
fn reconciliation_keeps_healthy_pcm_owner_and_retires_only_changed_or_removed_devices() {
    let mut unchanged = idle_boundary(1, 48_000);
    unchanged.render_pcm[..64].fill(0.25);
    unchanged.capture_pcm[..64].fill(-0.75);
    let render_address = unchanged.render_pcm.as_ptr();
    let capture_address = unchanged.capture_pcm.as_ptr();
    let mut bank = EndpointBoundaries {
        endpoints: vec![unchanged, idle_boundary(2, 0), idle_boundary(3, 48_000)],
    };
    let mut retired = Vec::new();
    bank.reconcile(
        EndpointBoundaries {
            endpoints: vec![idle_boundary(1, 48_000), idle_boundary(2, 44_100)],
        },
        |id| retired.push(id),
    );
    assert_eq!(retired, vec![[2; 16], [3; 16]]);
    assert_eq!(bank.endpoints[0].render_pcm.as_ptr(), render_address);
    assert_eq!(bank.endpoints[0].capture_pcm.as_ptr(), capture_address);
    assert!(
        bank.endpoints[0].render_pcm[..64]
            .iter()
            .all(|value| value.to_bits() == 0.25_f32.to_bits())
    );
    assert!(
        bank.endpoints[0].capture_pcm[..64]
            .iter()
            .all(|value| value.to_bits() == (-0.75_f32).to_bits())
    );
    assert_eq!(bank.endpoints[1].format.device_rate, 44_100);
}

struct DeviceState {
    running: bool,
    active: bool,
}
impl CaptureTransport for DeviceState {
    fn clock(&mut self) -> Option<super::super::NativeClockSnapshot> {
        Some(super::super::NativeClockSnapshot {
            epoch: 1,
            sample_rate: 48_000,
            running: u32::from(self.running),
            capture_through: if self.active { 1024 } else { 0 },
            ..Default::default()
        })
    }
    fn write(&mut self, _: u64, _: &[f32], _: u32) -> bool {
        true
    }
    fn invalidate_capture(&mut self) -> bool {
        true
    }
}
impl RenderTransport for DeviceState {
    fn render_snapshot(&mut self) -> Option<super::super::callback_source::RenderSnapshot> {
        Some(super::super::callback_source::RenderSnapshot {
            clock: self.clock().unwrap(),
            stream_epoch: 1,
            channels: 2,
            through: if self.active { 1024 } else { 0 },
            activity: if self.active {
                InputActivity::Active
            } else {
                InputActivity::Inactive
            },
        })
    }
}

#[test]
fn only_referenced_active_directions_participate_in_clock_health() {
    for source in [false, true] {
        let mut endpoint = EndpointBoundary::new(
            BoundaryConfig {
                identity: "01010101-0101-0101-0101-010101010101".into(),
                id: [1; 16],
                audio: NativeAudioConfig {
                    capacity_frames: 16_384,
                    render_channels: 2,
                    capture_channels: 2,
                },
                graph_rate: 48_000,
                device_rate: 48_000,
                directions: Directions {
                    source,
                    sink: !source,
                },
            },
            DeviceState {
                running: false,
                active: false,
            },
        )
        .unwrap();
        endpoint.begin(32);
        assert!(
            endpoint.synchronized(),
            "a stopped device is not a drifting branch"
        );
        endpoint.transport.running = true;
        endpoint.begin(32);
        assert!(
            endpoint.synchronized(),
            "another device direction may be running without this client"
        );
        endpoint.transport.active = true;
        endpoint.begin(32);
        assert!(
            !endpoint.synchronized(),
            "a referenced active converter has not primed yet"
        );
        assert_eq!(endpoint.source.is_some(), source);
        assert_eq!(endpoint.sink.is_some(), !source);
    }
}

#[test]
fn pending_swap_returns_old_pcm_ownership_for_release_after_callback_resume() {
    let create = |rate| {
        EndpointBoundary::new(
            BoundaryConfig {
                identity: "01010101-0101-0101-0101-010101010101".into(),
                id: [1; 16],
                audio: NativeAudioConfig {
                    capacity_frames: 16_384,
                    render_channels: 0,
                    capture_channels: 2,
                },
                graph_rate: 48_000,
                device_rate: rate,
                directions: Directions {
                    source: false,
                    sink: true,
                },
            },
            NativeCapture::new([1; 16]),
        )
        .unwrap()
    };
    let mut original = create(0);
    original.capture_pcm[..64].fill(0.75);
    let original_address = original.capture_pcm.as_ptr();
    let mut replacement = create(48_000);
    replacement.capture_pcm[..64].fill(-0.25);
    let replacement_address = replacement.capture_pcm.as_ptr();
    let mut bank = EndpointBoundaries {
        endpoints: vec![original],
    };
    let retired = bank.replace_pending(EndpointBoundaries {
        endpoints: vec![replacement],
    });
    assert_eq!(bank.endpoints[0].capture_pcm.as_ptr(), replacement_address);
    assert_eq!(retired.endpoints[0].capture_pcm.as_ptr(), original_address);
    assert!(
        retired.endpoints[0].capture_pcm[..64]
            .iter()
            .all(|value| value.to_bits() == 0.75_f32.to_bits())
    );
    drop(retired);
    assert!(
        bank.endpoints[0].capture_pcm[..64]
            .iter()
            .all(|value| value.to_bits() == (-0.25_f32).to_bits())
    );
}

fn plan() -> RenderPlan {
    let mut document: GraphDocument = serde_json::from_value(serde_json::json!({
        "schemaVersion":2,"revision":1,"nodes":[
            {"id":"graph-a::node","type":"virtual_endpoint","config":{
                "id":"01010101-0101-0101-0101-010101010101","displayName":"One",
                "reference":{"render":true,"capture":true},
                "render":{"channels":2,"channelLayout":"stereo"},
                "capture":{"channels":2,"channelLayout":"stereo"},
                "supportedSampleRates":[48000],"defaultSampleRate":48000}},
            {"id":"graph-b::node","type":"virtual_endpoint","config":{
                "id":"02020202-0202-0202-0202-020202020202","displayName":"Two",
                "reference":{"render":true,"capture":true},
                "render":{"channels":2,"channelLayout":"stereo"},
                "capture":{"channels":2,"channelLayout":"stereo"},
                "supportedSampleRates":[44100],"defaultSampleRate":44100}}
        ],"edges":[]
    }))
    .unwrap();
    // Native boundary fixtures exercise transport ownership, not authored bus composition.
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
                    sample_rate: 48000,
                    block_frames: 32,
                },
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    RenderPlan::prepare(&execution).unwrap()
}

#[test]
fn canonical_bindings_select_separate_pcm_and_duplicate_reads_do_not_advance_source() {
    let plan = plan();
    let endpoints = plan
        .source_bindings()
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            let identity = binding.virtual_endpoint_id().unwrap().clone();
            EndpointBoundary::new(
                BoundaryConfig {
                    id: parse_endpoint_id(identity.as_str()).unwrap(),
                    identity,
                    audio: NativeAudioConfig {
                        capacity_frames: 16_384,
                        render_channels: 2,
                        capture_channels: 2,
                    },
                    graph_rate: 48000,
                    device_rate: 0,
                    directions: Directions {
                        source: true,
                        sink: true,
                    },
                },
                Disconnected,
            )
            .map(|mut endpoint| {
                endpoint.prepare_block(32);
                endpoint.render_pcm[..64].fill(if index == 0 { 0.25 } else { -0.75 });
                endpoint.source_result = TransferOutcome::complete(32);
                endpoint
            })
            .unwrap()
        })
        .collect();
    let mut bank = EndpointBoundaries { endpoints };
    for (index, binding) in plan.source_bindings().iter().enumerate() {
        let mut first = [0.0; 64];
        let mut again = [0.0; 64];
        assert_eq!(bank.read(binding, &mut first).unwrap().frames, 32);
        assert_eq!(bank.read(binding, &mut again).unwrap().frames, 32);
        assert_eq!(first.map(f32::to_bits), again.map(f32::to_bits));
        let expected: f32 = if index == 0 { 0.25 } else { -0.75 };
        assert!(
            first
                .iter()
                .all(|sample| sample.to_bits() == expected.to_bits())
        );
    }
    let sink = &plan.sink_bindings()[0];
    bank.write(sink, &[0.25; 64]);
    bank.write(sink, &[0.5; 64]);
    assert!(
        bank.endpoints[0].capture_pcm[..64]
            .iter()
            .all(|sample| sample.to_bits() == 0.75_f32.to_bits())
    );
    assert!(
        bank.endpoints[1].capture_pcm[..64]
            .iter()
            .all(|sample| sample.to_bits() == 0.0_f32.to_bits())
    );
}
