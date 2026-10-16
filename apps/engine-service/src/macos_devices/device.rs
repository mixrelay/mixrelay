//! Graph-facing queue handle and native worker lifetime.
use super::{
    activity::BridgeActivity,
    aggregate::Aggregate,
    bridge::{DeviceBridge, NEXT_DEVICE_EPOCH},
    ffi::{mr_device_io_graph_read, mr_device_io_graph_write, mr_device_io_output_epoch},
    host::Host,
    native::Native,
    spec::Spec,
    worker::WorkerContext,
};
use mixrelay_audio_io::process::{InputActivity, TransferOutcome};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
pub(super) struct Device {
    pub(super) activity: Arc<BridgeActivity>,
    pub(super) identity: u64,
    pub(super) native: Arc<Native>,
    pub(super) channels: usize,
    pub(super) input: bool,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) failed: Arc<AtomicBool>,
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) graph_rate: Arc<AtomicU32>,
    pub(super) graph_quantum: Arc<AtomicU32>,
    pub(super) ready_rate: Arc<AtomicU32>,
    pub(super) native_rate: Arc<AtomicU32>,
    pub(super) clock_identity: Arc<AtomicU32>,
    pub(super) bridge_ready: Arc<AtomicBool>,
    pub(super) pending_bridge: Arc<Mutex<Option<DeviceBridge>>>,
    pub(super) clock_aligned: Arc<AtomicBool>,
    pub(super) worker: Option<thread::JoinHandle<()>>,
}
impl Device {
    #[cfg(test)]
    pub(super) fn open(spec: &Spec, rate: u32, host: Arc<dyn Host>) -> Result<Self, String> {
        Self::open_with_group(spec, rate, None, 0, host)
    }
    pub(super) fn open_with_group(
        spec: &Spec,
        rate: u32,
        group: Option<Arc<Aggregate>>,
        buffer_frames: u32,
        host: Arc<dyn Host>,
    ) -> Result<Self, String> {
        let native = Arc::new(Native::create(spec, rate, group, buffer_frames, host)?);
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(AtomicU64::new(0));
        let graph_rate = Arc::new(AtomicU32::new(rate));
        let graph_quantum = Arc::new(AtomicU32::new(buffer_frames));
        let ready_rate = Arc::new(AtomicU32::new(0));
        let native_rate = Arc::new(AtomicU32::new(0));
        let clock_identity = Arc::new(AtomicU32::new(0));
        let bridge_ready = Arc::new(AtomicBool::new(false));
        let pending_bridge = Arc::new(Mutex::new(None));
        let clock_aligned = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let activity = Arc::clone(&native.render.activity);
        let context = WorkerContext {
            activity: Arc::clone(&activity),
            native: native.clone(),
            stop: stop.clone(),
            failed: failed.clone(),
            epoch: epoch.clone(),
            graph_rate: graph_rate.clone(),
            graph_quantum: graph_quantum.clone(),
            ready_rate: ready_rate.clone(),
            native_rate: native_rate.clone(),
            clock_identity: clock_identity.clone(),
            bridge_ready: bridge_ready.clone(),
            pending_bridge: pending_bridge.clone(),
            clock_aligned: clock_aligned.clone(),
            channels: usize::from(spec.channels),
            input: spec.input,
        };
        let worker = thread::Builder::new()
            .name("mixrelay-coreaudio-device".into())
            .spawn(move || context.run(&tx))
            .map_err(|e| e.to_string())?;
        let result = Self {
            activity,
            identity: NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed),
            native,
            channels: usize::from(spec.channels),
            input: spec.input,
            stop,
            failed,
            epoch,
            graph_rate,
            graph_quantum,
            ready_rate,
            native_rate,
            clock_identity,
            bridge_ready,
            pending_bridge,
            clock_aligned,
            worker: Some(worker),
        };
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(result),
            error => {
                result.stop.store(true, Ordering::Release);
                // Drop transfers the worker to tracked retirement; a timeout
                // cannot cancel a native call or release its callback allocation.
                Err(format!("CoreAudio device initialization failed: {error:?}"))
            }
        }
    }
    /// This device's latest callback label (its mean frame) in host ticks, or zero before any.
    pub(super) fn callback_host(&self) -> u64 {
        // SAFETY: this owner retains Native; only an atomic is read.
        unsafe { super::ffi::mr_device_io_callback_host(self.native.pointer.as_ptr()) }
    }
    /// `host` labels this graph cycle's transfers; see `DeviceSet::graph_host`.
    pub(super) fn read(&self, samples: &mut [f32], host: u64) -> TransferOutcome {
        samples.fill(0.0);
        if !self.input || !self.ready() || !samples.len().is_multiple_of(self.channels) {
            return TransferOutcome::complete(0);
        }
        let frames = u32::try_from(samples.len() / self.channels).unwrap_or(0);
        if !self.activity.ready() {
            return super::recovery::offline_transfer(samples.len(), self.channels);
        }
        // SAFETY: this serialized graph consumer retains Native and owns the
        // complete writable PCM span, disjoint from the producer's queue storage.
        let read = unsafe {
            mr_device_io_graph_read(
                self.native.pointer.as_ptr(),
                samples.as_mut_ptr(),
                frames,
                self.activity.read_epoch(self.epoch.load(Ordering::Acquire)),
                host,
            )
        };
        TransferOutcome {
            frames: read.frames,
            discontinuity: false,
            activity: if read.initial_epoch && read.frames < frames {
                // The first freshness reset admits a timeline; it is not loss
                // from an already running input. The next shortage is Active.
                InputActivity::Priming
            } else {
                InputActivity::Active
            },
        }
    }
    pub(super) fn write(&self, samples: &[f32], host: u64) -> TransferOutcome {
        if self.input || !self.ready() || !samples.len().is_multiple_of(self.channels) {
            return TransferOutcome::complete(0);
        }
        let frames = u32::try_from(samples.len() / self.channels).unwrap_or(0);
        if !self.activity.ready() {
            return super::recovery::offline_transfer(samples.len(), self.channels);
        }
        TransferOutcome::complete(unsafe {
            mr_device_io_graph_write(self.native.pointer.as_ptr(), samples.as_ptr(), frames, host)
        })
    }
    pub(super) fn ready(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
            && self.ready_rate.load(Ordering::Acquire) == self.graph_rate.load(Ordering::Acquire)
    }

    pub(super) fn set_active(&self, enabled: bool) {
        if self.activity.request(enabled) && !self.input && !enabled {
            // SAFETY: the retained native owner accepts an atomic epoch request.
            // Its IOProc remains the only consumer that discards output PCM.
            unsafe {
                mr_device_io_output_epoch(
                    self.native.pointer.as_ptr(),
                    NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed),
                );
            }
        }
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            super::worker_retirement::WORKERS.retire(worker);
        }
    }
}
