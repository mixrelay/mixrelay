//! Exercise the production Windows graph worker with owned, faultable memory.
//! These tests do not install, disable, or mutate a kernel device. Real PCM
//! qualification remains a separate interactive LAB test.
use super::*;
use mixrelay_audio_graph::GraphDocument;
use mixrelay_audio_io::{process::ProcessingState, session::SessionIssue};
use mixrelay_engine_service::{RuntimeStatusBackend, windows_transport_contract::LAB_LAYOUT};

#[test]
fn diagnostic_transport_does_not_advertise_product_device_configuration() {
    let backend = WindowsVirtualEndpointBackend::default();
    let settings = backend.settings().unwrap();
    assert!(!settings.can_create);
    assert!(!settings.can_rename);
    assert_eq!(settings.render_channels, [2]);
    assert_eq!(settings.capture_channels, [1]);
    assert!(settings.creation_template.is_none());
    assert!(settings.devices.is_empty());
    assert_eq!(backend.capabilities().max_render_channels, 2);
}

#[test]
fn primary_native_clock_does_not_clear_independent_clock_warning() {
    let resources = WindowsGraphResources::default();
    let _lease = resources.runtime_status.begin_processing().unwrap();
    resources.device_clock_active.store(true, Ordering::Release);
    let session = resources.status(render_format()).audio_session.unwrap();
    assert_eq!(
        session.clock_authority,
        mixrelay_audio_io::session::ClockAuthority::Device
    );
    assert_eq!(session.issue, Some(SessionIssue::ClockNotSynchronized));
}

#[test]
fn prepared_rate_change_activates_in_the_running_graph_worker() {
    let fixture = WorkerFixture::new();
    let mut current = render_format();
    for (revision, sample_rate) in [(2, 44_100), (3, 96_000), (4, 48_000)] {
        let requested = RenderFormat {
            sample_rate,
            block_frames: 128,
        };
        let document = GraphDocument::new(revision, vec![], vec![]);
        let prepared =
            format_control::prepare(&fixture.resources, &document, current, requested).unwrap();
        let execution = GraphCompiler::default()
            .compile(
                &document,
                CompileContext {
                    format: requested,
                    limits: CompileLimits::default(),
                },
            )
            .unwrap();
        let processor = Box::new(BlockProcessor::with_peak_meters(
            Arc::new(RenderPlan::prepare(&execution).unwrap()),
            fixture.meters.clone(),
        ));
        fixture.resources.reconfigure(current, requested).unwrap();
        prepared.commit();
        fixture.exchange.submit(processor).unwrap();
        fixture.resources.resume();
        fixture.wait_for(|| {
            fixture.exchange.revisions().active == revision
                && fixture
                    .resources
                    .status(requested)
                    .audio_session
                    .is_some_and(|session| session.format.sample_rate == sample_rate)
        });
        let session = fixture.resources.status(requested).audio_session.unwrap();
        assert_eq!(session.format.sample_rate, sample_rate);
        assert_eq!(
            fixture.resources.active_spec.lock().unwrap().graph_rate,
            sample_rate
        );
        current = requested;
    }
}

struct WorkerFixture {
    resources: Arc<WindowsGraphResources>,
    exchange: Arc<ProcessorExchange>,
    meters: Arc<PeakMeterBank>,
    _worker: mixrelay_engine_service::session::AudioWorker,
}

impl WorkerFixture {
    fn new() -> Self {
        let resources = Arc::new(WindowsGraphResources::default());
        let exchange = Arc::new(ProcessorExchange::new(1));
        let meters = default_peak_meter_bank();
        let processor = create_initial_processor(meters.clone()).unwrap();
        let (worker_resources, worker_exchange) = (resources.clone(), exchange.clone());
        let thread =
            thread::spawn(move || run_audio_worker(processor, worker_exchange, worker_resources));
        let worker = mixrelay_engine_service::session::AudioWorker::new(
            resources.runtime_status.clone(),
            thread,
        );
        Self {
            resources,
            exchange,
            meters,
            _worker: worker,
        }
    }

    fn submit(&self, revision: u64, resources: Option<WindowsCaptureSet>) {
        if let Some(mut resources) = resources {
            resources.revision = revision;
            self.resources.publish(resources).unwrap();
        }
        let execution = GraphCompiler::default()
            .compile(
                &GraphDocument::new(revision, vec![], vec![]),
                CompileContext {
                    format: render_format(),
                    limits: CompileLimits::default(),
                },
            )
            .unwrap();
        self.exchange
            .submit(Box::new(BlockProcessor::with_peak_meters(
                Arc::new(RenderPlan::prepare(&execution).unwrap()),
                self.meters.clone(),
            )))
            .unwrap();
        self.wait_for(|| self.exchange.revisions().active == revision);
    }

