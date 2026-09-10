//! Direct CoreAudio/dispatch ABI. Only Objective-C message sends remain in the compiler shim.

use super::{TapError, TapSession};
use crate::process_tap_input::TapFormat;
use std::{
    ffi::{c_char, c_void},
    ptr,
    sync::{Mutex, OnceLock},
};
use tonenari_macos_transport::audio_buffer::AudioBufferList;

#[repr(C)]
#[derive(Debug)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

impl Address {
    const fn global(selector: [u8; 4]) -> Self {
        Self {
            selector: u32::from_be_bytes(selector),
            scope: u32::from_be_bytes(*b"glob"),
            element: 0,
        }
    }
}

/// Native timestamp prefix and trailing SMPTE storage, matching `AudioTimeStamp`.
#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct Timestamp {
    pub sample: f64,
    pub host: u64,
    rate_scalar: f64,
    word_clock: u64,
    smpte: [u8; 24],
    pub flags: u32,
    reserved: u32,
}

const _: () = assert!(size_of::<Timestamp>() == 64);

type IoProc = unsafe extern "C" fn(
    u32,
    *const Timestamp,
    *const AudioBufferList,
    *const Timestamp,
    *mut AudioBufferList,
    *const Timestamp,
    *mut c_void,
) -> i32;
type Listener = unsafe extern "C" fn(u32, u32, *const Address, *mut c_void) -> i32;

unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        output: *mut c_void,
    ) -> i32;
    fn AudioObjectAddPropertyListener(
        object: u32,
        address: *const Address,
        listener: Listener,
        context: *mut c_void,
    ) -> i32;
    fn AudioObjectRemovePropertyListener(
        object: u32,
        address: *const Address,
        listener: Listener,
        context: *mut c_void,
    ) -> i32;
    fn AudioDeviceCreateIOProcID(
        device: u32,
        callback: IoProc,
        context: *mut c_void,
        proc: *mut *mut c_void,
    ) -> i32;
    fn AudioDeviceDestroyIOProcID(device: u32, proc: *mut c_void) -> i32;
    fn AudioDeviceStart(device: u32, proc: *mut c_void) -> i32;
    fn AudioDeviceStop(device: u32, proc: *mut c_void) -> i32;
    fn AudioHardwareCreateProcessTap(description: *const c_void, tap: *mut u32) -> i32;
    fn AudioHardwareDestroyProcessTap(tap: u32) -> i32;
    fn AudioHardwareDestroyAggregateDevice(device: u32) -> i32;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        string: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFEqual(left: *const c_void, right: *const c_void) -> u8;
    fn CFRelease(object: *const c_void);
    fn tn_tap_description_supports_bundle_selection() -> bool;
    fn tn_tap_description_create(
        bundle: *const c_char,
        processes: *const u32,
        count: usize,
        mute: bool,
    ) -> *mut c_void;
    fn tn_tap_description_release(description: *mut c_void);
    fn tn_tap_aggregate_create(description: *mut c_void, device: *mut u32) -> i32;
    fn dispatch_queue_create(label: *const c_char, attribute: *const c_void) -> *mut c_void;
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_after_f(
        when: u64,
        queue: *mut c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_time(when: u64, delta: i64) -> u64;
}

