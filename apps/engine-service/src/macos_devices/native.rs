//! Native registration ownership and acknowledged teardown, never on DSP.
use super::{
    admission::{Admission, Permit},
    aggregate::Aggregate,
    buffer_control::{self, NativeBufferControl},
    buffer_transaction::BufferPlan,
    ffi::{
        mr_device_io_connect_status, mr_device_io_create_group, mr_device_io_destroy,
        mr_device_io_disconnect,
    },
    spec::Spec,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};
// Match the graph's 256-node ceiling, counting BOTH current and quarantined
// native workers. Never release admission merely because a node was removed.
static NATIVE_OWNERS: Admission = Admission::new(256);
pub(super) struct Native {
    pub(super) log_id: u64,
    pub(super) pointer: NonNull<c_void>,
    pub(super) group: Option<Arc<Aggregate>>,
    _permit: Permit<'static>,
    requested_buffer: AtomicU32,
    // Stable Rust-owned callback slot. Drop runs only after native destroy has
    // acknowledged all callbacks, so the native device owner cannot outlive this box.
    pub(super) render: Box<super::direct::RenderSlot>,
}
impl Native {
    #[cfg(test)]
    pub(super) fn active_owners() -> usize {
        NATIVE_OWNERS.in_use()
    }
    pub(super) fn create(
        spec: &Spec,
        rate: u32,
        group: Option<Arc<Aggregate>>,
        buffer_frames: u32,
    ) -> Result<Self, String> {
        let permit = NATIVE_OWNERS
            .acquire()
            .ok_or("CoreAudio native owner limit reached; waiting for retired device callbacks")?;
        let uid = spec.native_uid()?;
        let pointer = NonNull::new(unsafe {
            mr_device_io_create_group(
                group
                    .as_ref()
                    .map_or(std::ptr::null_mut(), |group| group.pointer.as_ptr()),
                uid.as_ptr(),
                spec.input,
                u32::from(spec.channels),
                rate,
            )
        })
        .ok_or("invalid CoreAudio channels or native allocation failed")?;
        let native = Self {
            log_id: super::bridge::NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed),
            pointer,
            group,
            _permit: permit,
            requested_buffer: AtomicU32::new(buffer_frames),
            render: Box::default(),
        };
        unsafe {
            super::ffi::mr_device_io_render_hook(
                native.pointer.as_ptr(),
                std::ptr::from_ref(native.render.as_ref()).cast_mut().cast(),
                super::direct::dispatch,
            );
        }
        Ok(native)
    }
    pub(super) fn request_buffer(&self, frames: u32) {
        self.requested_buffer.store(frames, Ordering::Release);
    }
    pub(super) fn connect(&self, stop: &AtomicBool) -> Result<(), String> {
        let requested = self.requested_buffer.load(Ordering::Acquire);
        let setting = if requested == 0 {
            None
        } else {
            let (device, previous) = buffer_control::prepare_initial(self, requested, stop)?;
            let plan = BufferPlan::prepare([(device, previous)], requested)
                .map_err(|error| format!("CoreAudio buffer preparation failed: {error:?}"))?;
            plan.apply(&mut NativeBufferControl)
                .map_err(|error| format!("CoreAudio buffer application failed: {error:?}"))?;
            Some((device, plan))
        };
        let expected = setting.as_ref().map_or(0, |(device, _)| *device);
        let status = unsafe { mr_device_io_connect_status(self.pointer.as_ptr(), expected) };
        if status != 0 {
            let restored = setting
                .as_ref()
                .is_none_or(|(_, plan)| plan.restore(&mut NativeBufferControl));
            return Err(format!(
                "CoreAudio connection failed: {} ({status}); buffer restored={restored}",
                connection_failure(status)
            ));
        }
        Ok(())
    }
}

fn connection_failure(status: i32) -> &'static str {
    match status {
        -1 => "previous callback registration not drained",
        -2 => "selected device or private group unavailable",
        -3 => "native stream format or channel layout unavailable",
        -5 => "selected device changed during preparation",
        _ => "native callback registration or start rejected",
    }
}
pub(super) struct Connection<'a> {
    pub(super) pointer: *mut c_void,
    pub(super) failed: &'a AtomicBool,
    pub(super) clock_identity: &'a AtomicU32,
}
impl Drop for Connection<'_> {
    fn drop(&mut self) {
        self.failed.store(true, Ordering::Release);
        self.clock_identity.store(0, Ordering::Release);
        // Keep the original owner/context alive until CoreAudio acknowledges
        // unregistering. Reopening over a failed teardown would race callbacks.
        while !unsafe { mr_device_io_disconnect(self.pointer) } {
            thread::park_timeout(Duration::from_millis(100));
        }
    }
}
// SAFETY: the owning native worker exclusively connects/disconnects/queries.
// Native callback, SRC worker and graph each own distinct SPSC cursors. The
// mutable SRC never lives behind this Arc. The device owner uses bounded SPSC storage;
// the last Arc is destroyed only after the native worker and callbacks stop.
unsafe impl Send for Native {}
unsafe impl Sync for Native {}
impl Drop for Native {
    fn drop(&mut self) {
        // Normally the connection guard already drained registration. Even if
        // that invariant is violated, retain the group AND admission until the
        // native destructor acknowledges release; a debug assertion is not a
        // lifetime guarantee in release builds. This destructor is never on DSP.
        while !unsafe { mr_device_io_destroy(self.pointer.as_ptr()) } {
            thread::park_timeout(Duration::from_millis(100));
        }
    }
}
