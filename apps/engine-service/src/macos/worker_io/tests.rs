use super::*;
use crate::macos::{MacTimeline, NativePacer, RouteClock, adapter_profile, capture_schedule};
use mixrelay_audio_graph::RenderFormat;
const DEVICE_ID: &str = "2f2f2f2f-2f2f-2f2f-2f2f-2f2f2f2f2f2f";
use mixrelay_engine_service::{
    RuntimeStatusBackend,
    endpoint_registry::{CallbackEndpoint, EndpointRegistry, EndpointSpec},
    endpoint_service::AudioConfig,
    session::AdapterRuntimeStatus,
};

fn primary() -> PrimaryEndpoint {
    PrimaryEndpoint {
        identity: DEVICE_ID.into(),
        id: [47; 16],
        directions: Directions {
            source: true,
            sink: true,
        },
        config: AudioConfig {
            capacity_frames: 1024,
            render_channels: 2,
            capture_channels: 2,
        },
    }
}

fn secondary(rate: u32) -> EndpointBoundaries<NativeCapture> {
    use crate::macos::endpoint_boundaries::{BoundaryConfig, EndpointBoundary};
    EndpointBoundaries {
        endpoints: vec![
            EndpointBoundary::new(
                BoundaryConfig {
                    identity: "02020202-0202-0202-0202-020202020202".into(),
                    id: [2; 16],
                    audio: AudioConfig {
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
                NativeCapture::new([2; 16]),
            )
            .unwrap(),
        ],
    }
}

#[test]
fn changing_only_secondary_preserves_primary_epoch_pcm_and_partial_callback_cursor() {
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    io.independent = secondary(0);
    let snapshot = demand(&io.transport, 64, 64);
    let first = io.schedule.reserve(&snapshot, 32, 1024).unwrap();
    assert_eq!(first, 64);
    assert!(io.transport.write(&snapshot, first, &[0.25; 64], 32));
    let render_address = io.render.as_ptr();
    let capture_address = io.capture.as_ptr();
    let mut retired = Vec::new();
    assert!(!io.configure(
        WorkerConfiguration {
            primary: Some(primary()),
            independent: secondary(48_000)
        },
        false,
        |id| retired.push(id),
    ));
    assert_eq!(retired, vec![[2; 16]]);
    assert_eq!(io.render.as_ptr(), render_address);
    assert_eq!(io.capture.as_ptr(), capture_address);
    let next = io.schedule.reserve(&snapshot, 32, 1024).unwrap();
    assert_eq!(
        next, 96,
        "the existing partial callback must not replay frame 64"
    );
    assert!(io.transport.write(&snapshot, next, &[-0.5; 64], 32));
    assert!(io.schedule.reserve(&snapshot, 32, 1024).is_none());
    let mut pcm = [0.0; 128];
    // SAFETY: the fixture pin retains this live stereo mapping; one worker
    // owns its producer and the reader slice contains exactly 64 stereo frames.
    unsafe {
        assert_eq!(
            mr_shared_audio_buffer_stats(io.transport.endpoint.capture_buffer()).epoch,
            snapshot.capture_epoch
        );
        assert_eq!(
            mr_shared_audio_buffer_read(
                io.transport.endpoint.capture_buffer(),
                64,
                pcm.as_mut_ptr(),
                64
            ),
            64
        );
    }
    assert!(
        pcm[..64]
            .iter()
            .all(|value| value.to_bits() == 0.25_f32.to_bits())
    );
    assert!(
        pcm[64..]
            .iter()
            .all(|value| value.to_bits() == (-0.5_f32).to_bits())
    );
}

#[test]
fn primary_direction_or_graph_rate_change_retires_only_its_old_writer() {
    for rate_changed in [false, true] {
        let registry = EndpointRegistry::new();
        let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
        io.independent = secondary(48_000);
        let mut next_primary = primary();
        if !rate_changed {
            next_primary.directions.sink = false;
        }
        let mut retired = Vec::new();
        assert!(io.configure(
            WorkerConfiguration {
                primary: Some(next_primary),
                independent: secondary(48_000)
            },
            rate_changed,
            |id| retired.push(id),
        ));
        assert_eq!(retired, vec![[47; 16]]);
    }
}

fn processor_for_reference(format: RenderFormat, capture: bool) -> Box<BlockProcessor> {
    use mixrelay_audio_graph::{CompileContext, CompileLimits, GraphCompiler};
    let mut document = mixrelay_engine_service::default_session_document(1, format);
    for node in &mut document.nodes {
        if let mixrelay_audio_graph::NodeConfig::VirtualEndpoint(endpoint) = &mut node.config {
            endpoint.id = DEVICE_ID.into();
            let mut spec = endpoint.spec.clone();
            if !capture {
                spec.capture = None;
            }
            node.config = mixrelay_audio_graph::NodeConfig::VirtualEndpointTransport(spec);
        }
    }
    if !capture {
        document.edges.clear();
    }
    let plan = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    Box::new(BlockProcessor::new(std::sync::Arc::new(
        mixrelay_audio_engine::RenderPlan::prepare(&plan).unwrap(),
    )))
}
use mixrelay_macos_transport::{
    endpoint_transport::{
        mr_endpoint_transport_begin_clock_transition, mr_endpoint_transport_clock_demand,
        mr_endpoint_transport_clock_snapshot, mr_endpoint_transport_clock_state,
        mr_endpoint_transport_incarnation,
    },
    shared_audio_buffer::{
        mr_shared_audio_buffer_read, mr_shared_audio_buffer_stats,
        mr_shared_audio_buffer_try_begin_epoch, mr_shared_audio_buffer_write,
    },
};

// Only the service discovery boundary is substituted. PCM storage, native
// credits, scheduling and BlockProcessor are the actual production code.
struct LocalTransport<'a> {
    endpoint: CallbackEndpoint<'a>,
    reads: usize,
    writes: usize,
    prepares: usize,
    invalidations: usize,
    reject_prepare: bool,
    reject_write: bool,
}
impl Transport for LocalTransport<'_> {
    fn invalidate_capture(&mut self, _: &EndpointSnapshot) -> bool {
        self.invalidations += 1;
        // SAFETY: this fixture is the sole writer of its pinned capture mapping.
        unsafe { mr_shared_audio_buffer_try_begin_epoch(self.endpoint.capture_buffer()) != 0 }
    }
    fn prepare_capture(&mut self, endpoint: &EndpointSnapshot) -> Option<EndpointSnapshot> {
        self.prepares += 1;
        (!self.reject_prepare).then_some(*endpoint)
    }
    fn read(&mut self, _: &EndpointSnapshot, frame: u64, pcm: &mut [f32], frames: u32) -> u32 {
        self.reads += 1;
        assert_eq!(pcm.len(), frames as usize * 2);
        // SAFETY: the registry pin retains the initialized stereo mapping; the
        // fixture is its sole PCM writer and this slice has exactly frames*2 cells.
        unsafe {
            mr_shared_audio_buffer_read(
                self.endpoint.render_buffer(),
                frame,
                pcm.as_mut_ptr(),
                frames,
            )
        }
    }
    fn write(&mut self, endpoint: &EndpointSnapshot, frame: u64, pcm: &[f32], frames: u32) -> bool {
        self.writes += 1;
        assert_eq!(pcm.len(), frames as usize * 2);
        // SAFETY: this mutable fixture exclusively publishes into the pinned
        // capture mapping using its observed epoch and a stereo slice of exact size.
        !self.reject_write
            && unsafe {
                mr_shared_audio_buffer_write(
                    self.endpoint.capture_buffer(),
                    endpoint.capture_epoch,
                    frame,
                    pcm.as_ptr(),
                    frames,
                )
            }
    }
}

