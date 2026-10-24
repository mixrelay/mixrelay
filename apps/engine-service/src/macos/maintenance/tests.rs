use super::*;
use std::sync::mpsc;
use std::time::Instant;

#[test]
fn stopping_observers_cancels_resource_preparation_before_joining() {
    use mixrelay_engine_service::GraphResourceBackend;

    let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
    let devices = Arc::new(MacDevices::without_devices(status, 48_000, 32));
    let observers = ControlObservers {
        format_monitor: None,
        control: None,
        devices: Arc::downgrade(&devices),
    };
    observers.stop_observing();

    let error = devices
        .reconcile(
            &mixrelay_audio_graph::GraphDocument::new(1, vec![], vec![]),
            mixrelay_engine_service::PlaybackBindings::Preserve,
        )
        .unwrap_err();
    assert!(error.to_string().contains("cancelled during shutdown"));
    let error = devices.device_inventory(&[]).unwrap_err();
    assert!(error.to_string().contains("cancelled during shutdown"));
    observers.finish().unwrap();
    assert_eq!(Arc::strong_count(&devices), 1);
}

#[test]
fn dropping_worker_wakes_park_and_releases_its_captured_owner() {
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = MaintenanceWorker::start("maintenance-drop-test", move |stop| {
        ready_tx.send(()).unwrap();
        while !stop.load(Ordering::Acquire) {
            // A pending unpark token also covers drop racing with entry to park.
            thread::park();
        }
        drop(owner);
    })
    .unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    drop(worker);

    assert!(weak.upgrade().is_none());
}

#[test]
fn stop_request_prevents_another_observation_before_join() {
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let worker = MaintenanceWorker::start("observer-cancellation-test", move |stop| {
        while !stop.load(Ordering::Acquire) {
            count.fetch_add(1, Ordering::Relaxed);
            entered.send(()).unwrap();
            blocked.recv().unwrap();
        }
    })
    .unwrap();
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.request_stop();
    release.send(()).unwrap();
    drop(worker);
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn dropping_worker_joins_an_in_flight_operation_before_returning() {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (joined_tx, joined_rx) = mpsc::sync_channel(1);
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let worker = MaintenanceWorker::start("maintenance-join-test", move |_| {
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        worker_finished.store(true, Ordering::Release);
    })
    .unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let stop = worker.stop.clone();
    let dropping = thread::spawn(move || {
        drop(worker);
        joined_tx.send(()).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !stop.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(!finished.load(Ordering::Acquire));
    assert!(matches!(
        joined_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    release_tx.send(()).unwrap();
    joined_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dropping.join().unwrap();
    assert!(finished.load(Ordering::Acquire));
}

#[test]
fn failed_later_startup_drops_previously_started_maintenance() {
    fn start_then_fail(owner: Arc<()>) -> Result<(), &'static str> {
        let _worker = MaintenanceWorker::start("maintenance-startup-test", move |stop| {
            while !stop.load(Ordering::Acquire) {
                thread::park();
            }
            drop(owner);
        })
        .unwrap();
        Err("later startup failed")
    }
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);

    assert_eq!(start_then_fail(owner), Err("later startup failed"));

    assert!(weak.upgrade().is_none());
}

#[test]
fn reclamation_pass_releases_upgraded_exchange_before_waiting() {
    let exchange = Arc::new(ProcessorExchange::new(1));
    let weak = Arc::downgrade(&exchange);

    assert!(reclaim_once(&weak, &Weak::new()));

    assert_eq!(Arc::strong_count(&exchange), 1);
    drop(exchange);
    assert!(!reclaim_once(&weak, &Weak::new()));
    assert!(weak.upgrade().is_none());
}

#[test]
fn reclaimer_exits_when_the_engine_owner_is_gone() {
    let weak = Weak::new();
    let worker = start_reclaimer(weak.clone(), Weak::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.thread.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "reclaimer retained its own exchange"
        );
        thread::sleep(Duration::from_millis(1));
    }

    assert!(weak.upgrade().is_none());
    drop(worker);
}

fn observers_with_lifetime_probe() -> (
    ControlObservers,
    Arc<AtomicBool>,
    Weak<EngineGraphController>,
) {
    let controller = Arc::new(EngineGraphController::new(
        Arc::new(ProcessorExchange::new(1)),
        "endpoint",
        mixrelay_audio_graph::RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        },
        Arc::new(|| true),
    ));
    let control = controller.start_control_maintenance().unwrap();
    let weak = Arc::downgrade(&controller);
    let probe = weak.clone();
    let alive_at_monitor_exit = Arc::new(AtomicBool::new(false));
    let observed = alive_at_monitor_exit.clone();
    let format_monitor = MaintenanceWorker::start("observer-order-test", move |stop| {
        while !stop.load(Ordering::Acquire) {
            thread::park();
        }
        // Only ControlMaintenance owns the controller by this point. Its final
        // checkpoint and owner release must follow the format monitor's join.
        observed.store(probe.upgrade().is_some(), Ordering::Release);
    })
    .unwrap();
    drop(controller);
    (
        ControlObservers {
            format_monitor: Some(format_monitor),
            control: Some(control),
            devices: Weak::new(),
        },
        alive_at_monitor_exit,
        weak,
    )
}

#[test]
fn explicit_shutdown_joins_monitor_before_final_control_owner_release() {
    let (observers, observed, weak) = observers_with_lifetime_probe();

    observers.finish().unwrap();

    assert!(observed.load(Ordering::Acquire));
    assert!(weak.upgrade().is_none());
}

#[test]
fn failed_monitor_reports_its_reason_and_still_finalizes_control() {
    let (mut observers, _, weak) = observers_with_lifetime_probe();
    // Retire the normal monitor before injecting a deterministic failure.
    observers.format_monitor.take().unwrap().finish().unwrap();
    observers.format_monitor = Some(
        MaintenanceWorker::start("failed-format-monitor", |_| {
            panic!("injected format observation failure");
        })
        .unwrap(),
    );

    assert_eq!(
        observers.finish(),
        Err("failed-format-monitor panicked: injected format observation failure".into())
    );
    assert!(weak.upgrade().is_none());
}

#[test]
fn explicit_reclaimer_finish_reports_a_panicked_worker_after_owner_release() {
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let worker = MaintenanceWorker::start("failed-reclaimer", move |_| {
        drop(owner);
        panic!("injected reclaim failure");
    })
    .unwrap();

    assert_eq!(
        worker.finish(),
        Err("failed-reclaimer panicked: injected reclaim failure".into())
    );
    assert!(weak.upgrade().is_none());
}

#[test]
fn cancelled_server_future_keeps_the_same_observer_shutdown_order() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let (observers, observed, weak) = observers_with_lifetime_probe();
    let mut serving = Box::pin(async move {
        let _observers = observers;
        std::future::pending::<()>().await;
    });
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(serving.as_mut().poll(&mut context), Poll::Pending);

    drop(serving);

    assert!(observed.load(Ordering::Acquire));
    assert!(weak.upgrade().is_none());
}

#[test]
fn unwinding_keeps_the_same_observer_shutdown_order() {
    let (observers, observed, weak) = observers_with_lifetime_probe();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _observers = observers;
        panic!("server failed");
    }));

    assert!(result.is_err());
    assert!(observed.load(Ordering::Acquire));
    assert!(weak.upgrade().is_none());
}
