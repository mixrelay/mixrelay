use std::ffi::{CStr, c_char, c_void};
use std::os::fd::{FromRawFd, OwnedFd};

use crate::registry::Registry;

pub(crate) const MAX_DEVICES: usize = 64;
pub(crate) const CONFIG_ACTION: u64 = 0x4d52_6368;
pub(crate) const OK: i32 = 0;
pub(crate) const INVALID: i32 = -1;
pub(crate) const NOT_FOUND: i32 = -2;
pub(crate) const NATIVE_ERROR: i32 = -4;
pub(crate) const CAPACITY: i32 = -5;
pub(crate) const LIST: u32 = 1;
pub(crate) const NAME: u32 = 2;
pub(crate) const INPUT_LATENCY: u32 = 10;
pub(crate) const FORMAT: u32 = 3;
pub(crate) const ALIVE: u32 = 4;
pub(crate) const RUNNING: u32 = 5;
pub(crate) const STREAM_ACTIVE: u32 = 6;
pub(crate) const OUTPUT_VOLUME: u32 = 7;
pub(crate) const OUTPUT_MUTE: u32 = 8;
pub(crate) const TRANSPORT: u32 = 9;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub endpoint_id: [u8; 16],
    pub render_channels: u32,
    pub capture_channels: u32,
}