fn endpoint(registry: &EndpointRegistry) -> LocalTransport<'_> {
    let id = [47; 16];
    registry
        .prepare_inventory(&[EndpointSpec {
            id,
            display_name: "Worker PCM test".into(),
            sample_rate: 48_000,
            capacity_frames: 1024,
            render_channels: 2,
            capture_channels: 2,
        }])
        .unwrap()
        .commit()
        .unwrap();
    let endpoint = registry.callback(id).unwrap();
    // SAFETY: the newly created test mapping has no external callbacks and is
    // retained by this pin until after the worker and all PCM accesses finish.
    unsafe {
        assert!(mr_endpoint_transport_begin_clock_transition(
            endpoint.transport()
        ));
        mr_endpoint_transport_clock_state(endpoint.transport(), 1, 48_000, true);
    }
    LocalTransport {
        endpoint,
        reads: 0,
        writes: 0,
        prepares: 0,
        invalidations: 0,
        reject_prepare: false,
        reject_write: false,
    }
}

fn demand(transport: &LocalTransport<'_>, through: u64, quantum: u32) -> EndpointSnapshot {
    let endpoint = &transport.endpoint;
    let mut snapshot = EndpointSnapshot {
        endpoint_id: [47; 16],
        ..EndpointSnapshot::default()
    };
    // SAFETY: exclusive fixture publication into the live pinned mapping. Both
    // callbacks publish their quantum, independently of the observation gap.
    unsafe {
        mr_endpoint_transport_clock_demand(endpoint.transport(), true, through, quantum);
        mr_endpoint_transport_clock_demand(endpoint.transport(), false, through, quantum);
        assert!(mr_endpoint_transport_clock_snapshot(
            endpoint.transport(),
            &raw mut snapshot.clock
        ));
        assert!(mr_endpoint_transport_incarnation(
            endpoint.transport(),
            snapshot.incarnation.as_mut_ptr()
        ));
        snapshot.render_epoch = mr_shared_audio_buffer_stats(endpoint.render_buffer()).epoch;
        snapshot.capture_epoch = mr_shared_audio_buffer_stats(endpoint.capture_buffer()).epoch;
        let pcm: Vec<f32> = (through - u64::from(quantum)..through)
            .flat_map(|frame| [f32::from(u16::try_from(frame).unwrap()) / 1024.0, -0.25])
            .collect();
        assert!(mr_shared_audio_buffer_write(
            endpoint.render_buffer(),
            snapshot.render_epoch,
            through - u64::from(quantum),
            pcm.as_ptr(),
            quantum
        ));
    }
    snapshot.render_channels = 2;
    snapshot.capture_channels = 2;
    snapshot.render_activity = 3;
    snapshot.published_through = through;
    snapshot
}

