use mixrelay_macos_transport::endpoint_identity::endpoint_uid;
use mixrelay_pcm_ring::SampleFormat;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::abi::{Geometry, NativeOps, Snapshot};

pub(crate) type ConversionScratch = [Box<[f32]>; 2];

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
    pub formats: [SampleFormat; 2],
}

pub(crate) struct Control {
    pub name: String,
    pub latency_lease: u64,
    pub latency_revision: u64,
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
    pub input_latency: crate::input_latency::InputLatency,
    pub output_control: crate::output_control::OutputControl,
    pub capture_format: NativeStream,
    pub render_format: NativeStream,
    pub control: Mutex<Control>,
    pub native: NativeOps,
    pub runtime: AtomicPtr<c_void>,
    frozen: AtomicBool,
    io_readers: AtomicUsize,
}

// Runtime stays allocated while alive; control mutation and retirement exclude
// IO readers. Dead devices retain only metadata and return silent PCM without
// touching the released runtime. Native timestamp/IO callbacks obey their lock-free ABI.
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
            input_latency: crate::input_latency::InputLatency::default(),
            output_control: crate::output_control::OutputControl::default(),
            capture_format: NativeStream::new(),
            render_format: NativeStream::new(),
            control: Mutex::new(Control {
                name,
                clients: BTreeMap::new(),
                latency_lease: 0,
                latency_revision: 0,
                pending: None,
                next_generation: 1,
                attachment_failed: false,
            }),
            native: native_runtime.0,
            runtime: AtomicPtr::new(native_runtime.1),
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
            running: self.alive.load(Ordering::Acquire)
                && self.running.load(Ordering::Acquire) != 0,
            input_active: self.input_active.load(Ordering::Acquire),
            output_active: self.output_active.load(Ordering::Acquire),
            name,
            uid,
            capture_format: self.capture_format.format() as u32,
            render_format: self.render_format.format() as u32,
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

    /// Allocates bounded directional conversion storage on the owning control thread.
    pub(crate) fn prepare_scratch(
        &self,
        pending: &Pending,
    ) -> Result<ConversionScratch, std::collections::TryReserveError> {
        self.prepare_scratch_with(pending, mixrelay_audio_io::pcm_buffer::zeroed)
    }

    pub(crate) fn prepare_scratch_with(
        &self,
        pending: &Pending,
        mut reserve: impl FnMut(usize) -> Result<Vec<f32>, std::collections::TryReserveError>,
    ) -> Result<ConversionScratch, std::collections::TryReserveError> {
        let (capture, render) = pending.attachment.as_ref().map_or(
            (
                self.capture_channels.load(Ordering::Acquire),
                self.render_channels.load(Ordering::Acquire),
            ),
            |attachment| {
                (
                    attachment.geometry.capture_channels,
                    attachment.geometry.render_channels,
                )
            },
        );
        Ok([
            NativeStream::prepare(pending.formats[0], capture, &mut reserve)?,
            NativeStream::prepare(pending.formats[1], render, &mut reserve)?,
        ])
    }

    /// Caller holds control and freeze after successful native rate/mapping change.
    ///
    /// # Safety
    /// Device control and its freeze guard remain held, excluding all IO pins until
    /// the matching native rate, geometry and scratch have been fully published.
    pub(crate) unsafe fn publish_configuration(
        &self,
        pending: &Pending,
        scratch: ConversionScratch,
    ) {
        if let Some(attachment) = &pending.attachment {
            self.render_channels
                .store(attachment.geometry.render_channels, Ordering::Release);
            self.capture_channels
                .store(attachment.geometry.capture_channels, Ordering::Release);
        }
        self.input_latency.publish(0, 0, None, None);
        self.rate.store(pending.rate, Ordering::Release);
        let [capture, render] = scratch;
        // SAFETY: the configuration owner holds control and freeze; no IO pin can access
        // old scratch. Only a successful native transition reaches this publication.
        unsafe {
            self.capture_format.publish(pending.formats[0], capture);
            self.render_format.publish(pending.formats[1], render);
        }
    }

    /// The caller holds an IO pin, excluding format/scratch replacement for this call.
    ///
    /// # Safety
    /// `samples` owns `frames * direction_channels` native samples for this synchronous
    /// operation. Float32 is aligned; integer storage is packed bytes. The device's IO
    /// pin stays held, `alive` was checked after acquiring it, and no other reference
    /// aliases the host's operation buffer. Removal cannot release the runtime until
    /// this pin drains.
    pub(crate) unsafe fn process_io(
        &self,
        render: bool,
        frame: u64,
        samples: *mut c_void,
        frames: u32,
    ) -> bool {
        let runtime = self.runtime.load(Ordering::Acquire);
        let (stream, channels) = if render {
            (
                &self.render_format,
                self.render_channels.load(Ordering::Acquire),
            )
        } else {
            (
                &self.capture_format,
                self.capture_channels.load(Ordering::Acquire),
            )
        };
        let count = frames as usize * channels as usize;
        let format = stream.format();
        if format == SampleFormat::Float32 {
            if render {
                // SAFETY: the HAL main buffer is exclusive, aligned Float32 in this format;
                // the IO pin fixes its width and format until the callback returns.
                self.output_control
                    .apply(unsafe { std::slice::from_raw_parts_mut(samples.cast::<f32>(), count) });
            }
            return unsafe { (self.native.io)(runtime, render, frame, samples.cast(), frames) };
        }
        stream.with_scratch(count, |scratch| {
            // SAFETY: ReadInput/WriteMix supply count native interleaved samples. Integer
            // formats need only byte alignment; no Float32 reference aliases this storage.
            let bytes = unsafe {
                std::slice::from_raw_parts_mut(
                    samples.cast::<u8>(),
                    count * format.bytes_per_sample(),
                )
            };
            if render {
                if !format.decode(bytes, scratch) {
                    return false;
                }
                self.output_control.apply(scratch);
                unsafe { (self.native.io)(runtime, true, frame, scratch.as_mut_ptr(), frames) }
            } else {
                (unsafe { (self.native.io)(runtime, false, frame, scratch.as_mut_ptr(), frames) })
                    && format.encode(scratch, bytes)
            }
        })
    }
}

