//! Rust-owned native runtime. Transport layout and callbacks are imported from
//! the common Rust crate; only operating-system calls cross FFI.
use mixrelay_macos_transport::endpoint_transport::{
    EndpointTransport, EndpointTransportConfig as TransportConfig, mr_endpoint_transport_attach,
    mr_endpoint_transport_config, mr_endpoint_transport_incarnation,
};
use mixrelay_macos_transport::hal_io::{
    HalIoContext as IoContext, mr_hal_io_begin_stream, mr_hal_io_bind, mr_hal_io_clock_state,
    mr_hal_io_is_lock_free, mr_hal_io_read_capture, mr_hal_io_requires_replacement,
    mr_hal_io_set_render_activity, mr_hal_io_write_render,
};
use mixrelay_macos_transport::transport_inspection::inspect_descriptor;
use std::cell::UnsafeCell;
use std::ffi::c_void;

use crate::abi::{Geometry, HostOps, NativeOps, Timestamp};
use crate::clock::Clock;
use crate::io_outcome::IoOutcome;
use crate::registry::Registry;

#[repr(C)]
#[derive(Default)]
struct Timebase {
    numerator: u32,
    denominator: u32,
}

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(output: *mut Timebase) -> i32;
    fn mmap(
        address: *mut c_void,
        size: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, size: usize) -> i32;
}

struct Mapping {
    address: *mut c_void,
    size: usize,
    identity: [u8; 16],
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { munmap(self.address, self.size) };
    }
}

struct AttachedIo {
    context: UnsafeCell<IoContext>,
    transport: *mut EndpointTransport,
    mapping: Mapping,
}

impl Drop for AttachedIo {
    fn drop(&mut self) {
        // Registry control has drained callbacks before retiring an attachment.
        self.context.get_mut().release();
    }
}

impl AttachedIo {
    fn open(geometry: Geometry, fd: i32, size: usize) -> Option<Box<Self>> {
        let address = unsafe { mmap(std::ptr::null_mut(), size, 3, 1, fd, 0) };
        if address as isize == -1 {
            return None;
        }
        let mut mapping = Mapping {
            address,
            size,
            identity: [0; 16],
        };
        let transport = unsafe { mr_endpoint_transport_attach(address, size) };
        let mut config = TransportConfig::default();
        if transport.is_null()
            || !unsafe { mr_endpoint_transport_config(transport, &raw mut config) }
            || config.endpoint_id != geometry.endpoint_id
            || config.render_channels != geometry.render_channels
            || config.capture_channels != geometry.capture_channels
            || !unsafe {
                mr_endpoint_transport_incarnation(transport, mapping.identity.as_mut_ptr())
            }
        {
            return None;
        }
        let context = IoContext::new(geometry.render_channels, geometry.capture_channels)?;
        let result = Box::new(Self {
            context: UnsafeCell::new(context),
            transport,
            mapping,
        });
        if !unsafe { mr_hal_io_is_lock_free(result.context.get()) } {
            return None;
        }
        Some(result)
    }

    fn bind(&self) -> bool {
        unsafe { mr_hal_io_bind(self.context.get(), self.transport.cast()) }
    }
}

struct Runtime {
    clock: Clock,
    io: Box<AttachedIo>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { mr_hal_io_clock_state(self.io.context.get(), self.clock.rate, false) };
    }
}

unsafe extern "C" fn inspect(fd: i32, size: usize, output: *mut Geometry) -> bool {
    let Ok(config) = inspect_descriptor(fd, size) else {
        return false;
    };
    unsafe {
        output.write(Geometry {
            endpoint_id: config.endpoint_id,
            render_channels: config.render_channels,
            capture_channels: config.capture_channels,
        });
    };
    true
}

unsafe extern "C" fn create(
    geometry: *const Geometry,
    rate: u32,
    fd: i32,
    size: usize,
) -> *mut c_void {
    let mut timebase = Timebase::default();
    if unsafe { mach_timebase_info(&raw mut timebase) } != 0 {
        return std::ptr::null_mut();
    }
    let Some(clock) = Clock::new(rate, timebase.numerator, timebase.denominator) else {
        return std::ptr::null_mut();
    };
    let Some(io) = AttachedIo::open(unsafe { *geometry }, fd, size) else {
        return std::ptr::null_mut();
    };
    if !io.bind() {
        return std::ptr::null_mut();
    }
    unsafe { mr_hal_io_clock_state(io.context.get(), rate, false) };
    Box::into_raw(Box::new(Runtime { clock, io })).cast()
}

unsafe extern "C" fn destroy(runtime: *mut c_void) {
    drop(unsafe { Box::from_raw(runtime.cast::<Runtime>()) });
}