#[test]
fn coalesced_native_callbacks_deliver_the_latest_pcm_through_dsp() {
    assert_coalesced_pcm(32, 128, &[32, 96, 192]);
}

#[test]
fn primary_off_is_applied_even_when_capture_admission_is_blocked() {
    use mixrelay_audio_engine::{ProcessorExchange, RuntimeControlUpdate};
    use mixrelay_audio_graph::NodeControl;
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 32,
    };
    let mut processor = processor_for_reference(format, true);
    let exchange = ProcessorExchange::new(1);
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    let mut timeline = MacTimeline::new();
    for (revision, enabled) in [(2, true), (3, false), (4, true)] {
        exchange
            .publish_controls(
                processor.plan().controls(),
                revision,
                &[RuntimeControlUpdate {
                    node_id: "default-virtual-io".into(),
                    control: NodeControl {
                        enabled,
                        muted: false,
                    },
                    gain_db: None,
                }],
            )
            .unwrap();
        io.transport.reject_prepare = !enabled;
        let snapshot = demand(&io.transport, (revision - 1) * 32, 32);
        let before = (
            io.transport.prepares,
            io.transport.reads,
            io.transport.writes,
        );
        assert!(
            io.process(
                &mut processor,
                WorkerCycle {
                    timeline: &timeline,
                    frames: 32,
                    discontinuity: false,
                    write_hal: true,
                    devices: &devices,
                    status: &status,
                    endpoint: Some(snapshot),
                }
            )
            .unwrap()
        );
        let count = usize::from(enabled);
        assert_eq!(io.transport.prepares - before.0, count);
        assert_eq!(io.transport.reads - before.1, count);
        assert_eq!(io.transport.writes - before.2, count);
        assert_eq!(processor.plan().controls().applied_revision(), revision);
        timeline.frame += 32;
    }
    assert_eq!(io.transport.invalidations, 1);
}

