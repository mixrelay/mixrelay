//! Owning wrappers around the native `PipeWire` callback transport. Destruction is
//! control-thread only: native disconnect drains callbacks before freeing data.
use std::{
    ffi::{CString, c_char, c_void},
    ptr::NonNull,
};
use tonenari_audio_io::process::ProcessReport;

#[derive(Default)]
#[repr(C)]
struct NativeCounters {
    overflows: u64,
    starvations: u64,
}

#[repr(C)]
pub(super) struct NativeCycle {
    pub position: u64,
    pub monotonic_ns: u64,
    pub clock_id: u32,
    pub rate: u32,
    pub frames: u32,
}

unsafe extern "C" {
    fn tn_pw_stream_open(
        name: *const c_char,
        description: *const c_char,
        target: *const c_char,
        mode: u32,
        channels: u32,
        sample_rate: u32,
        block_frames: u32,
    ) -> *mut c_void;
    fn tn_pw_stream_close(stream: *mut c_void);
    fn tn_pw_stream_ready(stream: *mut c_void) -> i32;
    fn tn_pw_stream_connected(stream: *mut c_void) -> i32;
    fn tn_pw_stream_input_activity(stream: *mut c_void) -> i32;
    fn tn_pw_stream_clock(stream: *mut c_void) -> u64;
    fn tn_pw_stream_format(stream: *mut c_void) -> u64;
    fn tn_pw_stream_quantum(stream: *mut c_void) -> u64;
    fn tn_pw_stream_discard_input(stream: *mut c_void);
    fn tn_pw_stream_counters(stream: *mut c_void) -> NativeCounters;
    fn tn_pw_stream_read(stream: *mut c_void, samples: *mut f32, frames: u32) -> u32;
    fn tn_pw_stream_write(stream: *mut c_void, samples: *const f32, frames: u32) -> u32;
    fn tn_pw_driver_open(
        process: unsafe extern "C" fn(*mut c_void, *const NativeCycle),
        userdata: *mut c_void,
    ) -> *mut c_void;
    fn tn_pw_driver_ready(driver: *mut c_void) -> i32;
    fn tn_pw_driver_close(driver: *mut c_void);
}

pub(super) struct NativeStream {
    pointer: NonNull<c_void>,
    channels: usize,
    counters: NativeCounters,
    native_consumer: bool,
    sample_rate: u32,
}

// SAFETY: the native allocation is pinned and its control loop owns all native
// callbacks. Moving this handle transfers the sole Rust-side ring consumer or
// producer. It is intentionally not Sync and cannot be cloned.
unsafe impl Send for NativeStream {}

