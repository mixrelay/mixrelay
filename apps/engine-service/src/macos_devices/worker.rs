//! Reconnection supervisor. Owns native registration and converter mutation.
use super::{
    bridge::DeviceBridge,
    ffi::{
        tn_device_io_alive, tn_device_io_clock_device, tn_device_io_clock_discontinuities,
        tn_device_io_native_rate, tn_device_io_overflows, tn_device_io_ticks,
    },
    native::{Connection, Native},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
pub(super) struct WorkerContext {
    pub(super) native: Arc<Native>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) failed: Arc<AtomicBool>,
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) graph_rate: Arc<AtomicU32>,
    pub(super) ready_rate: Arc<AtomicU32>,
    pub(super) native_rate: Arc<AtomicU32>,
    pub(super) clock_identity: Arc<AtomicU32>,
    pub(super) bridge_ready: Arc<AtomicBool>,
    pub(super) pending_bridge: Arc<Mutex<Option<DeviceBridge>>>,
    pub(super) clock_aligned: Arc<AtomicBool>,
    pub(super) channels: usize,
    pub(super) input: bool,
}

impl WorkerContext {
    fn supervise_direct(&self) -> Result<bool, String> {
        if self.native.render.reconnect.swap(false, Ordering::AcqRel) {
            return Err("CoreAudio callback requested topology recovery".into());
        }
        if !self.native.render.enabled() {
            return Ok(false);
        }
        // Only lifetime/property supervision remains on this thread. PCM is
        // already rendered in the aggregate callback, not pumped on this timer.
        if !unsafe { tn_device_io_alive(self.native.pointer.as_ptr()) } {
            return Err("CoreAudio direct device lost or reconfigured".into());
        }
        thread::park_timeout(Duration::from_millis(50));
        Ok(true)
    }
    pub(super) fn run(self, tx: &std::sync::mpsc::SyncSender<Result<(), String>>) {
        tonenari_engine_service::device_worker::supervise(
            &self.stop,
            &self.failed,
            tx,
            |connected| self.connection(connected),
        );
    }
    pub(super) fn connection(&self, connected: &mut dyn FnMut()) -> Result<(), String> {
        let pointer = self.native.pointer.as_ptr();
        let _connection = Connection {
            pointer,
            failed: &self.failed,
            clock_identity: &self.clock_identity,
        };
        self.ready_rate.store(0, Ordering::Release);
        self.native.connect(&self.stop)?;
        let native_rate = unsafe { tn_device_io_native_rate(pointer) };
        self.native_rate.store(native_rate, Ordering::Release);
        // Only the connection owner queries CoreAudio. The graph uses this
        // atomic proof, not a UID string, nominal Hz or a property call on RT.
        self.clock_identity.store(
            unsafe { tn_device_io_clock_device(pointer) },
            Ordering::Release,
        );
        let mut bridge = DeviceBridge::new(self, native_rate)?;
        let mut ticks = unsafe { tn_device_io_ticks(pointer) };
        let mut last_progress = Instant::now();
        let mut last_properties = Instant::now();
        let mut activated = false;
        let mut render_generation = self.native.render.generation.load(Ordering::Acquire);
        while !self.stop.load(Ordering::Acquire) {
            if self.supervise_direct()? {
                continue;
            }
            let next_generation = self.native.render.generation.load(Ordering::Acquire);
            if render_generation != next_generation {
                bridge = DeviceBridge::new(self, native_rate)?;
                last_progress = Instant::now();
                activated = false;
                render_generation = next_generation;
            }
            // This lock is worker/control-only, never graph/native callback.
            // Control reserved the single slot before pausing native resources.
            let pending = self
                .pending_bridge
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(mut prepared) = pending {
                self.ready_rate.store(0, Ordering::Release);
                if prepared.native_rate == native_rate {
                    prepared.activate(self);
                    bridge = prepared;
                } else {
                    bridge = DeviceBridge::new(self, native_rate)?;
                }
                activated = false;
            }
            if bridge.graph_rate != self.graph_rate.load(Ordering::Acquire)
                || bridge.transport_overflows != unsafe { tn_device_io_overflows(pointer) }
                || bridge.clock_discontinuities
                    != unsafe { tn_device_io_clock_discontinuities(pointer) }
                || bridge.clock_aligned != self.clock_aligned.load(Ordering::Acquire)
            {
                self.ready_rate.store(0, Ordering::Release);
                bridge = DeviceBridge::new(self, native_rate)?;
                activated = false;
            }
            if last_properties.elapsed() >= Duration::from_millis(50) {
                if !unsafe { tn_device_io_alive(pointer) } {
                    return Err("CoreAudio device lost or reconfigured".into());
                }
                last_properties = Instant::now();
            }
            let next = unsafe { tn_device_io_ticks(pointer) };
            if next != ticks {
                ticks = next;
                last_progress = Instant::now();
                if !activated {
                    self.epoch.store(bridge.epoch, Ordering::Release);
                    self.ready_rate.store(bridge.graph_rate, Ordering::Release);
                    connected();
                    activated = true;
                }
            } else if last_progress.elapsed() >= Duration::from_secs(2) {
                return Err("CoreAudio device stopped delivering audio callbacks".into());
            }
            bridge.observe(self, last_progress.elapsed() < Duration::from_millis(100))?;
            bridge.pump(self)?;
            thread::park_timeout(Duration::from_millis(1));
        }
        Ok(())
    }
}