/// One directional native format and its exclusively leased conversion storage.
pub(crate) struct NativeStream {
    format: AtomicU32,
    scratch: UnsafeCell<Box<[f32]>>,
    claimed: AtomicBool,
}

// Scratch replacement requires Device::freeze (all IO pins drained) and control ownership.
// IO acquires a nonwaiting exclusive directional lease before accessing the UnsafeCell.
unsafe impl Sync for NativeStream {}

impl NativeStream {
    fn new() -> Self {
        Self {
            format: AtomicU32::new(SampleFormat::Float32 as u32),
            scratch: UnsafeCell::new(Box::default()),
            claimed: AtomicBool::new(false),
        }
    }

    pub(crate) fn format(&self) -> SampleFormat {
        match self.format.load(Ordering::Acquire) {
            1 => SampleFormat::Float32,
            2 => SampleFormat::Signed16,
            3 => SampleFormat::Signed24,
            4 => SampleFormat::Signed32,
            _ => unreachable!("native format is written only from SampleFormat"),
        }
    }

    fn prepare(
        format: SampleFormat,
        channels: u32,
        reserve: &mut impl FnMut(usize) -> Result<Vec<f32>, std::collections::TryReserveError>,
    ) -> Result<Box<[f32]>, std::collections::TryReserveError> {
        if format == SampleFormat::Float32 {
            return Ok(Box::default());
        }
        reserve(crate::formats::MAX_BUFFER_FRAMES * channels as usize).map(Vec::into_boxed_slice)
    }

    /// Caller owns device control and a freeze guard; no IO pin can access this storage.
    ///
    /// # Safety
    /// Device control and freeze exclude all readers of this directional scratch.
    pub(crate) unsafe fn publish(&self, format: SampleFormat, scratch: Box<[f32]>) {
        // SAFETY: publication is control-only after Device::freeze drained all IO readers.
        unsafe { *self.scratch.get() = scratch };
        self.format.store(format as u32, Ordering::Release);
    }

    fn with_scratch(&self, count: usize, process: impl FnOnce(&mut [f32]) -> bool) -> bool {
        if self
            .claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        // SAFETY: this successful directional lease is exclusive; the caller's IO pin
        // prevents replacement or reclamation until after the lease is released.
        let scratch = unsafe { &mut *self.scratch.get() };
        let result = scratch.get_mut(..count).is_some_and(process);
        self.claimed.store(false, Ordering::Release);
        result
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Registry reclamation guarantees no slot pins or callbacks remain.
        // Explicit removal may already have released the runtime while clients
        // still retain this device's identity and metadata.
        let runtime = *self.runtime.get_mut();
        if !runtime.is_null() {
            unsafe { (self.native.destroy)(runtime) };
        }
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