impl NativeStream {
    pub(super) fn open_format(
        name: &str,
        description: &str,
        target: Option<&str>,
        mode: u32,
        channels: u16,
        format: tonenari_audio_graph::RenderFormat,
    ) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "invalid PipeWire node name")?;
        let description = CString::new(description).map_err(|_| "invalid PipeWire description")?;
        let target = target
            .map(CString::new)
            .transpose()
            .map_err(|_| "invalid PipeWire target")?;
        // SAFETY: input strings remain live until the native factory copies them.
        let pointer = unsafe {
            tn_pw_stream_open(
                name.as_ptr(),
                description.as_ptr(),
                target.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                mode,
                u32::from(channels),
                format.sample_rate,
                format.block_frames,
            )
        };
        Ok(Self {
            pointer: NonNull::new(pointer).ok_or("could not open native PipeWire stream")?,
            channels: usize::from(channels),
            counters: NativeCounters::default(),
            native_consumer: matches!(mode, 1 | 4),
            sample_rate: format.sample_rate,
        })
    }

    pub(super) fn ready(&self) -> bool {
        // SAFETY: this is an atomic observation of our live owned stream.
        let ready = unsafe { tn_pw_stream_ready(self.pointer.as_ptr()) != 0 };
        ready
            && self.negotiated_format()
                == (u64::from(self.sample_rate) << 32 | self.channels as u64)
    }

    pub(super) fn connected(&self) -> bool {
        // SAFETY: atomic state observation; no native control calls on the DSP thread.
        unsafe { tn_pw_stream_connected(self.pointer.as_ptr()) != 0 }
    }

    pub(super) fn shares_clock(&self, clock_id: u32, rate: u32) -> bool {
        // SAFETY: atomic value copy; never dereferences native IO memory here.
        let observed = unsafe { tn_pw_stream_clock(self.pointer.as_ptr()) };
        observed != 0 && observed == (u64::from(clock_id) << 32 | u64::from(rate))
    }

    pub(super) fn negotiated_format(&self) -> u64 {
        // SAFETY: atomic negotiated format snapshot from our owned stream.
        unsafe { tn_pw_stream_format(self.pointer.as_ptr()) }
    }

    pub(super) fn callback_quantum(&self) -> u64 {
        // SAFETY: atomic value snapshot; graph and PCM units remain distinct.
        unsafe { tn_pw_stream_quantum(self.pointer.as_ptr()) }
    }

    pub(super) fn input_activity(&self) -> tonenari_audio_io::process::InputActivity {
        use tonenari_audio_io::process::InputActivity;
        // SAFETY: one atomic native state read; ownership keeps the stream live.
        match unsafe { tn_pw_stream_input_activity(self.pointer.as_ptr()) } {
            1 => InputActivity::Inactive,
            2 => InputActivity::Active,
            _ => InputActivity::Unknown,
        }
    }

    pub(super) fn discard_input(&mut self) {
        // SAFETY: exclusive engine-side consumer. Native rejects output modes;
        // only the consumer cursor changes, never the concurrent producer's.
        unsafe {
            tn_pw_stream_discard_input(self.pointer.as_ptr());
        }
    }

    pub(super) fn take_report(&mut self) -> ProcessReport {
        // SAFETY: native only reads atomic counters from the owned live stream.
        let counters = unsafe { tn_pw_stream_counters(self.pointer.as_ptr()) };
        let report = ProcessReport {
            // The opposite (DSP-facing) side already reports through ExternalIo.
            // Include only native-side failures here to avoid double counting.
            input_starvations: if self.native_consumer {
                counters
                    .starvations
                    .saturating_sub(self.counters.starvations)
            } else {
                0
            },
            output_overflows: if self.native_consumer {
                0
            } else {
                counters.overflows.saturating_sub(self.counters.overflows)
            },
            discontinuities: 0,
        };
        self.counters = counters;
        report
    }

    pub(super) fn read(&mut self, destination: &mut [f32]) -> u32 {
        let frames = destination.len() / self.channels;
        destination.fill(0.0);
        let Ok(frames) = u32::try_from(frames) else {
            return 0;
        };
        // SAFETY: the mutable slice holds at least frames * channels samples.
        unsafe { tn_pw_stream_read(self.pointer.as_ptr(), destination.as_mut_ptr(), frames) }
    }

    pub(super) fn write(&mut self, source: &[f32]) -> u32 {
        let Ok(frames) = u32::try_from(source.len() / self.channels) else {
            return 0;
        };
        // SAFETY: source holds frames * channels readable samples; native copies them.
        unsafe { tn_pw_stream_write(self.pointer.as_ptr(), source.as_ptr(), frames) }
    }
}

impl Drop for NativeStream {
    fn drop(&mut self) {
        // SAFETY: exclusive owning handle, retired off the DSP callback.
        unsafe {
            tn_pw_stream_close(self.pointer.as_ptr());
        }
    }
}

pub(super) struct NativeDriver(NonNull<c_void>);
impl NativeDriver {
    /// Caller pins userdata until this owner is dropped; only the RT callback may
    /// access it while live. Drop synchronizes with the native data loop.
    pub(super) unsafe fn open(
        process: unsafe extern "C" fn(*mut c_void, *const NativeCycle),
        userdata: *mut c_void,
    ) -> Result<Self, String> {
        // SAFETY: caller guarantees callback signature and exclusive userdata lifetime.
        NonNull::new(unsafe { tn_pw_driver_open(process, userdata) })
            .map(Self)
            .ok_or_else(|| "could not open PipeWire graph clock".to_owned())
    }
    pub(super) fn ready(&self) -> bool {
        // SAFETY: atomic state read on our owned allocation.
        unsafe { tn_pw_driver_ready(self.0.as_ptr()) != 0 }
    }
}
impl Drop for NativeDriver {
    fn drop(&mut self) {
        // SAFETY: destroys and drains the unique native driver before userdata dies.
        unsafe {
            tn_pw_driver_close(self.0.as_ptr());
        }
    }
}