    fn wait_for(&self, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            self.exchange.reclaim_retired();
            self.resources.reclaim_retired();
            if condition() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "worker stopped accepting configuration or failed to release resources"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn mutate(&self, io: &Mutex<transport::Transport>, mutation: impl Fn(&transport::Transport)) {
        self.wait_for(|| {
            let Ok(io) = io.try_lock() else {
                return false;
            };
            mutation(&io);
            true
        });
    }
}

#[test]
fn offline_and_corrupt_mappings_allow_edits_removal_and_fresh_attachment() {
    let fixture = WorkerFixture::new();
    let mut revision = 1;
    for cycle in 0..12 {
        eprintln!("recovery fixture cycle={cycle}: attach");
        let io = Arc::new(Mutex::new(transport::Transport::test_fixture(LAB_LAYOUT)));
        let weak = Arc::downgrade(&io);
        *fixture.resources.virtual_cache.lock().unwrap() = weak.clone();
        revision += 1;
        fixture.submit(
            revision,
            Some(WindowsCaptureSet {
                virtual_io: Some(io.clone()),
                endpoint_node: Some("fixture".into()),
                ..WindowsCaptureSet::default()
            }),
        );
        fixture.wait_for(|| {
            fixture
                .resources
                .virtual_clock_active
                .load(Ordering::Acquire)
        });
        assert!(!fixture.resources.virtual_failed.load(Ordering::Acquire));
        // No producer frames: waiting for native demand must not be a failure.
        assert_eq!(
            fixture
                .resources
                .status(render_format())
                .audio_session
                .unwrap()
                .processing_state,
            ProcessingState::Running
        );
        fixture.mutate(&io, |io| {
            if cycle % 2 == 0 {
                io.test_set_online(false);
            } else {
                io.test_set_produced(LAB_LAYOUT.capacity() + 1);
            }
        });
        fixture.wait_for(|| fixture.resources.virtual_failed.load(Ordering::Acquire));
        eprintln!("recovery fixture cycle={cycle}: fault observed");
        let failed = fixture.resources.status(render_format());
        assert_eq!(
            failed.driver_state,
            mixrelay_control_protocol::DriverState::Unavailable
        );
        let session = failed.audio_session.unwrap();
        assert_eq!(session.processing_state, ProcessingState::Running);
        assert_eq!(session.issue, Some(SessionIssue::DeviceUnavailable));
        assert_eq!(session.observed_device_sample_rate, None);
        assert!(
            fixture
                .resources
                .prepare_virtual("fixture", &mut WindowsCaptureSet::default())
                .is_err()
        );
        // Even a plausible header must not revive the same untrusted owner.
        fixture.mutate(&io, |io| {
            io.test_set_online(true);
            io.test_set_produced(0);
        });
        revision += 1;
        fixture.submit(revision, None); // Editing the faulted graph still activates.
        assert!(fixture.resources.virtual_failed.load(Ordering::Acquire));
        revision += 1;
        fixture.submit(revision, Some(WindowsCaptureSet::default()));
        fixture.wait_for(|| {
            !fixture
                .resources
                .device_clock_active
                .load(Ordering::Acquire)
        });
        assert!(!fixture.resources.virtual_failed.load(Ordering::Acquire));
        assert_ne!(
            fixture
                .resources
                .status(render_format())
                .audio_session
                .unwrap()
                .issue,
            Some(SessionIssue::DeviceUnavailable)
        );
        drop(io);
        fixture.wait_for(|| weak.upgrade().is_none()); // Retirement, not audio-thread free.
        eprintln!("recovery fixture cycle={cycle}: removed and released");
    }
}

#[test]
fn shutdown_joins_worker_even_when_a_mapping_has_failed() {
    let fixture = WorkerFixture::new();
    let io = Arc::new(Mutex::new(transport::Transport::test_fixture(LAB_LAYOUT)));
    io.lock().unwrap().test_set_online(false);
    let weak = Arc::downgrade(&io);
    fixture.submit(
        2,
        Some(WindowsCaptureSet {
            virtual_io: Some(io),
            ..WindowsCaptureSet::default()
        }),
    );
    fixture.wait_for(|| fixture.resources.virtual_failed.load(Ordering::Acquire));
    let status = fixture.resources.runtime_status.clone();
    drop(fixture);
    assert_eq!(
        status
            .status(render_format())
            .audio_session
            .unwrap()
            .processing_state,
        ProcessingState::Stopped
    );
    assert!(weak.upgrade().is_none());
}
