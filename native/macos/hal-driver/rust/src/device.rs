use mixrelay_macos_transport::endpoint_identity::endpoint_uid;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::abi::{Geometry, NativeOps, Snapshot};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct Attachment {
    pub descriptor: OwnedFd,
    pub size: usize,
    pub geometry: Geometry,
}

pub(crate) struct Pending {
    pub generation: u64,
    pub requested: bool,
    pub retry_after: u32,
    pub failures: u32,
    pub attachment: Option<Attachment>,
    pub name: String,
    pub rate: u32,
}

pub(crate) struct Control {
    pub name: String,
    pub clients: BTreeMap<u32, bool>,
    pub pending: Option<Pending>,
    pub next_generation: u64,
    pub attachment_failed: bool,
}

pub(crate) struct Device {
    pub id: u32,
    pub endpoint: [u8; 16],
    pub uid: String,
    pub alive: AtomicBool,
    pub running: AtomicU32,
    pub rate: AtomicU32,
    pub render_channels: AtomicU32,
    pub capture_channels: AtomicU32,
    pub input_active: AtomicBool,
    pub output_active: AtomicBool,
    pub control: Mutex<Control>,
    pub native: NativeOps,
    pub runtime: *mut c_void,
    frozen: AtomicBool,
    io_readers: AtomicUsize,
}

// Runtime is immutable in memory location, control mutation is serialized and
// excludes IO readers. Native timestamp/IO callbacks obey their lock-free ABI.
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    pub(crate) fn new(
        id: u32,
        geometry: Geometry,
        name: String,
        rate: u32,
        native_runtime: (NativeOps, *mut c_void),
    ) -> Self {
        Self {
            id,
            endpoint: geometry.endpoint_id,
            uid: endpoint_uid(geometry.endpoint_id),
            alive: AtomicBool::new(true),
            running: AtomicU32::new(0),
            rate: AtomicU32::new(rate),
            render_channels: AtomicU32::new(geometry.render_channels),
            capture_channels: AtomicU32::new(geometry.capture_channels),
            input_active: AtomicBool::new(true),
            output_active: AtomicBool::new(true),
            control: Mutex::new(Control {
                name,
                clients: BTreeMap::new(),
                pending: None,
                next_generation: 1,
                attachment_failed: false,
            }),
            native: native_runtime.0,
            runtime: native_runtime.1,
            frozen: AtomicBool::new(false),
            io_readers: AtomicUsize::new(0),
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let control = lock(&self.control);
        let mut name = [0; 128];
        name[..control.name.len()].copy_from_slice(control.name.as_bytes());
        let mut uid = [0; 80];
        uid[..self.uid.len()].copy_from_slice(self.uid.as_bytes());
        Snapshot {
            device_id: self.id,
            input_stream_id: self.id + 1,
            output_stream_id: self.id + 2,
            sample_rate: self.rate.load(Ordering::Acquire),
            render_channels: self.render_channels.load(Ordering::Acquire),
            capture_channels: self.capture_channels.load(Ordering::Acquire),
            client_count: u32::try_from(control.clients.len()).unwrap_or(u32::MAX),
            alive: self.alive.load(Ordering::Acquire),
            running: self.running.load(Ordering::Acquire) != 0,
            input_active: self.input_active.load(Ordering::Acquire),
            output_active: self.output_active.load(Ordering::Acquire),
            name,
            uid,
        }
    }

    pub(crate) fn io_pin(&self) -> Option<IoPin<'_>> {
        self.io_readers.fetch_add(1, Ordering::SeqCst);
        if self.frozen.load(Ordering::SeqCst) {
            self.io_readers.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(IoPin(self))
    }

    /// Only call while holding `control`. Waiting and eventual reclamation are
    /// confined to the control plane, never the callback's last-reader path.
    pub(crate) fn freeze(&self) -> Freeze<'_> {
        self.frozen.store(true, Ordering::SeqCst);
        while self.io_readers.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
        Freeze(self)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Registry reclamation guarantees no slot pins or callbacks remain.
        unsafe { (self.native.destroy)(self.runtime) };
    }
}

pub(crate) struct IoPin<'a>(&'a Device);
impl Drop for IoPin<'_> {
    fn drop(&mut self) {
        self.0.io_readers.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) struct Freeze<'a>(&'a Device);
impl Drop for Freeze<'_> {
    fn drop(&mut self) {
        self.0.frozen.store(false, Ordering::SeqCst);
    }
}
