use std::{
    ffi::{c_char, c_long, c_void},
    ptr::{self, NonNull},
};

use super::Backend;
#[cfg(test)]
mod tests;

type Object = *mut c_void;
const INTERVAL_NS: u64 = 10_000_000;
const LEEWAY_NS: u64 = 1_000_000;

unsafe extern "C" {
    static _dispatch_source_type_timer: c_char;
    fn xpc_connection_create_mach_service(name: *const c_char, queue: Object, flags: u64)
    -> Object;
    fn xpc_connection_activate(connection: Object);
    fn xpc_connection_cancel(connection: Object);
    fn xpc_release(object: Object);
    fn dispatch_get_global_queue(identifier: c_long, flags: usize) -> Object;
    fn dispatch_source_create(
        kind: *const c_char,
        handle: usize,
        mask: usize,
        queue: Object,
    ) -> Object;
    fn dispatch_time(when: u64, delta: i64) -> u64;
    fn dispatch_source_set_timer(timer: Object, start: u64, interval: u64, leeway: u64);
    fn dispatch_source_set_event_handler_f(source: Object, handler: extern "C" fn(Object));
    fn dispatch_activate(object: Object);
    fn dispatch_source_cancel(source: Object);
    fn dispatch_release(object: Object);
    fn tn_engine_dispatch_install_handler(listener: Object);
    fn tn_engine_dispatch_install_discard_handler(listener: Object);
}

pub(in super::super) struct Native;
pub(in super::super) struct Listener(NonNull<c_void>);
pub(in super::super) struct Timer(NonNull<c_void>);

// SAFETY: these unique +1 XPC/dispatch references permit control-thread transfer.
// Dispatch serializes activation/release; callbacks capture neither wrapper nor
// their owner. The native objects themselves support thread-safe cancellation.
unsafe impl Send for Listener {}
// SAFETY: the same ownership rule applies to the timer; its handler uses only the
// process-lifetime SERVICE and never accesses this wrapper's storage.
unsafe impl Send for Timer {}

extern "C" fn control_tick(_context: Object) {
    super::super::xpc::tn_engine_service_control_tick();
}

impl Backend for Native {
    type Listener = Listener;
    type Timer = Timer;

    fn listener(&self) -> Option<Listener> {
        // SAFETY: static NUL-terminated service name; null selects XPC's default
        // queue and flag 1 creates a listener. A non-null result owns one reference.
        let listener = unsafe {
            xpc_connection_create_mach_service(
                c"io.tonenari.audio.engine".as_ptr(),
                ptr::null_mut(),
                1,
            )
        };
        NonNull::new(listener).map(Listener)
    }

    fn timer(&self) -> Option<Timer> {
        // SAFETY: the SDK's immortal timer type/global queue are borrowed. The
        // source's event context defaults to null; control_tick ignores it.
        let timer = unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            NonNull::new(dispatch_source_create(
                &raw const _dispatch_source_type_timer,
                0,
                0,
                queue,
            ))?
        };
        // SAFETY: creation returned a unique suspended source. Configuration is
        // completed before activation, with a static C-ABI callback and no context.
        unsafe {
            dispatch_source_set_timer(
                timer.as_ptr(),
                dispatch_time(0, 10_000_000),
                INTERVAL_NS,
                LEEWAY_NS,
            );
            dispatch_source_set_event_handler_f(timer.as_ptr(), control_tick);
        }
        Some(Timer(timer))
    }

    fn activate(&self, listener: &Listener, timer: &Timer) {
        // SAFETY: Dispatch invokes this once after both creations succeed. The
        // copied XPC Blocks call static Rust entry points without capturing owners.
        unsafe {
            tn_engine_dispatch_install_handler(listener.0.as_ptr());
            xpc_connection_activate(listener.0.as_ptr());
            dispatch_activate(timer.0.as_ptr());
        }
    }

    fn cancel_listener(&self, listener: &Listener) {
        // SAFETY: only the activated Dispatch::drop calls this, before release.
        unsafe { xpc_connection_cancel(listener.0.as_ptr()) };
    }

    fn discard_listener(&self, listener: &Listener) {
        // SAFETY: failed startup owns one inactive connection. An inert static
        // handler is installed before cancellation; activating the cancelled
        // connection permits release without publishing an accepting listener.
        unsafe {
            tn_engine_dispatch_install_discard_handler(listener.0.as_ptr());
            xpc_connection_cancel(listener.0.as_ptr());
            xpc_connection_activate(listener.0.as_ptr());
            xpc_release(listener.0.as_ptr());
        }
    }

    fn cancel_timer(&self, timer: &Timer) {
        // SAFETY: only the activated Dispatch::drop calls this, before release.
        unsafe { dispatch_source_cancel(timer.0.as_ptr()) };
    }

    fn release_listener(&self, listener: &Listener) {
        // SAFETY: Dispatch::drop releases this sole +1 reference only after
        // activation and cancellation. Failed startup uses discard_listener.
        unsafe { xpc_release(listener.0.as_ptr()) };
    }

    fn release_timer(&self, timer: &Timer) {
        // SAFETY: a timer can reach Dispatch::drop only after activation. Cancel
        // before releasing its unique +1 reference; no suspended source is released.
        unsafe { dispatch_release(timer.0.as_ptr()) };
    }
}
