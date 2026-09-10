use super::super::endpoint_boundaries::EndpointBoundaries;
use super::{Descriptor, DirectPlan, Renderer, render};
use crate::macos_devices::direct::Registration;
use mixrelay_audio_engine::RenderPlan;
use std::{cell::UnsafeCell, pin::Pin, sync::Arc};

/// Owns the only renderer storage and its descriptor. Pinning both allocations
/// keeps native addresses stable even if this control-side owner moves.
pub(super) struct PreparedRenderer<'state> {
    descriptor: Pin<Box<Descriptor>>,
    state: Pin<Box<UnsafeCell<Renderer<'state>>>>,
    plan: Arc<RenderPlan>,
}

impl<'state> PreparedRenderer<'state> {
    pub(super) fn new(renderer: Renderer<'state>) -> Self {
        let plan = Arc::clone(renderer.processor.plan());
        let state = Box::pin(UnsafeCell::new(renderer));
        let descriptor = Box::pin(Descriptor {
            context: state.as_ref().get_ref().get().cast(),
            render,
            sample_rate: plan.format().sample_rate,
        });
        Self {
            descriptor,
            state,
            plan,
        }
    }

    /// Publication borrows this owner exclusively until the registration has
    /// drained. No safe caller can inspect, mutate or reclaim its renderer then.
    pub(super) fn install<'published>(
        &'published mut self,
        plan: &'published DirectPlan,
    ) -> InstalledRenderer<'published, 'state> {
        let descriptor = self.descriptor.as_ref().get_ref();
        // SAFETY: both pinned allocations outlive the returned exclusive owner
        // borrow. RenderSlot serializes callbacks; InstalledRenderer withdraws
        // publication and drains readers before accessing UnsafeCell or dropping.
        let registration = unsafe { plan.install(descriptor) };
        InstalledRenderer {
            registration,
            descriptor,
            state: self.state.as_ref().get_ref(),
            plan: &self.plan,
        }
    }
}

/// Registration is dropped before the borrowed storage can be reclaimed. The
/// control API returns no renderer reference and cannot publish during an update.
pub(super) struct InstalledRenderer<'published, 'state> {
    registration: Registration<'published>,
    descriptor: &'published Descriptor,
    state: &'published UnsafeCell<Renderer<'state>>,
    plan: &'published RenderPlan,
}

impl InstalledRenderer<'_, '_> {
    pub(super) fn replace_boundaries(&mut self) -> Result<(), String> {
        // SRC construction and storage allocation use immutable plan metadata;
        // neither requires access to the live callback-owned renderer.
        let replacement = EndpointBoundaries::prepare(self.plan)?;
        self.registration.suspend();
        let retired = {
            // SAFETY: suspend removed this exact descriptor and waited for all
            // callbacks holding it. This non-Sync exclusive control owner is now
            // the only accessor. The renderer borrow ends at this block's exit;
            // the returned retired bank owns its storage rather than borrowing it.
            let renderer = unsafe { &mut *self.state.get() };
            super::replace_boundaries(renderer, replacement)
        };
        // SAFETY: the exclusive renderer borrow above has ended. The same pinned
        // descriptor/state are still borrowed by this installed owner. On a panic
        // above, resume is skipped and Registration::drop drains without republishing.
        unsafe { self.registration.resume(self.descriptor) };
        // Potentially large converter/PCM deallocation occurs only after healthy
        // hardware callbacks can run again; no retired bank remains reachable there.
        drop(retired);
        Ok(())
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
            processor: &mut processor,
            bindings: &bindings,
            clock_id: 7,
            epoch: 1,
            native_clock: None,
            status: &status,
            frame: 0,
            progress: &progress,
            failed: &failed,
            invalid: &invalid,
            endpoints: EndpointBoundaries::empty(),
            reconfigure: &invalid,
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
        let descriptor = owner.descriptor.as_ref().get_ref();
        assert_eq!(
            invoke_unpublished(&owner).map(f32::to_bits),
            [0.25_f32.to_bits(); 64]
        );
        assert_eq!(progress.load(Ordering::Acquire), 1);
        assert!(!failed.load(Ordering::Acquire));
        let retired = {
            // SAFETY: there is no native publication in this storage-lifetime
            // fixture and the prior synchronous callback has returned. The live
            // publication/drain ordering is covered by RenderSlot's held-reader test.
            let renderer = unsafe { &mut *owner.state.as_ref().get_ref().get() };
            super::super::replace_boundaries(renderer, EndpointBoundaries::empty())
        };
        assert_eq!(
            invoke_unpublished(&owner).map(f32::to_bits),
            [0.25_f32.to_bits(); 64]
        );
        assert_eq!(std::ptr::from_ref(descriptor), descriptor_address);
        assert_eq!(progress.load(Ordering::Acquire), 2);
        drop(retired);
        drop(owner);
        assert_eq!(processor.plan().format().sample_rate, 48_000);
    }
}