#[test]
fn closing_capture_client_does_not_reject_continuing_playback() {
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 32,
    };
    let mut processor = processor_for_reference(format, true);
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    let mut timeline = MacTimeline::new();
    for callback in 1..=1000 {
        let mut snapshot = demand(&io.transport, callback * 32, 32);
        // Capture stops after one callback, while playback keeps supplying PCM.
        // Its last published stamp stays valid; it is not a new capture request.
        snapshot.clock.capture_through = 32;
        assert!(
            io.process(
                &mut processor,
                WorkerCycle {
                    timeline: &timeline,
                    frames: 32,
                    discontinuity: false,
                    write_hal: true,
                    devices: &devices,
                    status: &status,
                    endpoint: Some(snapshot),
                }
            )
            .unwrap(),
            "capture backpressure must not stop playback DSP"
        );
        timeline.frame += 32;
    }
    assert_eq!((io.transport.reads, io.transport.writes), (1000, 1));
    assert_eq!(status.status(format).overruns, 0);
    let snapshot = demand(&io.transport, 32_032, 32);
    assert!(
        io.process(
            &mut processor,
            WorkerCycle {
                timeline: &timeline,
                frames: 32,
                discontinuity: false,
                write_hal: true,
                devices: &devices,
                status: &status,
                endpoint: Some(snapshot),
            }
        )
        .unwrap()
    );
    assert_eq!(
        io.transport.writes, 2,
        "new capture demand resumes without reset"
    );
}

#[test]
fn render_only_reference_never_publishes_capture_even_with_an_active_capture_client() {
    let registry = EndpointRegistry::new();
    let mut selected = primary();
    selected.directions.sink = false;
    let mut io = WorkerIo::new(endpoint(&registry), Some(selected));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 32,
    };
    let mut processor = processor_for_reference(format, false);
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    let timeline = MacTimeline::new();
    let snapshot = demand(&io.transport, 32, 32);
    assert_eq!(snapshot.clock.capture_frames, 32);
    assert!(
        io.process(
            &mut processor,
            WorkerCycle {
                timeline: &timeline,
                frames: 32,
                discontinuity: true,
                write_hal: true,
                devices: &devices,
                status: &status,
                endpoint: Some(snapshot),
            }
        )
        .unwrap()
    );
    assert_eq!((io.transport.reads, io.transport.writes), (1, 0));
    assert!(io.capture.is_empty());
    assert_eq!(status.status(format).overruns, 0);
}

#[test]
fn coalesced_large_callbacks_deliver_the_entire_latest_window_in_small_dsp_blocks() {
    assert_coalesced_pcm(256, 32, &[256, 768, 1536]);
}

fn assert_coalesced_pcm(quantum: u32, block_frames: u32, throughs: &[u64]) {
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames,
    };
    let mut processor = processor_for_reference(format, true);
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    let route = RouteClock {
        maximum_frames: block_frames,
        has_hal: true,
        synchronized: true,
        independent_idle: false,
    };
    let mut timeline = MacTimeline::new();
    let mut pacer = NativePacer::default();
    for &through in throughs {
        let snapshot = demand(&io.transport, through, quantum);
        loop {
            let pace =
                capture_schedule::pace(&mut pacer, Some(&snapshot), 48_000, block_frames, 1024);
            if pace == crate::macos::Pace::Wait {
                break;
            }
            let (frames, discontinuity) = timeline
                .accept((pace, 1), &route, &status, Some(&snapshot))
                .unwrap();
            assert_eq!(frames, quantum.min(block_frames));
            assert!(
                io.process(
                    &mut processor,
                    WorkerCycle {
                        timeline: &timeline,
                        frames,
                        discontinuity,
                        write_hal: true,
                        devices: &devices,
                        status: &status,
                        endpoint: Some(snapshot),
                    }
                )
                .unwrap()
            );
        }
        let mut pcm = vec![0.0; quantum as usize * 2];
        // SAFETY: sole reader, pinned live stereo mapping and exactly quantum*2 output cells.
        assert_eq!(
            unsafe {
                mr_shared_audio_buffer_read(
                    io.transport.endpoint.capture_buffer(),
                    through,
                    pcm.as_mut_ptr(),
                    quantum,
                )
            },
            quantum
        );
        let expected: Vec<f32> = (through - u64::from(quantum)..through)
            .flat_map(|frame| [f32::from(u16::try_from(frame).unwrap()) / 1024.0, -0.25])
            .collect();
        assert_eq!(pcm, expected);
    }
    let transfers = throughs.len() * quantum.div_ceil(block_frames) as usize;
    assert_eq!(
        (io.transport.reads, io.transport.writes),
        (transfers, transfers)
    );
    assert_eq!(
        status.status(format).overruns,
        2,
        "expired windows are explicit"
    );
}

