//! A single native callback owns a same-clock hardware graph. The control
//! supervisor prepares channel maps and drains publication before reclaiming
//! the borrowed Rust renderer. Independent clocks retain their explicit bridge.
use super::{
    DeviceSet,
    device::Device,
    ffi::{
        tn_device_io_clock_discontinuities, tn_device_io_direct_binding, tn_device_io_output_epoch,
    },
};
use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use tonenari_audio_engine::RenderPlan;
use tonenari_audio_graph::{FROM_OS_PORT_ID, TO_OS_PORT_ID};

pub(crate) fn next_epoch() -> u64 {
    super::bridge::NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed)
}

#[repr(C)]
pub(crate) struct Descriptor {
    pub(crate) context: *mut c_void,
    pub(crate) render: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void, u32),
}

#[derive(Default)]
pub(super) struct RenderSlot {
    enabled: AtomicBool,
    descriptor: AtomicPtr<Descriptor>,
    readers: AtomicU32,
    rendering: AtomicBool,
    pub(super) generation: AtomicU64,
    pub(super) reconnect: AtomicBool,
}
impl RenderSlot {
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    fn publish(&self, descriptor: *mut Descriptor) {
        self.descriptor.store(descriptor, Ordering::SeqCst);
    }
    fn drained(&self) -> bool {
        self.readers.load(Ordering::SeqCst) == 0
    }
    fn finish(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.enabled.store(false, Ordering::Release);
    }
}

pub(super) unsafe extern "C" fn dispatch(
    context: *mut c_void,
    input: *const c_void,
    output: *mut c_void,
    frames: u32,
) -> bool {
    // SAFETY: Native owns this stable box until CoreAudio unregister is acknowledged.
    let slot = unsafe { &*context.cast::<RenderSlot>() };
    if !slot.enabled() {
        return false;
    }
    slot.readers.fetch_add(1, Ordering::SeqCst);
    let descriptor = slot.descriptor.load(Ordering::SeqCst);
    // Never wait on an audio callback, even for an unexpected concurrent vendor
    // invocation. This also enforces the exclusive Rust mutable-renderer contract.
    if !descriptor.is_null()
        && slot
            .rendering
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    {
        let descriptor = unsafe { &*descriptor };
        unsafe {
            (descriptor.render)(descriptor.context, input, output, frames);
        }
        slot.rendering.store(false, Ordering::Release);
    }
    slot.readers.fetch_sub(1, Ordering::SeqCst);
    true
}

pub(crate) struct Binding {
    pub(crate) id: String,
    pub(crate) input: bool,
    pub(crate) offset: usize,
    pub(crate) channels: usize,
}

pub(crate) struct DirectPlan {
    master: Arc<Device>,
    owners: Vec<Arc<Device>>,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) clock_id: u32,
}

impl DirectPlan {
    /// Control only. A shared rate is insufficient: every hardware binding
    /// must name the same verified native clock and actual graph rate. No HAL
    /// shared-memory or app-capture timeline is silently relabelled as this `IOProc`.
    pub(crate) fn prepare(devices: &DeviceSet, plan: &RenderPlan) -> Option<Self> {
        let master = devices
            .0
            .values()
            .filter(|device| device.ready())
            .min_by_key(|device| device.input)?
            .clone();
        let clock_id = master.clock_identity.load(Ordering::Acquire);
        let native_rate = master.native_rate.load(Ordering::Acquire);
        if clock_id == 0 {
            return None;
        }
        let mut bindings = Vec::new();
        for (input, list) in [
            (true, plan.source_bindings()),
            (false, plan.sink_bindings()),
        ] {
            for binding in list {
                if input
                    && binding.virtual_endpoint_id().is_some()
                    && binding.port_id().as_str() == FROM_OS_PORT_ID
                {
                    continue;
                }
                if !input
                    && binding.virtual_endpoint_id().is_some()
                    && binding.port_id().as_str() == TO_OS_PORT_ID
                {
                    continue;
                }
                let device = devices.0.get(binding.node_id().as_str())?;
                if !device.ready()
                    || device.input != input
                    || device.clock_identity.load(Ordering::Acquire) != clock_id
                    || device.native_rate.load(Ordering::Acquire) != native_rate
                    || native_rate != plan.format().sample_rate
                {
                    return None;
                }
                let (mut resolved, mut offset, mut total) = (0, 0, 0);
                // SAFETY: control-only immutable selection resolution, with owned native lifetime.
                if !unsafe {
                    tn_device_io_direct_binding(
                        device.native.pointer.as_ptr(),
                        &raw mut resolved,
                        &raw mut offset,
                        &raw mut total,
                    )
                } || resolved != clock_id
                {
                    return None;
                }
                bindings.push(Binding {
                    id: binding.node_id().as_str().to_owned(),
                    input,
                    offset: offset as usize,
                    channels: usize::from(binding.channels()),
                });
            }
        }
        if bindings.is_empty() {
            return None;
        }
        Some(Self {
            master,
            owners: devices.0.values().cloned().collect(),
            bindings,
            clock_id,
        })
    }

    pub(crate) fn healthy(&self) -> bool {
        self.owners.iter().all(|device| {
            !device.failed.load(Ordering::Acquire)
                && device.clock_identity.load(Ordering::Acquire) == self.clock_id
        })
    }

