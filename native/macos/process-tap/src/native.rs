//! Direct CoreAudio/dispatch ABI. Only Objective-C message sends remain in the compiler shim.

use super::{TapError, TapSession};
use crate::process_tap_input::TapFormat;
use mixrelay_macos_transport::audio_buffer::AudioBufferList;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::{Mutex, OnceLock},
};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

    const fn output(selector: [u8; 4]) -> Self {
        Self {
            selector: u32::from_be_bytes(selector),
            scope: u32::from_be_bytes(*b"outp"),
            element: 0,
        }
    }

    const fn input(selector: [u8; 4]) -> Self {
        Self {
            selector: u32::from_be_bytes(selector),
            scope: u32::from_be_bytes(*b"inpt"),
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
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(object: *const c_void);
    fn mr_tap_description_supports_bundle_selection() -> bool;
    fn mr_tap_description_supports_device_stream_selection() -> bool;
    fn mr_tap_description_create(
        bundle: *const c_char,
        processes: *const u32,
        count: usize,
        target_device_uid: *const c_char,
        select_bundle: bool,
        mute: bool,
    ) -> *mut c_void;
    fn mr_tap_description_release(description: *mut c_void);
    fn mr_tap_aggregate_create(description: *mut c_void, device: *mut u32) -> i32;
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
    fn device(&self, uid: &CStr) -> Result<DeviceLease, TapError>;
    fn processes(
        &self,
        bundle: &CStr,
        target_device: Option<u32>,
    ) -> Result<Vec<ProcessIdentity>, TapError>;
    fn rebuild(&self, session: &TapSession, selection: &Selection) -> Result<(), TapError>;
    fn format(&self, tap: u32) -> Result<TapFormat, TapError>;
    fn start(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    fn stop(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    /// Adopts even an error-returned identifier before returning its status.
    fn register(&self, session: &TapSession) -> Result<(), TapError>;
    fn unregister(&self, device: u32, proc: *mut c_void) -> Result<(), TapError>;
    fn remove_listener(&self, tap: u32, session: &TapSession) -> Result<(), TapError>;
    fn remove_inventory_listener(&self, session: &TapSession) -> Result<(), TapError>;
    fn destroy_aggregate(&self, device: u32) -> Result<(), TapError>;
    fn destroy_tap(&self, tap: u32) -> Result<(), TapError>;
    /// One temporary, unmuted observation of exactly the same selection.
    fn diagnostic(
        &self,
        selection: &Selection,
        observed_at: std::time::Instant,
    ) -> Result<super::watchdog::Diagnostic, TapError>;
    fn diagnostic_has_audio(&self, diagnostic: &super::watchdog::Diagnostic) -> bool;
    fn retire_diagnostic(&self, diagnostic: super::watchdog::Diagnostic);
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
    fn device(&self, uid: &CStr) -> Result<DeviceLease, TapError> {
        device(uid)
    }
    fn processes(
        &self,
        bundle: &CStr,
        target_device: Option<u32>,
    ) -> Result<Vec<ProcessIdentity>, TapError> {
        processes(bundle, target_device)
    }
    fn rebuild(&self, session: &TapSession, selection: &Selection) -> Result<(), TapError> {
        let description = Description::create(selection)?;
        description.tap(session)?;
        add_listener(session)?;
        description.aggregate(session)?;
        Ok(())
    }
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
    fn remove_inventory_listener(&self, session: &TapSession) -> Result<(), TapError> {
        // SAFETY: the listener/context pair is identical to registration and the
        // session stays owned until removal and active-listener drain complete.
        TapError::status(unsafe {
            AudioObjectRemovePropertyListener(
                1,
                &Address::global(*b"prs#"),
                inventory_changed,
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
    fn diagnostic(
        &self,
        selection: &Selection,
        _observed_at: std::time::Instant,
    ) -> Result<super::watchdog::Diagnostic, TapError> {
        let preparation_started = std::time::Instant::now();
        let current = self.processes(
            &selection.bundle,
            selection.target_device.map(|lease| lease.object),
        )?;
        if selection.fallback && current != selection.selected {
            return Err(TapError::code(9));
        }
        if let (Some(uid), Some(expected)) = (&selection.target_device_uid, selection.target_device)
            && self.device(uid)? != expected
        {
            return Err(TapError::code(9));
        }
        if !current.iter().any(|process| {
            property::<u32>(process.object, *b"piro").is_ok_and(|running| running != 0)
        }) {
            return Err(TapError::code(3));
        }
        super::ffi::create_diagnostic(selection).map(|session| super::watchdog::Diagnostic {
            session,
            preparation_started,
            preparation_time: preparation_started.elapsed(),
        })
    }
    fn diagnostic_has_audio(&self, diagnostic: &super::watchdog::Diagnostic) -> bool {
        // SAFETY: the control watchdog retains this diagnostic until retirement.
        unsafe { &*diagnostic.session }
            .audible_callbacks
            .load(std::sync::atomic::Ordering::Relaxed)
            != 0
    }
    fn retire_diagnostic(&self, diagnostic: super::watchdog::Diagnostic) {
        // SAFETY: the control watchdog transfers its unique ownership exactly once.
        unsafe { super::ffi::mr_process_tap_destroy(diagnostic.session) };
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

/// A process object alone is not a stable application identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ProcessIdentity {
    pub object: u32,
    pub pid: i32,
}

/// One output device and its sole output stream at the time of tap admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeviceLease {
    pub object: u32,
    pub stream: u32,
}

#[derive(Debug, Clone)]
pub(super) struct Selection {
    pub bundle: CString,
    pub fallback: bool,
    pub selected: Vec<ProcessIdentity>,
    pub target_device_uid: Option<CString>,
    pub target_device: Option<DeviceLease>,
    pub mute: bool,
}

impl Selection {
    pub(super) fn new(config: &super::TapConfig, native: &impl Native) -> Result<Self, TapError> {
        // SAFETY: the public create boundary validated and retains this C string.
        let bundle = unsafe { CStr::from_ptr(config.bundle_id) }.to_owned();
        let target_device_uid = if config.target_device_uid.is_null() {
            None
        } else {
            // SAFETY: create validated the NUL-terminated owned UID.
            Some(unsafe { CStr::from_ptr(config.target_device_uid) }.to_owned())
        };
        let target_device = target_device_uid
            .as_deref()
            .map(|uid| native.device(uid))
            .transpose()?;
        // A mutable bundle selector can acquire other output routes. Muted taps
        // must bind exact process objects and revalidate their device lease.
        let fallback =
            target_device.is_some() || !unsafe { mr_tap_description_supports_bundle_selection() };
        if target_device.is_some()
            && !unsafe { mr_tap_description_supports_device_stream_selection() }
        {
            return Err(TapError::code(2));
        }
        Ok(Self {
            bundle,
            fallback,
            selected: Vec::new(),
            target_device_uid,
            target_device,
            mute: config.mute_while_captured,
        })
    }
}

impl Description {
    pub(super) fn create(selection: &Selection) -> Result<Self, TapError> {
        let processes: Vec<u32> = selection.selected.iter().map(|id| id.object).collect();
        // SAFETY: create validated UTF-8 input; process slice remains readable for this call.
        let pointer = unsafe {
            mr_tap_description_create(
                selection.bundle.as_ptr(),
                processes.as_ptr(),
                processes.len(),
                selection
                    .target_device_uid
                    .as_ref()
                    .map_or(ptr::null(), |uid| uid.as_ptr()),
                !selection.fallback,
                selection.mute,
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
        let result = unsafe { mr_tap_aggregate_create(self.0, &raw mut resources.aggregate) };
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
        unsafe { mr_tap_description_release(self.0) };
    }
}

pub(super) fn owned_endpoint_uid(uid: &CStr) -> bool {
    let bytes = uid.to_bytes();
    let Some(suffix) = bytes.strip_prefix(b"io.mixrelay.audio.endpoint.") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && suffix.iter().any(|byte| *byte != b'0')
}

fn object_ids(object: u32, address: Address) -> Result<Vec<u32>, TapError> {
    let mut size = 0;
    // SAFETY: initialized property address and writable byte count remain live.
    TapError::status(unsafe {
        AudioObjectGetPropertyDataSize(object, &raw const address, 0, ptr::null(), &raw mut size)
    })?;
    if !size.is_multiple_of(4) || size > 4096 * 4 {
        return Err(TapError::code(5));
    }
    let mut ids = Vec::new();
    ids.try_reserve_exact(size as usize / 4)
        .map_err(|_| TapError::code(6))?;
    ids.resize(size as usize / 4, 0_u32);
    if ids.is_empty() {
        return Ok(ids);
    }
    // SAFETY: the vector owns at least the queried byte extent; a growth race
    // is rejected using the returned size before any element is inspected.
    TapError::status(unsafe {
        AudioObjectGetPropertyData(
            object,
            &raw const address,
            0,
            ptr::null(),
            &raw mut size,
            ids.as_mut_ptr().cast(),
        )
    })?;
    if !size.is_multiple_of(4) || size as usize > ids.len() * 4 {
        return Err(TapError::code(9));
    }
    ids.truncate(size as usize / 4);
    Ok(ids)
}

fn device(uid: &CStr) -> Result<DeviceLease, TapError> {
    // SAFETY: CF copies the owned UTF-8 UID and returns one local reference.
    let expected = unsafe { CFStringCreateWithCString(ptr::null(), uid.as_ptr(), 0x0800_0100) };
    if expected.is_null() {
        return Err(TapError::code(6));
    }
    let mut object = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).expect("device ID fits native extent");
    // SAFETY: the qualifier contains one live CFString pointer and result owns one UInt32.
    let status = unsafe {
        AudioObjectGetPropertyData(
            1,
            &Address::global(*b"uidd"),
            u32::try_from(size_of::<*const c_void>()).expect("pointer extent fits UInt32"),
            ptr::from_ref(&expected).cast(),
            &raw mut size,
            ptr::from_mut(&mut object).cast(),
        )
    };
    let stream = (status == 0 && size as usize == size_of::<u32>() && object != 0)
        .then(|| object_ids(object, Address::output(*b"stm#")))
        .and_then(Result::ok)
        .filter(|streams| streams.len() == 1 && streams[0] != 0)
        .map(|streams| streams[0]);
    let valid = status == 0
        && size as usize == size_of::<u32>()
        && object != 0
        && cfstring_property_matches(object, Address::global(*b"uid "), expected)
        && stream.is_some();
    // SAFETY: translation and validation are complete; no native reference escapes.
    unsafe { CFRelease(expected) };
    stream
        .filter(|_| valid)
        .map(|stream| DeviceLease { object, stream })
        .ok_or(TapError::code(9))
}

fn process_uses_output_device(process: u32, target: u32) -> bool {
    object_ids(process, Address::output(*b"pdv#")).is_ok_and(|devices| devices.contains(&target))
}

fn processes(bundle: &CStr, target_device: Option<u32>) -> Result<Vec<ProcessIdentity>, TapError> {
    let ids = object_ids(1, Address::global(*b"prs#"))?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // SAFETY: owned UTF-8 C string remains live; CF copies it synchronously.
    let expected = unsafe { CFStringCreateWithCString(ptr::null(), bundle.as_ptr(), 0x0800_0100) };
    if expected.is_null() {
        return Err(TapError::code(6));
    }
    let mut selected = Vec::new();
    for object in ids {
        // Recheck both properties around the same object ID so a disappearing
        // process cannot be paired with the PID of an unrelated replacement.
        if process_matches(object, expected)
            && let Ok(pid) = property::<i32>(object, *b"ppid")
            && pid > 0
            && target_device.is_none_or(|device| process_uses_output_device(object, device))
            && process_matches(object, expected)
            && property::<i32>(object, *b"ppid").is_ok_and(|again| again == pid)
        {
            selected.push(ProcessIdentity { object, pid });
        }
    }
    // SAFETY: owned expected CFString no longer used after filtering.
    unsafe { CFRelease(expected) };
    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

/// Atomically invalidates delivery before the off-RT control owner rescans.
unsafe extern "C" fn inventory_changed(
    _: u32,
    _: u32,
    _: *const Address,
    context: *mut c_void,
) -> i32 {
    // SAFETY: registration retains the pinned session through removal and drain.
    let session = unsafe { &*context.cast::<TapSession>() };
    let _active = super::ffi::Active::enter(&session.active_listeners);
    session
        .inventory_epoch
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    session.lifecycle.changed();
    0
}

pub(super) fn add_inventory_listener(session: &TapSession) -> Result<(), TapError> {
    // SAFETY: pinned session is initialized before the listener is published.
    TapError::status(unsafe {
        AudioObjectAddPropertyListener(
            1,
            &Address::global(*b"prs#"),
            inventory_changed,
            context(session),
        )
    })?;
    let mut resources = session.resources.get();
    resources.inventory_listener = true;
    session.resources.set(resources);
    Ok(())
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

/// A `CFString` property as UTF-8, or `None` when absent, unreadable or longer than 1 KiB.
fn string_property(object: u32, address: Address) -> Option<String> {
    let mut value: *const c_void = ptr::null();
    let mut size =
        u32::try_from(size_of::<*const c_void>()).expect("pointer size fits native extent");
    // SAFETY: exclusive pointer output starts null; a non-null result is a +1 reference
    // released below on every path.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &raw const address,
            0,
            ptr::null(),
            &raw mut size,
            ptr::from_mut(&mut value).cast(),
        )
    };
    let mut text = None;
    finish_property(
        (status, size, value),
        |value| {
            let mut buffer = [0 as c_char; 1024];
            // SAFETY: the buffer is writable for its full length and CF null-terminates it.
            let copied =
                unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), 1024, 0x0800_0100) != 0 };
            if copied {
                // SAFETY: CFStringGetCString succeeded, so the buffer holds a C string.
                text = unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_str()
                    .ok()
                    .map(str::to_owned);
            }
            copied
        },
        |value| {
            // SAFETY: property output transfers a +1 reference even on partial failure.
            unsafe { CFRelease(value) };
        },
    );
    text
}

/// Every running process's use of audio devices, from `CoreAudio` process objects: which
/// device UIDs it is playing into (running output) and recording from (running input).
pub(super) fn device_uses() -> Result<Vec<super::DeviceUse>, TapError> {
    let mut uses = Vec::new();
    for process in object_ids(1, Address::global(*b"prs#"))? {
        let (Ok(process_id), Some(bundle_id)) = (
            property::<i32>(process, *b"ppid"),
            string_property(process, Address::global(*b"pbid")),
        ) else {
            continue;
        };
        let playing = property::<u32>(process, *b"piro").is_ok_and(|running| running != 0);
        let recording = property::<u32>(process, *b"piri").is_ok_and(|running| running != 0);
        let uids = |address: Address, active: bool| -> Vec<String> {
            if !active {
                return Vec::new();
            }
            object_ids(process, address)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|device| string_property(device, Address::global(*b"uid ")))
                .collect()
        };
        super::push_device_uses(
            &mut uses,
            process_id,
            &bundle_id,
            &uids(Address::output(*b"pdv#"), playing),
            &uids(Address::input(*b"pdv#"), recording),
        );
    }
    Ok(uses)
}

fn process_matches(process: u32, expected: *const c_void) -> bool {
    cfstring_property_matches(process, Address::global(*b"pbid"), expected)
}

fn cfstring_property_matches(object: u32, address: Address, expected: *const c_void) -> bool {
    let mut actual: *const c_void = ptr::null();
    let mut size =
        u32::try_from(size_of::<*const c_void>()).expect("pointer size fits native extent");
    // SAFETY: exclusive pointer output starts null. Every non-null +1 result is
    // adopted before interpreting either status or returned property extent.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &raw const address,
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
        dispatch_queue_create(c"io.mixrelay.capture.retired".as_ptr(), ptr::null())
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