pub(super) trait Native {
    fn format(&self, tap: u32) -> Result<TapFormat, TapError>;
    fn start(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    fn stop(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    /// Adopts even an error-returned identifier before returning its status.
    fn register(&self, session: &TapSession) -> Result<(), TapError>;
    fn unregister(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    fn remove_listener(&self, tap: u32, session: &TapSession) -> Result<(), TapError>;
    fn destroy_aggregate(&self, device: u32) -> Result<(), TapError>;
    fn destroy_tap(&self, tap: u32) -> Result<(), TapError>;
}

pub(super) struct CoreAudio;

fn context(session: &TapSession) -> *mut c_void {
    std::ptr::from_ref(session).cast_mut().cast()
}

fn property<T: Default>(object: u32, selector: [u8; 4]) -> Result<T, TapError> {
    let mut value = T::default();
    let mut size = u32::try_from(size_of::<T>()).expect("native property fits UInt32");
    // SAFETY: typed output owns size bytes; address/size live through synchronous query.
    TapError::status(unsafe {
        AudioObjectGetPropertyData(
            object,
            &Address::global(selector),
            0,
            ptr::null(),
            &raw mut size,
            ptr::from_mut(&mut value).cast(),
        )
    })?;
    if size as usize != size_of::<T>() {
        return Err(TapError::code(5));
    }
    Ok(value)
}

impl Native for CoreAudio {
    fn format(&self, tap: u32) -> Result<TapFormat, TapError> {
        // TapFormat's trailing alignment padding corresponds to ASBD.mReserved.
        property(tap, *b"tfmt")
    }
    fn start(&self, device: u32, proc: *mut c_void) -> Result<(), TapError> {
        // SAFETY: control owner retains the matching native registration.
        TapError::status(unsafe { AudioDeviceStart(device, proc) })
    }
    fn stop(&self, device: u32, proc: *mut c_void) -> Result<(), TapError> {
        // SAFETY: only the serialized control owner starts/stops this registration.
        TapError::status(unsafe { AudioDeviceStop(device, proc) })
    }
    fn register(&self, session: &TapSession) -> Result<(), TapError> {
        let mut proc = ptr::null_mut();
        let resources = session.resources.get();
        // SAFETY: heap-pinned session is retained until unregister and callback drain.
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                resources.aggregate,
                super::ffi::io_proc,
                context(session),
                &raw mut proc,
            )
        };
        session.adopt_registration(proc, status)
    }
    fn unregister(&self, device: u32, proc: *mut c_void) -> Result<(), TapError> {
        // SAFETY: identifier is cleared only after this succeeds; failures retain owner.
        TapError::status(unsafe { AudioDeviceDestroyIOProcID(device, proc) })
    }
    fn remove_listener(&self, tap: u32, session: &TapSession) -> Result<(), TapError> {
        // SAFETY: exact callback/context tuple matches registration and is still retained.
        TapError::status(unsafe {
            AudioObjectRemovePropertyListener(
                tap,
                &Address::global(*b"tfmt"),
                format_changed,
                context(session),
            )
        })
    }
    fn destroy_aggregate(&self, device: u32) -> Result<(), TapError> {
        // SAFETY: control owner removed IOProc before destroying its aggregate.
        TapError::status(unsafe { AudioHardwareDestroyAggregateDevice(device) })
    }
    fn destroy_tap(&self, tap: u32) -> Result<(), TapError> {
        // SAFETY: aggregate and listener are gone before their underlying tap.
        TapError::status(unsafe { AudioHardwareDestroyProcessTap(tap) })
    }
}

unsafe extern "C" fn format_changed(
    _: u32,
    _: u32,
    _: *const Address,
    context: *mut c_void,
) -> i32 {
    // SAFETY: registration retains session until removal succeeds and active listeners drain.
    let session = unsafe { &*context.cast::<TapSession>() };
    let _active = super::ffi::Active::enter(&session.active_listeners);
    // SAFETY: listener touches only the initialized atomic invalidation gate.
    session.lifecycle.changed();
    0
}

pub(super) struct Description(*mut c_void);

impl Description {
    pub(super) fn create(config: &super::TapConfig) -> Result<Self, TapError> {
        // SAFETY: helper only checks Objective-C runtime selector availability.
        let processes = if unsafe { tn_tap_description_supports_bundle_selection() } {
            Vec::new()
        } else {
            processes(config.bundle_id)?
        };
        // SAFETY: create validated UTF-8 input; process slice remains readable for this call.
        let pointer = unsafe {
            tn_tap_description_create(
                config.bundle_id,
                processes.as_ptr(),
                processes.len(),
                config.mute_while_captured,
            )
        };
        if pointer.is_null() {
            Err(TapError::code(6))
        } else {
            Ok(Self(pointer))
        }
    }