    pub(crate) fn discontinuities(&self) -> u64 {
        // Atomic observation only; unlike device properties this is callback-safe.
        unsafe { tn_device_io_clock_discontinuities(self.master.native.pointer.as_ptr()) }
    }

    pub(crate) fn request_reconnect(&self) {
        for owner in &self.owners {
            owner.failed.store(true, Ordering::Release);
            owner.native.render.reconnect.store(true, Ordering::Release);
            if let Some(worker) = &owner.worker {
                worker.thread().unpark();
            }
        }
    }

    /// # Safety
    /// Descriptor and its exclusive renderer must remain valid and untouched
    /// until this guard drops. The `IOProc` is its only mutable caller.
    pub(crate) unsafe fn install<'a>(&'a self, descriptor: &'a Descriptor) -> Registration<'a> {
        for device in &self.owners {
            device.native.render.enabled.store(true, Ordering::Release);
        }
        self.master
            .native
            .render
            .publish(std::ptr::from_ref(descriptor).cast_mut());
        Registration { plan: self }
    }
}

pub(crate) struct Registration<'a> {
    plan: &'a DirectPlan,
}
impl Registration<'_> {
    /// Control-side boundary-only replacement. The native registration stays
    /// enabled, so hardware owners do not revert to polling/SRC workers. The
    /// descriptor's borrowed state may be changed only after this reader drain.
    pub(crate) fn suspend(&self) {
        self.plan.master.native.render.publish(std::ptr::null_mut());
        while !self.plan.master.native.render.drained() {
            thread::sleep(Duration::from_micros(100));
        }
    }
    /// # Safety
    /// The original live descriptor and renderer must remain valid through this
    /// registration's drain. No callback may own that renderer while suspended.
    pub(crate) unsafe fn resume(&self, descriptor: &Descriptor) {
        self.plan
            .master
            .native
            .render
            .publish(std::ptr::from_ref(descriptor).cast_mut());
    }
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        // Remove publication first; a timeout is NOT permission to free a
        // renderer still reachable by CoreAudio. Waiting occurs on control only.
        self.suspend();
        for device in &self.plan.owners {
            device.ready_rate.store(0, Ordering::Release);
            // Invalidate pre-direct PCM BEFORE exposing the ring consumer again.
            // This is an atomic epoch publication, not a cross-thread cursor reset.
            unsafe {
                tn_device_io_output_epoch(device.native.pointer.as_ptr(), next_epoch());
            }
            // Publish the EXIT, not entry: the supervisor may have sampled the
            // old mode just before entry, so an entry generation can be consumed
            // before the direct lifetime even starts.
            device.native.render.finish();
            if let Some(worker) = &device.worker {
                worker.thread().unpark();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        entered: AtomicBool,
        release: AtomicBool,
        calls: AtomicU64,
    }
    unsafe extern "C" fn held_render(
        context: *mut c_void,
        _: *const c_void,
        _: *mut c_void,
        _: u32,
    ) {
        let probe = unsafe { &*context.cast::<Probe>() };
        probe.calls.fetch_add(1, Ordering::Relaxed);
        probe.entered.store(true, Ordering::Release);
        // Deliberately hold a TEST renderer to prove control cannot reclaim it.
        while !probe.release.load(Ordering::Acquire) {
            thread::yield_now();
        }
    }

    #[test]
    fn removal_drains_in_flight_renderer_and_excludes_concurrent_mutation() {
        let slot = Arc::new(RenderSlot::default());
        let mut probe = Box::new(Probe {
            entered: AtomicBool::new(false),
            release: AtomicBool::new(false),
            calls: AtomicU64::new(0),
        });
        let mut descriptor = Box::new(Descriptor {
            context: (&raw mut *probe).cast(),
            render: held_render,
        });
        slot.enabled.store(true, Ordering::Release);
        slot.publish(&raw mut *descriptor);
        let other = slot.clone();
        let callback = thread::spawn(move || unsafe {
            dispatch(
                Arc::as_ptr(&other).cast_mut().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                32,
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !probe.entered.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            thread::yield_now();
        }
        // A second native invocation must not form another &mut Renderer.
        unsafe {
            dispatch(
                Arc::as_ptr(&slot).cast_mut().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                32,
            );
        }
        assert_eq!(probe.calls.load(Ordering::Acquire), 1);
        slot.publish(std::ptr::null_mut());
        assert!(!slot.drained());
        probe.release.store(true, Ordering::Release);
        assert!(callback.join().unwrap());
        assert!(slot.drained());
        drop(descriptor);
        drop(probe);
        for _ in 0..10000 {
            assert!(unsafe {
                dispatch(
                    Arc::as_ptr(&slot).cast_mut().cast(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    32,
                )
            });
        }
        assert!(slot.drained());
    }

    #[test]
    fn completed_direct_lifetime_is_observable_without_observing_enabled() {
        let slot = RenderSlot::default();
        let observed = slot.generation.load(Ordering::Acquire);
        slot.enabled.store(true, Ordering::Release);
        slot.finish();
        assert!(!slot.enabled());
        assert_ne!(observed, slot.generation.load(Ordering::Acquire));
    }
}