impl Geometry {
    pub(crate) fn valid(self) -> bool {
        self.endpoint_id != [0; 16]
            && self.render_channels <= 64
            && self.capture_channels <= 64
            && self.render_channels + self.capture_channels > 0
    }
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct Timestamp {
    pub sample_time: u64,
    pub host_time: u64,
    pub seed: u64,
}

#[repr(C)]
pub(crate) struct Snapshot {
    pub device_id: u32,
    pub input_stream_id: u32,
    pub output_stream_id: u32,
    pub sample_rate: u32,
    pub render_channels: u32,
    pub capture_channels: u32,
    pub client_count: u32,
    pub alive: bool,
    pub running: bool,
    pub input_active: bool,
    pub output_active: bool,
    pub name: [u8; 128],
    pub uid: [u8; 80],
    pub capture_format: u32,
    pub render_format: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeOps {
    pub inspect: unsafe extern "C" fn(i32, usize, *mut Geometry) -> bool,
    pub create: unsafe extern "C" fn(*const Geometry, u32, i32, usize) -> *mut c_void,
    pub destroy: unsafe extern "C" fn(*mut c_void),
    pub replace: unsafe extern "C" fn(*mut c_void, *const Geometry, u32, i32, usize) -> bool,
    pub start: unsafe extern "C" fn(*mut c_void, u64) -> bool,
    pub stop: unsafe extern "C" fn(*mut c_void),
    pub remaining_client: unsafe extern "C" fn(*mut c_void),
    pub set_rate: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    pub timestamp: unsafe extern "C" fn(*mut c_void, u64, *mut Timestamp) -> bool,
    pub io: unsafe extern "C" fn(*mut c_void, bool, u64, *mut f32, u32) -> bool,
    pub requires_replacement: unsafe extern "C" fn(*mut c_void) -> bool,
    pub same_transport: unsafe extern "C" fn(*mut c_void, i32, usize) -> bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct HostOps {
    pub context: *mut c_void,
    pub changed: unsafe extern "C" fn(*mut c_void, u32, u32),
    pub request_configuration: unsafe extern "C" fn(*mut c_void, u32, u64, u64) -> bool,
}

// The host promises thread-safe callbacks and keeps context alive until destroy.
unsafe impl Send for HostOps {}
unsafe impl Sync for HostOps {}

impl HostOps {
    pub(crate) fn changed(self, object: u32, change: u32) {
        unsafe { (self.changed)(self.context, object, change) };
    }

    pub(crate) fn request(self, object: u32, generation: u64) -> bool {
        unsafe { (self.request_configuration)(self.context, object, CONFIG_ACTION, generation) }
    }
}

/// Scopes SDK IO entries, including nested/reentrant native callbacks.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn mr_hal_core_enter_audio_thread() {
    crate::transport::enter_audio_thread();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn mr_hal_core_leave_audio_thread() {
    crate::transport::leave_audio_thread();
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_create(
    native: *const NativeOps,
    host: *const HostOps,
) -> *mut Registry {
    if native.is_null() || host.is_null() {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Registry::new(unsafe { *native }, unsafe {
        *host
    })))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_destroy(registry: *mut Registry) {
    if !registry.is_null() {
        drop(unsafe { Box::from_raw(registry) });
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_upsert(
    registry: *mut Registry,
    endpoint: *const [u8; 16],
    name: *const c_char,
    rate: u32,
    fd: i32,
    size: usize,
    object: *mut u32,
) -> i32 {
    if fd < 0 {
        return INVALID;
    }
    // Ownership crosses the ABI even if another argument is invalid.
    let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return INVALID;
    };
    let Some(endpoint) = (unsafe { endpoint.as_ref() }) else {
        return INVALID;
    };
    if name.is_null() || object.is_null() {
        return INVALID;
    }
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return INVALID;
    };
    match registry.upsert(*endpoint, name, rate, (descriptor, size)) {
        Ok(id) => {
            unsafe { *object = id };
            OK
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_remove(
    registry: *mut Registry,
    endpoint: *const [u8; 16],
) -> i32 {
    match (unsafe { registry.as_ref() }, unsafe { endpoint.as_ref() }) {
        (Some(registry), Some(endpoint)) => registry.remove(*endpoint),
        _ => INVALID,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_tick(registry: *mut Registry) {
    if let Some(registry) = unsafe { registry.as_ref() } {
        registry.tick();
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_devices(
    registry: *mut Registry,
    output: *mut u32,
    capacity: usize,
) -> usize {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return 0;
    };
    let devices = registry.devices();
    if !output.is_null() {
        let count = devices.len().min(capacity);
        unsafe { std::ptr::copy_nonoverlapping(devices.as_ptr(), output, count) };
    }
    devices.len()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_lookup_uid(
    registry: *mut Registry,
    uid: *const c_char,
) -> u32 {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return 0;
    };
    if uid.is_null() {
        return 0;
    }
    let Ok(uid) = unsafe { CStr::from_ptr(uid) }.to_str() else {
        return 0;
    };
    registry.lookup_uid(uid).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_device_alive(
    registry: *const Registry,
    device: u32,
) -> bool {
    unsafe { registry.as_ref() }.is_some_and(|registry| registry.device_alive(device))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_snapshot(
    registry: *mut Registry,
    object: u32,
    output: *mut Snapshot,
) -> bool {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return false;
    };
    if output.is_null() {
        return false;
    }
    let Some(snapshot) = registry.snapshot(object) else {
        return false;
    };
    unsafe { output.write(snapshot) };
    true
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_client(
    registry: *mut Registry,
    device: u32,
    client: u32,
    add: bool,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| registry.client(device, client, add))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_client_identity(
    registry: *mut Registry,
    device: u32,
    client: u32,
    pid: i32,
    bundle: *const c_char,
    add: bool,
) -> i32 {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return INVALID;
    };
    // Copy callback-owned storage before entering registry control or returning.
    let bundle = if bundle.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(bundle) }
            .to_str()
            .ok()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    let _gate = crate::device::lock(&registry.client_identity_gate);
    let status = registry.client(device, client, add);
    if status == OK {
        if add {
            registry.client_pid(device, client, u32::try_from(pid).unwrap_or(0));
            registry.transport.client(device, client, pid, bundle);
        } else {
            registry.transport.remove(device, client);
        }
    }
    status
}

/// Marks a render client from IO without taking control locks.
/// # Safety
/// The registry is live for the synchronous audio callback.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_render_client(
    registry: *const Registry,
    device: u32,
    client: u32,
) {
    if let Some(registry) = unsafe { registry.as_ref() } {
        registry.render_client(device, client);
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_start(
    registry: *mut Registry,
    device: u32,
    client: u32,
    now: u64,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| registry.start(device, client, now))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_stop(
    registry: *mut Registry,
    device: u32,
    client: u32,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| registry.stop(device, client))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_set_rate(
    registry: *mut Registry,
    device: u32,
    rate: u32,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| registry.set_rate(device, rate))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_set_stream_active(
    registry: *mut Registry,
    stream: u32,
    active: bool,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| {
        registry.set_stream_active(stream, active)
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_perform(
    registry: *mut Registry,
    device: u32,
    action: u64,
    generation: u64,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| {
        registry.perform(device, action, generation)
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_abort(
    registry: *mut Registry,
    device: u32,
    action: u64,
    generation: u64,
) -> i32 {
    unsafe { registry.as_ref() }.map_or(INVALID, |registry| {
        registry.abort(device, action, generation)
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_timestamp(
    registry: *mut Registry,
    device: u32,
    now: u64,
    output: *mut Timestamp,
) -> bool {
    let (Some(registry), Some(output)) = (unsafe { registry.as_ref() }, unsafe { output.as_mut() })
    else {
        return false;
    };
    registry.timestamp(device, now, output)
}

#[unsafe(no_mangle)]
/// Processes an exclusively writable `CoreAudio` operation buffer in place.
///
/// # Safety
/// The registry and buffer remain live for this synchronous call. `samples`
/// contains `frames * stream_channels` exclusively writable samples in the current
/// native stream format. Float32 buffers must be float-aligned; packed integer
/// buffers require only byte alignment.
/// The SDK's `WriteMix` operation supplies this main buffer for in-place use;
/// engine/shared transport storage must never be supplied as this buffer.
pub(crate) unsafe extern "C" fn mr_hal_core_io(
    registry: *mut Registry,
    device: u32,
    stream: u32,
    render: bool,
    frame: u64,
    samples: *mut c_void,
    frames: u32,
) -> bool {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return false;
    };
    if samples.is_null() || frames as usize > crate::formats::MAX_BUFFER_FRAMES {
        return false;
    }
    unsafe {
        registry.io(crate::registry::IoOperation {
            device,
            stream,
            render,
            frame,
            samples,
            frames,
        })
    }
}
