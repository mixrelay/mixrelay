//! Rust-owned native runtime. Transport layout and callbacks are imported from
//! the common Rust crate; only operating-system calls cross FFI.
use mixrelay_macos_transport::endpoint_transport::{
    EndpointTransport, EndpointTransportConfig as TransportConfig, mr_endpoint_transport_attach,
    mr_endpoint_transport_config, mr_endpoint_transport_incarnation,
};
use mixrelay_macos_transport::hal_io::{
    HalIoContext as IoContext, mr_hal_io_begin_stream, mr_hal_io_bind, mr_hal_io_clock_state,
    mr_hal_io_requires_replacement, mr_hal_io_set_render_activity,
};
use mixrelay_macos_transport::transport_inspection::inspect_descriptor;
use std::cell::UnsafeCell;
use std::ffi::c_void;

use crate::abi::{Geometry, HostOps, NativeOps, Timestamp};
use crate::clock::Clock;
use crate::io_outcome::IoOutcome;
use crate::registry::Registry;
mod client_mix;
use client_mix::{ClientMix, PreparedClient};
mod pcm_snapshot;
pub(crate) use pcm_snapshot::client_snapshot;

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
    geometry: Geometry,
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
            geometry,
        });
        Some(result)
    }

    /// Revalidates the header of a mapping prepared earlier; the Driver service owns
    /// its contents and may have reinitialized it since. Reads only, without mapping.
    fn intact(&self) -> bool {
        let mut config = TransportConfig::default();
        // SAFETY: this owner retains the complete mapping; attach only reads its header.
        let transport =
            unsafe { mr_endpoint_transport_attach(self.mapping.address, self.mapping.size) };
        transport == self.transport
            && unsafe { mr_endpoint_transport_config(transport, &raw mut config) }
            && config.endpoint_id == self.geometry.endpoint_id
            && config.render_channels == self.geometry.render_channels
            && config.capture_channels == self.geometry.capture_channels
    }

    fn bind(&self) -> bool {
        unsafe { mr_hal_io_bind(self.context.get(), self.transport.cast()) }
    }
}

struct Runtime {
    clock: Clock,
    io: Box<AttachedIo>,
    clients: ClientMix,
}

/// Storage for one configuration change, allocated on control before the IO freeze.
/// The registry owns it until the frozen commit consumes it or `discard` releases it.
enum Change {
    Replacement {
        io: Box<AttachedIo>,
        clients: ClientMix,
    },
    Client(PreparedClient),
}

/// # Safety
/// `change` is null or was returned by `prepare`/`prepare_client` and not yet consumed.
unsafe fn take(change: *mut c_void) -> Option<Change> {
    // SAFETY: per the contract above, a non-null pointer owns one boxed change.
    (!change.is_null()).then(|| *unsafe { Box::from_raw(change.cast::<Change>()) })
}

fn boxed(change: Change) -> *mut c_void {
    Box::into_raw(Box::new(change)).cast()
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
    let Some(clients) = ClientMix::new(unsafe { *geometry }) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(Runtime { clock, io, clients })).cast()
}

unsafe extern "C" fn destroy(runtime: *mut c_void) {
    drop(unsafe { Box::from_raw(runtime.cast::<Runtime>()) });
}

unsafe extern "C" fn prepare(
    runtime: *mut c_void,
    geometry: *const Geometry,
    fd: i32,
    size: usize,
) -> *mut c_void {
    // SAFETY: registry control excludes every runtime mutation. IO callbacks share
    // this runtime concurrently; preparation only reads the registered client set.
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    let geometry = unsafe { *geometry };
    let Some(io) = AttachedIo::open(geometry, fd, size) else {
        return std::ptr::null_mut();
    };
    runtime
        .clients
        .replacement(geometry)
        .map_or(std::ptr::null_mut(), |clients| {
            boxed(Change::Replacement { io, clients })
        })
}

unsafe extern "C" fn prepare_client(runtime: *mut c_void, client: u32) -> *mut c_void {
    // SAFETY: as in `prepare`; the new queue joins the endpoint clock without publication.
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    runtime
        .clients
        .prepare(client)
        .map_or(std::ptr::null_mut(), |client| boxed(Change::Client(client)))
}

unsafe extern "C" fn discard(change: *mut c_void) {
    // SAFETY: the registry releases each unconsumed preparation once, outside IO.
    drop(unsafe { take(change) });
}

