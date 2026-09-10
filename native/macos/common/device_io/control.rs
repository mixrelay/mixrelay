use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::atomic::Ordering::{Acquire, Release},
};

use super::{
    Connection, DeviceIo,
    native::{self, Properties, System},
};
use crate::{
    aggregate_group::tn_aggregate_group_ready,
    callback_clock::tn_callback_clock_reset,
    device_io::lifecycle::{Backend, Created},
};

struct Registration<'a> {
    io: &'a DeviceIo,
    connection: &'a mut Connection,
    context: NonNull<c_void>,
}

impl Backend for Registration<'_> {
    fn prepare(&mut self, expected: u32) -> Result<u32, i32> {
        let binding = native::resolve(&self.io.selection, &System).ok_or(-2)?;
        let rate = native::format(&self.io.selection, binding, &System).ok_or(-3)?;
        if expected != 0 && binding.device != expected {
            return Err(-5);
        }
        *self.connection = Connection { binding, rate };
        // SAFETY: lifecycle has no registration during prepare. The engine
        // control owner has acknowledged drain before reusing callback state.
        let callback = unsafe { &mut *self.io.callback.get() };
        callback.selection = native::buffer_selection(&self.io.selection, binding);
        callback.prime = true;
        // SAFETY: the preceding registration drained; atomic observers may remain.
        unsafe { tn_callback_clock_reset(ptr::from_ref(&self.io.clock).cast_mut()) };
        self.io.progress.invalid.store(false, Release);
        Ok(binding.device)
    }

    fn create(&mut self, device: u32) -> Created {
        let mut procedure = ptr::null_mut();
        // SAFETY: the stable allocation is retained even on partial creation;
        // callback accesses only its cell, queues and atomic publication fields.
        let status = unsafe {
            native::create(
                device,
                super::tn_device_io_process,
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
    control.lifecycle.connect(
        expected,
        &mut Registration {
            io,
            connection: &mut control.connection,
            context,
        },
    )
}

// SAFETY: sole control worker; owner remains allocated on failed unregister.
pub(super) unsafe fn disconnect(io: &DeviceIo) -> bool {
    // SAFETY: callback borrows only disjoint cells, queues and atomics.
    let control = unsafe { &mut *io.control.get() };
    control.lifecycle.disconnect(&mut Registration {
        io,
        connection: &mut control.connection,
        context: NonNull::from(io).cast(),
    })
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
        && native::alive(connection.binding.device, properties)
        && native::format(&io.selection, connection.binding, properties) == Some(connection.rate)
}

fn group_ready(io: &DeviceIo) -> bool {
    io.selection.group.is_null() || {
        // SAFETY: enclosing Native retains aggregate Arc throughout this query.
        unsafe { tn_aggregate_group_ready(io.selection.group.cast()) }
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