    pub(super) fn tap(&self, session: &TapSession) -> Result<(), TapError> {
        let mut resources = session.resources.get();
        // SAFETY: retained CATapDescription and exclusive local result storage.
        let result = unsafe { AudioHardwareCreateProcessTap(self.0, &raw mut resources.tap) };
        session.resources.set(resources);
        TapError::status(result)
    }

    pub(super) fn aggregate(&self, session: &TapSession) -> Result<(), TapError> {
        let mut resources = session.resources.get();
        // SAFETY: shim constructs only Objective-C dictionary; Rust retains returned owner on failure.
        let result = unsafe { tn_tap_aggregate_create(self.0, &raw mut resources.aggregate) };
        session.resources.set(resources);
        TapError::status(result)?;
        for _ in 0..500 {
            if property::<u32>(resources.aggregate, *b"livn").is_ok_and(|alive| alive != 0) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Err(TapError::code(4))
    }
}

impl Drop for Description {
    fn drop(&mut self) {
        // SAFETY: helper consumes the +1 retained Objective-C description exactly once.
        unsafe { tn_tap_description_release(self.0) };
    }
}

fn processes(bundle: *const c_char) -> Result<Vec<u32>, TapError> {
    let address = Address::global(*b"prs#");
    let mut size = 0;
    // SAFETY: initialized address and exclusive native-size output.
    TapError::status(unsafe {
        AudioObjectGetPropertyDataSize(1, &raw const address, 0, ptr::null(), &raw mut size)
    })?;
    if size == 0 || !size.is_multiple_of(4) || size > 4096 * 4 {
        return Err(TapError::code(3));
    }
    let mut ids = Vec::new();
    ids.try_reserve_exact(size as usize / 4)
        .map_err(|_| TapError::code(6))?;
    ids.resize(size as usize / 4, 0_u32);
    // SAFETY: allocation covers queried byte extent; OS may shrink returned size.
    TapError::status(unsafe {
        AudioObjectGetPropertyData(
            1,
            &raw const address,
            0,
            ptr::null(),
            &raw mut size,
            ids.as_mut_ptr().cast(),
        )
    })?;
    if !size.is_multiple_of(4) || size as usize > ids.len() * 4 {
        return Err(TapError::code(4));
    }
    ids.truncate(size as usize / 4);
    // SAFETY: startup validated a readable UTF-8 C string; CF copies it synchronously.
    let expected = unsafe { CFStringCreateWithCString(ptr::null(), bundle, 0x0800_0100) };
    if expected.is_null() {
        return Err(TapError::code(6));
    }
    ids.retain(|id| process_matches(*id, expected));
    // SAFETY: owned expected CFString no longer used after filtering.
    unsafe { CFRelease(expected) };
    if ids.is_empty() {
        Err(TapError::code(3))
    } else {
        Ok(ids)
    }
}

pub(super) fn add_listener(session: &TapSession) -> Result<(), TapError> {
    let mut resources = session.resources.get();
    // SAFETY: pinned session initialized before publication; failed removal retains it.
    TapError::status(unsafe {
        AudioObjectAddPropertyListener(
            resources.tap,
            &Address::global(*b"tfmt"),
            format_changed,
            context(session),
        )
    })?;
    resources.listener = true;
    session.resources.set(resources);
    Ok(())
}

fn process_matches(process: u32, expected: *const c_void) -> bool {
    let mut actual: *const c_void = ptr::null();
    let mut size =
        u32::try_from(size_of::<*const c_void>()).expect("pointer size fits native extent");
    // SAFETY: exclusive pointer output starts null. Every non-null +1 result is
    // adopted before interpreting either status or returned property extent.
    let status = unsafe {
        AudioObjectGetPropertyData(
            process,
            &Address::global(*b"pbid"),
            0,
            ptr::null(),
            &raw mut size,
            ptr::from_mut(&mut actual).cast(),
        )
    };
    finish_property(
        (status, size, actual),
        |actual| {
            // SAFETY: only successful correctly sized CFString output reaches comparison.
            unsafe { CFEqual(expected, actual) != 0 }
        },
        |actual| {
            // SAFETY: property output transfers a +1 reference even on partial failure.
            unsafe { CFRelease(actual) };
        },
    )
}

pub(super) fn finish_property(
    result: (i32, u32, *const c_void),
    compare: impl FnOnce(*const c_void) -> bool,
    release: impl FnMut(*const c_void),
) -> bool {
    struct Owned<F: FnMut(*const c_void)> {
        value: *const c_void,
        release: F,
    }
    impl<F: FnMut(*const c_void)> Drop for Owned<F> {
        fn drop(&mut self) {
            (self.release)(self.value);
        }
    }
    let (status, size, value) = result;
    if value.is_null() {
        return false;
    }
    let owner = Owned { value, release };
    status == 0 && size as usize == size_of::<*const c_void>() && compare(owner.value)
}

pub(super) fn checked_queue(create: impl FnOnce() -> *mut c_void) -> Result<usize, TapError> {
    let queue = create();
    if queue.is_null() {
        Err(TapError::code(6))
    } else {
        Ok(queue as usize)
    }
}

pub(super) fn prepare_cleanup() -> Result<usize, TapError> {
    static QUEUE: OnceLock<usize> = OnceLock::new();
    static INITIALIZE: Mutex<()> = Mutex::new(());
    if let Some(queue) = QUEUE.get() {
        return Ok(*queue);
    }
    let _guard = INITIALIZE.lock().map_err(|_| TapError::code(9))?;
    if let Some(queue) = QUEUE.get() {
        return Ok(*queue);
    }
    // SAFETY: static NUL-terminated label; successful serial queue is process-lived.
    let queue = checked_queue(|| unsafe {
        dispatch_queue_create(c"io.tonenari.capture.retired".as_ptr(), ptr::null())
    })?;
    QUEUE
        .set(queue)
        .expect("queue initialization lock has one publisher");
    Ok(queue)
}

pub(super) fn retire(session: *mut TapSession, retry: bool) {
    // SAFETY: creator acquired this non-null queue before any native publication;
    // queue outlives every admitted session and the retiring owner remains live.
    let queue = unsafe { &*session }.retirement_queue;
    // SAFETY: this queue exclusively owns the live session until cleanup/retry completes.
    unsafe { enqueue(queue, session.cast(), cleanup, retry) };
}

// SAFETY: queue is a successfully created process-lived serial queue. Context
// remains allocated until work consumes it or explicitly schedules its next retry.
pub(super) unsafe fn enqueue(
    queue: usize,
    context: *mut c_void,
    work: unsafe extern "C" fn(*mut c_void),
    retry: bool,
) {
    // SAFETY: destroy transfers exclusive retirement responsibility to this queue.
    // On failure the same owner is requeued, never copied or freed prematurely.
    unsafe {
        if retry {
            dispatch_after_f(
                dispatch_time(0, 1_000_000_000),
                queue as *mut c_void,
                context,
                work,
            );
        } else {
            dispatch_async_f(queue as *mut c_void, context, work);
        }
    }
}

unsafe extern "C" fn cleanup(context: *mut c_void) {
    let pointer = context.cast::<TapSession>();
    // SAFETY: single serial queue owns the allocation until reclaim returns true.
    if unsafe { &*pointer }.reclaim(&CoreAudio) {
        // SAFETY: every native handle removed and callbacks/listeners drained; caller
        // transferred pump/observer ownership before asynchronous destroy.
        drop(unsafe { Box::from_raw(pointer) });
    } else {
        retire(pointer, true);
    }
}