unsafe extern "C" fn replace(
    runtime: *mut c_void,
    geometry: *const Geometry,
    rate: u32,
    fd: i32,
    size: usize,
) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    let Some(candidate) = AttachedIo::open(unsafe { *geometry }, fd, size) else {
        return false;
    };
    let mut clock = runtime.clock.clone();
    if !clock.set_rate(rate, unsafe { mach_absolute_time() }) {
        return false;
    }
    let previous = unsafe { &*runtime.io.context.get() };
    let context = unsafe { &mut *candidate.context.get() };
    context.inherit_lifecycle(previous);
    if !candidate.bind() {
        return false;
    }
    if runtime.clock.running && !unsafe { mr_hal_io_begin_stream(candidate.context.get()) } {
        return false;
    }
    runtime.clock = clock;
    unsafe { mr_hal_io_clock_state(candidate.context.get(), rate, runtime.clock.running) };
    if candidate.mapping.identity != runtime.io.mapping.identity {
        unsafe { mr_hal_io_clock_state(runtime.io.context.get(), rate, false) };
    }
    runtime.io = candidate;
    true
}

unsafe extern "C" fn start(runtime: *mut c_void, now: u64) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    let mut clock = runtime.clock.clone();
    if !clock.start(now) || !unsafe { mr_hal_io_begin_stream(runtime.io.context.get()) } {
        return false;
    }
    runtime.clock = clock;
    unsafe { mr_hal_io_clock_state(runtime.io.context.get(), runtime.clock.rate, true) };
    true
}

unsafe extern "C" fn stop(runtime: *mut c_void) {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    runtime.clock.running = false;
    unsafe {
        mr_hal_io_clock_state(runtime.io.context.get(), runtime.clock.rate, false);
        mr_hal_io_set_render_activity(runtime.io.context.get(), 1);
    }
}

unsafe extern "C" fn remaining(runtime: *mut c_void) {
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    unsafe { mr_hal_io_set_render_activity(runtime.io.context.get(), 0) };
}

unsafe extern "C" fn set_rate(runtime: *mut c_void, rate: u32) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    let mut clock = runtime.clock.clone();
    if !clock.set_rate(rate, unsafe { mach_absolute_time() }) {
        return false;
    }
    if runtime.clock.running && !unsafe { mr_hal_io_begin_stream(runtime.io.context.get()) } {
        return false;
    }
    runtime.clock = clock;
    unsafe { mr_hal_io_clock_state(runtime.io.context.get(), rate, runtime.clock.running) };
    true
}

unsafe extern "C" fn timestamp(runtime: *mut c_void, now: u64, output: *mut Timestamp) -> bool {
    unsafe { &*runtime.cast::<Runtime>() }
        .clock
        .timestamp(now, unsafe { &mut *output })
}

unsafe extern "C" fn io(
    runtime: *mut c_void,
    render: bool,
    frame: u64,
    samples: *mut f32,
    count: u32,
) -> IoOutcome {
    // SAFETY: the registry's IO pin retains this initialized native runtime.
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    if render {
        // SAFETY: HAL provides the pinned mapping and count * render channels.
        if unsafe { mr_hal_io_write_render(runtime.io.context.get(), frame, samples, count) } {
            IoOutcome::completed(count)
        } else {
            IoOutcome::default()
        }
    } else {
        // SAFETY: HAL provides count * capture channels writable Float32 samples.
        IoOutcome::completed(unsafe {
            mr_hal_io_read_capture(runtime.io.context.get(), frame, samples, count)
        })
    }
}

unsafe extern "C" fn requires_replacement(runtime: *mut c_void) -> bool {
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    unsafe { mr_hal_io_requires_replacement(runtime.io.context.get()) }
}

unsafe extern "C" fn same_transport(runtime: *mut c_void, fd: i32, size: usize) -> bool {
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    let address = unsafe { mmap(std::ptr::null_mut(), size, 3, 1, fd, 0) };
    if address as isize == -1 {
        return false;
    }
    let mut candidate = Mapping {
        address,
        size,
        identity: [0; 16],
    };
    let transport = unsafe { mr_endpoint_transport_attach(address, size) };
    let valid =
        unsafe { mr_endpoint_transport_incarnation(transport, candidate.identity.as_mut_ptr()) };
    valid && candidate.identity == runtime.io.mapping.identity
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_create_native(host: *const HostOps) -> *mut Registry {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(native_registry(
        *host,
        crate::transport::ProcessInspection::SYSTEM,
    )))
}

/// The registry over the native device operations, inspecting processes through `inspection`.
pub(crate) fn native_registry(
    host: HostOps,
    inspection: crate::transport::ProcessInspection,
) -> Registry {
    let native = NativeOps {
        inspect,
        create,
        destroy,
        replace,
        start,
        stop,
        remaining_client: remaining,
        set_rate,
        timestamp,
        io,
        requires_replacement,
        same_transport,
    };
    Registry::with_inspection(native, host, inspection)
}

/* Used by the C ABI contract test to compare the C11 and Rust atomic layouts. */
#[unsafe(no_mangle)]
extern "C" fn mr_hal_core_io_context_size() -> usize {
    size_of::<IoContext>()
}
#[unsafe(no_mangle)]
extern "C" fn mr_hal_core_io_context_alignment() -> usize {
    align_of::<IoContext>()
}