#[test]
fn refused_admission_does_not_consume_pcm_and_failed_publication_is_reported() {
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };
    let mut processor = processor_for_reference(format, true);
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    let mut timeline = MacTimeline::new();
    timeline.clock_id = 1;
    timeline.epoch = 1;
    let snapshot = demand(&io.transport, 32, 32);
    let cycle = WorkerCycle {
        timeline: &timeline,
        frames: 32,
        discontinuity: true,
        write_hal: true,
        devices: &devices,
        status: &status,
        endpoint: Some(snapshot),
    };
    io.transport.reject_prepare = true;
    status.observe_device_rate(Some(48_000));
    status.observe_clock(mixrelay_audio_io::session::ClockAuthority::Device, true);
    assert!(!io.process(&mut processor, cycle).unwrap());
    assert_eq!((io.transport.reads, io.transport.writes), (0, 0));
    assert_eq!(
        status.status(format).audio_session.unwrap().issue,
        Some(mixrelay_audio_io::session::SessionIssue::ClockNotSynchronized)
    );
    io.transport.reject_prepare = false;
    io.transport.reject_write = true;
    assert!(io.process(&mut processor, cycle).unwrap());
    assert_eq!((io.transport.reads, io.transport.writes), (1, 1));
    assert_eq!(status.status(format).overruns, 2);
}

#[test]
fn rejected_hal_admission_does_not_stall_a_mixed_route_or_claim_sync() {
    use mixrelay_audio_engine::RenderPlan;
    use mixrelay_audio_graph::{CompileContext, CompileLimits, GraphCompiler, GraphDocument};
    use std::sync::Arc;
    let registry = EndpointRegistry::new();
    let mut io = WorkerIo::new(endpoint(&registry), Some(primary()));
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };
    let mut document: GraphDocument = serde_json::from_value(serde_json::json!({
        "schemaVersion":2,"revision":1,"nodes":[
            {"id":"default-virtual-io","type":"virtual_endpoint","config":{
                "id":DEVICE_ID,"displayName":"Test",
                "reference":{"render":true,"capture":true},
                "render":{"channels":2,"channelLayout":"stereo"},
                "capture":{"channels":2,"channelLayout":"stereo"},
                "supportedSampleRates":[48000],"defaultSampleRate":48000}},
            {"id":"output","type":"audio_device_output","config":{"selector":{"match":"default_output"},"inputChannels":2}}
        ],"edges":[
            {"id":"hal","source":{"nodeId":"default-virtual-io","portId":mixrelay_audio_graph::FROM_OS_PORT_ID},
                "destination":{"nodeId":"default-virtual-io","portId":mixrelay_audio_graph::TO_OS_PORT_ID}},
            {"id":"hardware","source":{"nodeId":"default-virtual-io","portId":mixrelay_audio_graph::FROM_OS_PORT_ID},
                "destination":{"nodeId":"output","portId":"audio"}}
        ]
    })).unwrap();
    // This test exercises raw HAL transport admission, before device-bus composition.
    for node in &mut document.nodes {
        if let mixrelay_audio_graph::NodeConfig::VirtualEndpoint(endpoint) = &node.config {
            node.config =
                mixrelay_audio_graph::NodeConfig::VirtualEndpointTransport(endpoint.spec.clone());
        }
    }
    let plan = GraphCompiler::default()
        .compile(
            &document,
            CompileContext {
                format,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    let mut processor = BlockProcessor::new(Arc::new(RenderPlan::prepare(&plan).unwrap()));
    let devices = crate::macos_devices::DeviceSet::default();
    let status = AdapterRuntimeStatus::new(adapter_profile());
    status.observe_device_rate(Some(48_000));
    status.observe_clock(mixrelay_audio_io::session::ClockAuthority::Device, true);
    let timeline = MacTimeline::new();
    let snapshot = demand(&io.transport, 32, 32);
    io.transport.reject_prepare = true;
    assert!(
        io.process(
            &mut processor,
            WorkerCycle {
                timeline: &timeline,
                frames: 32,
                discontinuity: true,
                write_hal: true,
                devices: &devices,
                status: &status,
                endpoint: Some(snapshot),
            }
        )
        .unwrap()
    );
    assert_eq!((io.transport.reads, io.transport.writes), (1, 0));
    assert_eq!(
        status.status(format).audio_session.unwrap().issue,
        Some(mixrelay_audio_io::session::SessionIssue::ClockNotSynchronized)
    );
}