unsafe extern "C" fn replace(runtime: *mut c_void, change: *mut c_void, rate: u32) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    // SAFETY: the registry passes ownership of its prepared replacement exactly once.
    let Some(Change::Replacement {
        io: candidate,
        mut clients,
    }) = (unsafe { take(change) })
    else {
        return false;
    };
    // Control held the client set fixed since preparation; recheck rather than trust it.
    if !clients.same_members(&runtime.clients)
        || !candidate.intact()
        || !clients.supersede(&runtime.clients)
    {
        return false;
    }
    let mut clock = runtime.clock.clone();
    if !clock.set_rate(rate, unsafe { mach_absolute_time() }) {
        return false;
    }
    let previous = unsafe { &*runtime.io.context.get() };
    let context = unsafe { &mut *candidate.context.get() };
    context.inherit_lifecycle(previous);
    if !clients.publish_clock(&clock, true) {
        return false;
    }
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
    runtime.clients = clients;
    runtime.io = candidate;
    true
}

unsafe extern "C" fn start(runtime: *mut c_void, now: u64) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    let mut clock = runtime.clock.clone();
    if !clock.start(now)
        || !runtime.clients.publish_clock_after(&clock, true, || {
            // SAFETY: registry control owns this runtime and drained every IO pin.
            // Keep the shared client clock hidden until native preparation succeeds.
            unsafe { mr_hal_io_begin_stream(runtime.io.context.get()) }
        })
    {
        return false;
    }
    runtime.clock = clock;
    unsafe { mr_hal_io_clock_state(runtime.io.context.get(), runtime.clock.rate, true) };
    true
}

unsafe extern "C" fn stop(runtime: *mut c_void) {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    runtime.clock.running = false;
    let _ = runtime.clients.publish_clock(&runtime.clock, false);
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
    // Queues keep their mappings and grants. Rearming advances every ring epoch and the
    // shared clock together, so readers discard each frame committed at the old rate.
    if !runtime.clients.publish_clock(&clock, true) {
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
    client: u32,
    frame: u64,
    samples: *mut f32,
    count: u32,
) -> IoOutcome {
    // SAFETY: the registry's IO pin retains this initialized native runtime.
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    let outcome = if render {
        // SAFETY: MixOutput supplies canonical Float32 PCM for this registered client.
        runtime.clients.render(
            client,
            frame,
            unsafe {
                std::slice::from_raw_parts(
                    samples,
                    count as usize
                        * runtime
                            .io
                            .context
                            .get()
                            .as_ref()
                            .map_or(0, |context| context.render_channels)
                            as usize,
                )
            },
            count,
        )
    } else {
        // SAFETY: HAL provides count * capture channels writable Float32 samples.
        runtime.clients.capture(
            frame,
            unsafe {
                std::slice::from_raw_parts_mut(
                    samples,
                    count as usize
                        * runtime
                            .io
                            .context
                            .get()
                            .as_ref()
                            .map_or(0, |context| context.capture_channels)
                            as usize,
                )
            },
            count,
        )
    };
    if outcome.accepted && count != 0 {
        // SAFETY: this pinned runtime retains the validated transport. Progress is
        // published after the client queue, independently of any postmix callback.
        unsafe {
            mixrelay_macos_transport::endpoint_transport::mr_endpoint_transport_clock_demand(
                runtime.io.transport,
                render,
                frame + u64::from(count),
                count,
            );
            if render {
                mr_hal_io_set_render_activity(runtime.io.context.get(), 3);
            }
        }
    }
    outcome
}

unsafe extern "C" fn client(
    runtime: *mut c_void,
    client: u32,
    action: u32,
    change: *mut c_void,
) -> bool {
    // SAFETY: registry control holds the device freeze, draining every IO pin.
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    // SAFETY: registration passes ownership of its prepared queue; removal passes null.
    let change = unsafe { take(change) };
    match crate::io_outcome::ClientChange::from_raw(action) {
        Some(crate::io_outcome::ClientChange::Register) => {
            let Some(Change::Client(prepared)) = change else {
                return false;
            };
            if prepared.id() != client || !runtime.clients.register(prepared) {
                return false;
            }
            if !runtime.clients.publish_clock(&runtime.clock, false) {
                runtime.clients.remove(client);
                return false;
            }
            true
        }
        Some(crate::io_outcome::ClientChange::Remove) => {
            runtime.clients.remove(client);
            true
        }
        _ => false,
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
        prepare,
        discard,
        replace,
        start,
        stop,
        remaining_client: remaining,
        set_rate,
        timestamp,
        prepare_client,
        client,
        io,
        requires_replacement,
        same_transport,
    };
    Registry::with_inspection(native, host, inspection)
}
