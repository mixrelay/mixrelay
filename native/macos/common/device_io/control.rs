use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            Ordering::{Acquire, Release},
        },
    },
};

use super::{
    Connection, DeviceIo,
    listeners::{Listeners, WatchTarget},
    native::{self, Properties, System},
};
use crate::{
    aggregate_group::mr_aggregate_group_ready,
    callback_clock::mr_callback_clock_reset,
    device_io::lifecycle::{Backend, Created},
};

struct Registration<'a> {
    io: &'a DeviceIo,
    connection: &'a mut Connection,
    context: NonNull<c_void>,
    listeners: &'a mut Listeners,
}

impl Backend for Registration<'_> {
    fn prepare(&mut self, expected: u32) -> Result<u32, i32> {
        let binding = native::resolve(&self.io.selection, &System).ok_or(-2)?;
        if expected != 0 && binding.device != expected {
            return Err(-5);
        }
        if !self.listeners.remove(&System) {
            return Err(-6);
        }
        // A fresh registration owns its latch. Late copies of removed native blocks cannot
        // invalidate the next registration. Metadata invalidation is reset only after drain.
        self.io.progress.invalid.store(false, Release);
        let watch_invalid = Arc::new(AtomicBool::new(false));
        self.listeners.install(
            &WatchTarget {
                device: binding.device,
                scope: u32::from_be_bytes(if self.io.selection.input {
                    *b"inpt"
                } else {
                    *b"outp"
                }),
                limit: if self.io.selection.group.is_null() {
                    64
                } else {
                    2048
                },
            },
            &watch_invalid,
            &System,
        )?;
        let format = native::admitted_format(&self.io.selection, binding, &System).ok_or(-3)?;
        if watch_invalid.load(Acquire) {
            return Err(-6);
        }
        *self.connection = Connection {
            binding,
            rate: format.rate,
            formats: format.layout.clone(),
            watch_invalid: watch_invalid.clone(),
        };
        // SAFETY: lifecycle has no registration during prepare. The engine
        // control owner has acknowledged drain before reusing callback state.
        let callback = unsafe { &mut *self.io.callback.get() };
        callback.selection = native::buffer_selection(&self.io.selection, binding);
        callback.formats = format.layout;
        callback.watch_invalid = watch_invalid;
        callback.prime = true;
        // SAFETY: the preceding registration drained; atomic observers may remain.
        unsafe { mr_callback_clock_reset(ptr::from_ref(&self.io.clock).cast_mut()) };
        Ok(binding.device)
    }

    fn create(&mut self, device: u32) -> Created {
        let mut procedure = ptr::null_mut();
        // SAFETY: the stable allocation is retained even on partial creation;
        // callback accesses only its cell, queues and atomic publication fields.
        let status = unsafe {
            native::create(
                device,
                super::mr_device_io_process,
                self.context.as_ptr(),
                &raw mut procedure,
            )
        };
        Created {
            procedure: NonNull::new(procedure),
            status,
        }
    }

    fn start(&mut self, device: u32, procedure: NonNull<c_void>) -> i32 {
        if self.io.progress.invalid.load(Acquire) || self.connection.watch_invalid.load(Acquire) {
            return -6;
        }
        // SAFETY: this pair came from create; no callback state mutable borrow survives.
        unsafe { native::start(device, procedure.as_ptr()) }
    }

    fn silence(&mut self) {
        self.io.progress.invalid.store(true, Release);
    }

    fn stop(&mut self, device: u32, procedure: NonNull<c_void>) -> i32 {
        // SAFETY: registration and allocation remain retained regardless of stop status.
        unsafe { native::stop(device, procedure.as_ptr()) }
    }

    fn destroy(&mut self, device: u32, procedure: NonNull<c_void>) -> i32 {
        // SAFETY: stop attempted; only lifecycle acknowledges confirmed native release.
        unsafe { native::destroy(device, procedure.as_ptr()) }
    }
}

// SAFETY: one engine control worker excludes other control calls for this owner.
// Callbacks do not touch Control, so borrowing that cell does not alias their state.
pub(super) unsafe fn connect(io: &DeviceIo, expected: u32) -> i32 {
    // SAFETY: serialized control access; no whole-DeviceIo mutable reference is formed.
    let control = unsafe { &mut *io.control.get() };
    let context = NonNull::from(io).cast();
    let status = control.lifecycle.connect(
        expected,
        &mut Registration {
            io,
            connection: &mut control.connection,
            context,
            listeners: &mut control.listeners,
        },
    );
    if status != 0 && !control.lifecycle.registered() {
        io.progress.invalid.store(true, Release);
        control.listeners.remove(&System);
    }
    status
}

// SAFETY: sole control worker; owner remains allocated on failed unregister.
pub(super) unsafe fn disconnect(io: &DeviceIo) -> bool {
    // SAFETY: callback borrows only disjoint cells, queues and atomics.
    let control = unsafe { &mut *io.control.get() };
    let drained = control.lifecycle.disconnect(&mut Registration {
        io,
        connection: &mut control.connection,
        context: NonNull::from(io).cast(),
        listeners: &mut control.listeners,
    });
    if !drained {
        return false;
    }
    io.progress.invalid.store(true, Release);
    control.listeners.remove(&System)
}

// SAFETY: only control accesses registration/binding/rate. Native property
// queries do not mutate this owner or grant callback access to Control.
pub(super) unsafe fn alive(io: &DeviceIo, properties: &impl Properties) -> bool {
    // SAFETY: sole control worker excludes connect/disconnect until return.
    let control = unsafe { &*io.control.get() };
    if !group_ready(io) {
        return false;
    }
    let connection = &control.connection;
    if io.selection.uid.len() == 1
        && native::default_device(io.selection.input, properties) != Some(connection.binding.device)
    {
        return false;
    }
    control.lifecycle.registered()
        && !io.progress.invalid.load(Acquire)
        && !connection.watch_invalid.load(Acquire)
        && native::alive(connection.binding.device, properties)
        && native::admitted_format(&io.selection, connection.binding, properties).is_some_and(
            |format| format.rate == connection.rate && format.layout == connection.formats,
        )
}

fn group_ready(io: &DeviceIo) -> bool {
    io.selection.group.is_null() || {
        // SAFETY: enclosing Native retains aggregate Arc throughout this query.
        unsafe { mr_aggregate_group_ready(io.selection.group.cast()) }
    }
}

// SAFETY: this query runs on the single control owner, excluding registration changes.
pub(super) unsafe fn clock_device(io: &DeviceIo, properties: &impl Properties) -> u32 {
    // SAFETY: callbacks never access the control cell.
    let control = unsafe { &*io.control.get() };
    if !control.lifecycle.registered() {
        return 0;
    }
    let device = control.connection.binding.device;
    if !io.selection.group.is_null() {
        return if group_ready(io) { device } else { 0 };
    }
    native::clock_device(device, properties)
}
