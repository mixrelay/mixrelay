use super::{Descriptor, DirectPlan, Renderer, render};
use crate::macos_devices::direct::Registration;
use std::{cell::UnsafeCell, pin::Pin};

/// Owns the only renderer storage and its descriptor. Pinning both allocations
/// keeps native addresses stable even if this control-side owner moves.
pub(super) struct PreparedRenderer<'state> {
    descriptor: Pin<Box<Descriptor>>,
    state: Pin<Box<UnsafeCell<Renderer<'state>>>>,
}

impl<'state> PreparedRenderer<'state> {
    pub(super) fn new(renderer: Renderer<'state>) -> Self {
        let sample_rate = renderer.processor.plan().format().sample_rate;
        let state = Box::pin(UnsafeCell::new(renderer));
        let descriptor = Box::pin(Descriptor {
            context: state.as_ref().get_ref().get().cast(),
            render,
            sample_rate,
        });
        Self { descriptor, state }
    }

    /// Publication borrows this owner exclusively until the registration has
    /// drained. No safe caller can inspect, mutate or reclaim its renderer then.
    pub(super) fn install<'published>(
        &'published mut self,
        plan: &'published DirectPlan,
    ) -> Registration<'published> {
        let descriptor = self.descriptor.as_ref().get_ref();
        debug_assert_eq!(
            descriptor.context,
            self.state.as_ref().get_ref().get().cast()
        );
        // SAFETY: both pinned allocations outlive the returned exclusive owner
        // borrow. RenderSlot serializes callbacks; Registration withdraws
        // publication and drains readers before accessing UnsafeCell or dropping.
        unsafe { plan.install(descriptor) }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{processor_at, stereo_bindings, stereo_list};
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    fn invoke_unpublished(owner: &PreparedRenderer<'_>) -> [f32; 64] {
        let mut input = [0.25_f32; 64];
        let mut output = [0.0; 64];
        let source = stereo_list(&mut input);
        let mut sink = stereo_list(&mut output);
        let descriptor = owner.descriptor.as_ref().get_ref();
        // SAFETY: the fixture owns an unpublished, non-Sync renderer. Both ABLs
        // retain exactly 32 stereo frames through this sole synchronous invocation;
        // the owner pins its context until after the invocation returns.
        unsafe {
            (descriptor.render)(
                descriptor.context,
                (&raw const source).cast(),
                (&raw mut sink).cast(),
                32,
                std::ptr::null(),
            );
        }
        output
    }

    #[test]
    fn moving_the_prepared_owner_keeps_native_state_and_descriptor_stable() {
        let mut processor = processor_at(32, 48_000);
        let bindings = stereo_bindings();
        let status = mixrelay_engine_service::session::AdapterRuntimeStatus::new(
            crate::macos::adapter_profile(),
        );
        let progress = AtomicU64::new(0);
        let failed = AtomicBool::new(false);
        let invalid = AtomicBool::new(false);
        let owner = PreparedRenderer::new(Renderer {
            timebase: mixrelay_realtime::Timebase::current(),
            processor: &mut processor,
            activation: None,
            bindings: &bindings,
            clock_id: 7,
            epoch: 1,
            native_clock: None,
            status: &status,
            position: None,
            progress: &progress,
            failed: &failed,
            invalid: &invalid,
        });
        let state_address = owner.state.as_ref().get_ref().get();
        let descriptor_address = std::ptr::from_ref(owner.descriptor.as_ref().get_ref());
        let mut moved = vec![owner];
        let owner = moved.pop().unwrap();
        assert_eq!(owner.state.as_ref().get_ref().get(), state_address);
        assert_eq!(
            std::ptr::from_ref(owner.descriptor.as_ref().get_ref()),
            descriptor_address
        );
        assert_eq!(
            invoke_unpublished(&owner).map(f32::to_bits),
            [0.25_f32.to_bits(); 64]
        );
        assert_eq!(progress.load(Ordering::Acquire), 1);
        assert!(!failed.load(Ordering::Acquire));
        drop(owner);
        assert_eq!(processor.plan().format().sample_rate, 48_000);
    }
}
