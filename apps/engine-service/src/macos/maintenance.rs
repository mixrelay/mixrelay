//! Control-side native observation and off-audio-thread reclamation. Both workers
//! have an explicit owner; shutdown waits for their current operation to finish.

use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use mixrelay_audio_engine::ProcessorExchange;
use mixrelay_engine_service::{
    ControlMaintenance, EngineGraphController, session::AdapterRuntimeStatus,
};

use crate::macos_devices::MacDevices;

/// Field order also governs cancellation/unwind: stop native observation before
/// the final control checkpoint, while the caller still owns its audio worker.
pub(super) struct ControlObservers {
    format_monitor: Option<MaintenanceWorker>,
    control: Option<ControlMaintenance>,
    devices: Weak<MacDevices>,
}

impl ControlObservers {
    pub(super) fn start(
        status: &Arc<AdapterRuntimeStatus>,
        controller: &Arc<EngineGraphController>,
        devices: &Arc<MacDevices>,
    ) -> Result<Self, String> {
        // If monitor startup fails, control maintenance still finalizes and joins.
        let mut observers = Self {
            format_monitor: None,
            control: Some(controller.start_control_maintenance()?),
            devices: Arc::downgrade(devices),
        };
        observers.format_monitor = Some(start_format_monitor(status, controller, devices)?);
        Ok(observers)
    }

    pub(super) fn stop_observing(&self) {
        if let Some(devices) = self.devices.upgrade() {
            devices.stop_preparing();
        }
        if let Some(control) = &self.control {
            control.stop_observing();
        }
        if let Some(monitor) = &self.format_monitor {
            monitor.request_stop();
        }
    }

    pub(super) fn finish(mut self) -> Result<(), String> {
        self.stop_observing();
        let observed = self
            .format_monitor
            .take()
            .map_or(Ok(()), MaintenanceWorker::finish);
        // Persistence must still finish if the native observer panicked.
        let saved = self
            .control
            .take()
            .map_or(Ok(()), ControlMaintenance::finish);
        observed.and(saved)
    }
}

impl Drop for ControlObservers {
    fn drop(&mut self) {
        // Cancel all observation before either join, including startup unwind.
        self.stop_observing();
    }
}

/// Stops, wakes and joins a maintenance worker on the control thread. It must
/// never be moved into its own worker or dropped from an audio callback.
pub(super) struct MaintenanceWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MaintenanceWorker {
    /// Cancels and joins the observer, retaining any panic as a shutdown error.
    pub(super) fn finish(mut self) -> Result<(), String> {
        self.join()
    }

    fn join(&mut self) -> Result<(), String> {
        self.request_stop();
        self.thread
            .take()
            .map_or(Ok(()), mixrelay_engine_service::session::join_worker)
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    fn start(
        name: &'static str,
        work: impl FnOnce(&AtomicBool) + Send + 'static,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name(name.into())
            .spawn(move || work(&worker_stop))
            .map_err(|error| format!("{name} failed to start: {error}"))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for MaintenanceWorker {
    fn drop(&mut self) {
        // Cancellation/unwind still joins; explicit finish also propagates failure.
        let _ = self.join();
    }
}

/// Reclaims retired processors/devices outside the audio thread. Keep this guard
/// alive until the audio worker joins. Weak references do not extend engine life.
/// Returns an error if the worker cannot be spawned.
pub(super) fn start_reclaimer(
    exchange: Weak<ProcessorExchange>,
    devices: Weak<MacDevices>,
) -> Result<MaintenanceWorker, String> {
    MaintenanceWorker::start("mixrelay-processor-reclaimer", move |stop| {
        while !stop.load(Ordering::Acquire) {
            if !reclaim_once(&exchange, &devices) {
                break;
            }
            thread::park_timeout(Duration::from_millis(100));
        }
    })
}

// Upgraded owners cannot escape one pass into the worker's wait interval.
// false means the engine owner is gone, so there is nothing left to supervise.
fn reclaim_once(exchange: &Weak<ProcessorExchange>, devices: &Weak<MacDevices>) -> bool {
    let Some(exchange) = exchange.upgrade() else {
        return false;
    };
    exchange.reclaim_retired();
    if let Some(devices) = devices.upgrade() {
        devices.reclaim();
    }
    true
}

/// Observes device clocks and reconciles graphs off the audio thread. Drop this
/// guard before finishing control persistence. Returns a thread-start error.
fn start_format_monitor(
    status: &Arc<AdapterRuntimeStatus>,
    controller: &Arc<EngineGraphController>,
    devices: &Arc<MacDevices>,
) -> Result<MaintenanceWorker, String> {
    let weak_status = Arc::downgrade(status);
    let weak_controller = Arc::downgrade(controller);
    let weak_devices = Arc::downgrade(devices);
    MaintenanceWorker::start("mixrelay-device-format-monitor", move |stop| {
        let mut last_error = None;
        let mut last_trace = Instant::now();
        while !stop.load(Ordering::Acquire) {
            let (Some(status), Some(controller)) =
                (weak_status.upgrade(), weak_controller.upgrade())
            else {
                break;
            };
            if super::endpoint_backend::MacVirtualEndpointBackend::snapshot()
                .is_ok_and(|snapshot| snapshot.state == mixrelay_driver_control::DriverState::Ready)
            {
                status.observe_endpoint(true);
            } else {
                status.forget_endpoint_observation();
            }
            if let Some(devices) = weak_devices.upgrade() {
                if last_trace.elapsed() >= Duration::from_secs(1) {
                    devices.trace_audio();
                    last_trace = Instant::now();
                }
                devices.recover(stop);
                if stop.load(Ordering::Acquire) {
                    break;
                }
                status.observe_routing_clock(devices.follows_hardware(), devices.routing_rate());
            }
            // Errors retain the observed mismatch and retry, never write the old rate.
            let error = controller.reconcile_observed_device_rate().err();
            if error != last_error {
                if let Some(error) = &error {
                    tracing::error!(?error, "MixRelay external rate reconciliation failed");
                }
                last_error = error;
            }
            drop(status);
            drop(controller);
            thread::park_timeout(Duration::from_millis(if last_error.is_some() {
                1000
            } else {
                250
            }));
        }
    })
}

#[cfg(test)]
mod tests;
