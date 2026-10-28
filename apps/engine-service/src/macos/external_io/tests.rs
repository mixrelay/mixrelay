use super::*;
use mixrelay_audio_engine::RenderPlan;
use mixrelay_audio_graph::{
    CompileContext, CompileLimits, GraphCompiler, GraphDocument, NodeConfig, RenderFormat,
};
use mixrelay_macos_transport::{
    application_source_transport::SourceTransport,
    source_control::SourceRequest,
    source_registry::{SchedulingClock, SourceRegistry},
};
use std::time::Instant;

struct Clock(Instant);
impl SchedulingClock for Clock {
    fn now(&self) -> Instant {
        self.0
    }
}

fn unrouted_virtual_plan() -> RenderPlan {
    qualification_plan(false)
}

/// REC-02's graph: one virtual input carried to a recorder, or the input alone.
fn qualification_plan(routed: bool) -> RenderPlan {
    let mut document: GraphDocument = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/audio-graph/src/fixtures/qualification-stereo.json"
    )))
    .unwrap();
    let NodeConfig::VirtualEndpoint(endpoint) = &document.nodes[0].config else {
        panic!("virtual source fixture")
    };
    document.nodes[0].config = NodeConfig::VirtualEndpointTransport(endpoint.spec.clone());
    if !routed {
        document.nodes.truncate(1);
        document.edges.clear();
    }
    let execution = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format: RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 128,
                },
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    RenderPlan::prepare(&execution).unwrap()
}

#[test]
fn unrouted_virtual_input_negotiates_format_and_stopped_input_cannot_drain_old_pcm() {
    let plan = unrouted_virtual_plan();
    let binding = &plan.source_bindings()[0];
    assert!(!binding.is_routed_source());
    let registry = SourceRegistry::with_clock(Box::new(Clock(Instant::now())));
    let mut request = SourceRequest {
        capacity_frames: 16_384,
        channels: 2,
        transport_version: mixrelay_macos_transport::application_clock::VERSION,
        ..SourceRequest::default()
    };
    let id = format!(
        "virtual:{}",
        binding.virtual_endpoint_id().unwrap().as_str()
    );
    for (target, byte) in request.source_id.iter_mut().zip(id.bytes()) {
        *target = std::ffi::c_char::from_ne_bytes([byte]);
    }
    let mut producer = SourceTransport::create(&registry, &request).unwrap();
    let cycle = ProcessCycle {
        clock: mixrelay_audio_io::process::ClockDomain {
            authority: mixrelay_audio_io::session::ClockAuthority::EngineTimer,
            id: 0,
            epoch: 1,
        },
        sample_rate: 48_000,
        frames: 128,
        first_frame: 0,
        monotonic_ns: None,
        discontinuity: false,
    };
    let mut pcm = [9.0; 256];
    assert_eq!(
        read_bound_source(binding, &mut pcm, &cycle, Some(&registry), None).frames,
        0
    );
    let format = producer.clock().unwrap().format_word;
    assert_ne!(
        format, 0,
        "unrouted meters must request the producer format"
    );
    assert!(producer.reset(format, 48_000));
    assert!(producer.write_pcm(&[0.5; 2048], 1024, None));
    producer.publish_native(1024, true);
    assert_eq!(
        read_bound_source(binding, &mut pcm, &cycle, Some(&registry), None).frames,
        128
    );
    assert_eq!(pcm.map(f32::to_bits), [0.5_f32.to_bits(); 256]);
    producer.publish_native(1024, false);
    let outcome = read_bound_source(binding, &mut pcm, &cycle, Some(&registry), None);
    assert_eq!(outcome.frames, 0);
    assert_eq!(
        outcome.activity,
        mixrelay_audio_io::process::InputActivity::Inactive
    );
    assert_eq!(pcm.map(f32::to_bits), [0.0_f32.to_bits(); 256]);
}

#[test]
fn a_plan_routes_its_recorded_virtual_input_as_the_head_consumer_only_without_devices() {
    let routed = qualification_plan(true);
    let token = binding_token(&routed.source_bindings()[0]);
    let published = |plan: &RenderPlan, enabled: bool, device_clocked: bool| -> Vec<_> {
        plan_routes(
            plan.source_bindings()
                .iter()
                .map(|binding| (binding, enabled)),
            device_clocked,
        )
        .collect()
    };
    assert_eq!(
        published(&routed, true, false),
        [(token, ConsumerRoute::Routed { head: true })]
    );
    // A device clock paces the graph, so the source joins at its live edge.
    assert_eq!(
        published(&routed, true, true),
        [(token, ConsumerRoute::Routed { head: false })]
    );
    // An off node and a metered-only input are not routed.
    assert_eq!(
        published(&routed, false, false),
        [(token, ConsumerRoute::Unrouted)]
    );
    assert_eq!(
        published(&unrouted_virtual_plan(), true, false),
        [(token, ConsumerRoute::Unrouted)]
    );
}
