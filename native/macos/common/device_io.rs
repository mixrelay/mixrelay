//! One retained device owner with disjoint control, callback and graph state.
//! The engine owns the stable allocation and releases it only after unregister
//! acknowledges callback drain and listener removal; failed destruction retains the owner.

use std::{
    alloc::{Layout, alloc},
    cell::UnsafeCell,
    ffi::{CStr, c_void},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64},
    },
};

use mixrelay_pcm_ring::PcmRing;

use crate::{
    callback_clock::CallbackClock,
    device_buffers::{BufferSelection, NativeFormats},
    device_io::lifecycle::DeviceIoLifecycle,
};

mod callback;
mod control;
mod diagnostics;
mod ffi;
mod lifecycle;
mod listeners;
mod native;
mod queues;
mod stream_format;
#[cfg(any(test, feature = "test-observation"))]
pub mod test_observation;
#[cfg(test)]
mod tests;

pub use callback::mr_device_io_process;
pub use diagnostics::{
    CLOCK_BREAK_CAPACITY, ClockBreak, ClockBreakRecord, mr_device_io_next_clock_break,
};
pub use ffi::{
    mr_device_buffer_size, mr_device_buffer_write, mr_device_io_alive, mr_device_io_bridge_discard,
    mr_device_io_bridge_read, mr_device_io_bridge_write, mr_device_io_buffer_info,
    mr_device_io_clock_device, mr_device_io_clock_discontinuities, mr_device_io_connect,
    mr_device_io_connect_status, mr_device_io_create, mr_device_io_create_group,
    mr_device_io_destroy, mr_device_io_direct_binding, mr_device_io_disconnect,
    mr_device_io_graph_frames, mr_device_io_graph_read, mr_device_io_graph_write,
    mr_device_io_native_frames, mr_device_io_native_rate, mr_device_io_output_epoch,
    mr_device_io_overflows, mr_device_io_pcm_hook, mr_device_io_quantum, mr_device_io_queued,
    mr_device_io_render_hook, mr_device_io_starvations, mr_device_io_ticks,
};
pub use native::BufferInfo;
pub use queues::GraphRead;

const MAX_FRAMES: u32 = 4096;

/// Borrowed engine render hook, installed only before registration. The hook must
/// not retain native buffers or outlive its enclosing engine render-slot owner.
pub type RenderHook = unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void, u32) -> bool;

/// Serialized clock-boundary processor. Returns whether native PCM is admitted.
/// Runs before native transfer; it may access only its designated queue cursors.
pub type PcmHook = unsafe extern "C" fn(*mut c_void) -> bool;

struct Selection {
    uid: Vec<u8>,
    input: bool,
    channels: u32,
    // The enclosing engine Native retains the aggregate Arc beyond unregister.
    group: *mut c_void,
}

#[derive(Default)]
struct Connection {
    binding: crate::aggregate_group::NativeBinding,
    rate: u32,
    formats: NativeFormats,
    // Shared only with this registration's listener block and callback.
    watch_invalid: Arc<AtomicBool>,
}

#[derive(Default)]
struct Control {
    lifecycle: DeviceIoLifecycle,
    connection: Connection,
    listeners: listeners::Listeners,
}

struct Callback {
    selection: BufferSelection,
    formats: NativeFormats,
    watch_invalid: Arc<AtomicBool>,
    scratch: Vec<f32>,
    prime: bool,
    epoch: u64,
    hook: Option<RenderHook>,
    context: *mut c_void,
    pcm_hook: Option<PcmHook>,
    pcm_context: *mut c_void,
    previous_timestamp: Option<(f64, u64, u32)>,
}

#[derive(Default)]
struct Progress {
    ticks: AtomicU64,
    native_frames: AtomicU64,
    graph_frames: AtomicU64,
    output_epoch: AtomicU64,
    native_quantum: AtomicU32,
    graph_quantum: AtomicU32,
    invalid: AtomicBool,
}

/// Opaque device context. No blanket `Send`/`Sync`: the enclosing engine owner
/// separately grants control, callback and graph access to their designated cells.
pub struct DeviceIo {
    selection: Selection,
    control: UnsafeCell<Control>,
    callback: UnsafeCell<Callback>,
    native_ring: PcmRing,
    graph_ring: PcmRing,
    graph_epoch: UnsafeCell<Option<u64>>,
    clock: CallbackClock,
    clock_evidence: diagnostics::ClockEvidence,
    progress: Progress,
}

impl DeviceIo {
    fn new(uid: &CStr, input: bool, channels: u32, rate: u32) -> Option<Self> {
        if !(1..=64).contains(&channels) || rate == 0 {
            return None;
        }
        let mut name = Vec::new();
        name.try_reserve_exact(uid.to_bytes_with_nul().len()).ok()?;
        name.extend_from_slice(uid.to_bytes_with_nul());
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(MAX_FRAMES as usize * channels as usize)
            .ok()?;
        scratch.resize(MAX_FRAMES as usize * channels as usize, 0.0);
        Some(Self {
            selection: Selection {
                uid: name,
                input,
                channels,
                group: ptr::null_mut(),
            },
            control: UnsafeCell::new(Control {
                connection: Connection {
                    rate,
                    ..Connection::default()
                },
                ..Control::default()
            }),
            callback: UnsafeCell::new(Callback {
                selection: BufferSelection {
                    channels,
                    offset: 0,
                    total: 0,
                    limit: 64,
                },
                formats: NativeFormats::default(),
                watch_invalid: Arc::default(),
                scratch,
                prime: false,
                epoch: 0,
                hook: None,
                context: ptr::null_mut(),
                pcm_hook: None,
                pcm_context: ptr::null_mut(),
                previous_timestamp: None,
            }),
            native_ring: PcmRing::new(channels, 16384)?,
            graph_ring: PcmRing::new(channels, 16384)?,
            graph_epoch: UnsafeCell::new(None),
            clock: CallbackClock::default(),
            clock_evidence: diagnostics::ClockEvidence::default(),
            progress: Progress::default(),
        })
    }

    fn publish(self) -> *mut Self {
        // SAFETY: one correctly aligned allocation becomes the stable IOProc context.
        // A failed allocation drops the unpublished owner and all prepared resources.
        let allocation = unsafe { alloc(Layout::new::<Self>()) }
            .cast::<c_void>()
            .cast::<Self>();
        if !allocation.is_null() {
            // SAFETY: allocation is exclusively owned and large enough for DeviceIo.
            unsafe { allocation.write(self) };
        }
        allocation
    }

    fn ring(&self, graph: bool) -> &PcmRing {
        if graph {
            &self.graph_ring
        } else {
            &self.native_ring
        }
    }
}
