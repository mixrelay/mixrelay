use super::{
    TapError, TapSession,
    native::{Native, ProcessIdentity},
};
use crate::process_tap_input::TapFormat;
use mixrelay_macos_transport::capture_ring::mr_audio_ring_read_interleaved;
use std::sync::atomic::Ordering;

struct Discovery {
    candidate: Vec<ProcessIdentity>,
    retrack: bool,
}

impl TapSession {
    pub(super) fn adopt_registration(
        &self,
        proc: *mut std::ffi::c_void,
        status: i32,
    ) -> Result<(), TapError> {
        let mut resources = self.resources.get();
        resources.proc = proc;
        self.resources.set(resources);
        TapError::status(status)?;
        if proc.is_null() {
            Err(TapError::code(4))
        } else {
            Ok(())
        }
    }
    pub(super) fn start(&self, native: &impl Native) -> Result<(), TapError> {
        if self.running.load(Ordering::Acquire) {
            return Err(TapError::code(7));
        }
        let resources = self.resources.get();
        if resources.proc.is_null() || !self.allowed() {
            return Err(TapError::code(9));
        }
        native.start(resources.aggregate, resources.proc)?;
        self.running.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn stop(&self, native: &impl Native) -> Result<(), TapError> {
        if !self.running.load(Ordering::Acquire) {
            return Err(TapError::code(8));
        }
        let mut resources = self.resources.get();
        native.stop(resources.aggregate, resources.proc)?;
        self.running.store(false, Ordering::Release);
        resources.resume = false;
        self.resources.set(resources);
        Ok(())
    }

    fn unregister(&self, native: &impl Native) -> bool {
        let mut resources = self.resources.get();
        if self.running.load(Ordering::Acquire)
            && native.stop(resources.aggregate, resources.proc).is_ok()
        {
            self.running.store(false, Ordering::Release);
        }
        if !resources.proc.is_null() && resources.aggregate != 0 {
            if native
                .unregister(resources.aggregate, resources.proc)
                .is_err()
            {
                return false;
            }
            resources.proc = std::ptr::null_mut();
            self.resources.set(resources);
        }
        self.running.store(false, Ordering::Release);
        self.active_callbacks.load(Ordering::Acquire) == 0
    }

    pub(super) fn reclaim(&self, native: &impl Native) -> bool {
        let mut resources = self.resources.get();
        if resources.inventory_listener {
            if native.remove_inventory_listener(self).is_err() {
                return false;
            }
            resources.inventory_listener = false;
            self.resources.set(resources);
        }
        self.reclaim_tap(native)
    }

    fn reclaim_tap(&self, native: &impl Native) -> bool {
        if !self.unregister(native) {
            return false;
        }
        let mut resources = self.resources.get();
        if resources.listener {
            if native.remove_listener(resources.tap, self).is_err() {
                return false;
            }
            resources.listener = false;
            self.resources.set(resources);
        }
        if self.active_listeners.load(Ordering::Acquire) != 0 {
            return false;
        }
        if resources.aggregate != 0 {
            if native.destroy_aggregate(resources.aggregate).is_err() {
                return false;
            }
            resources.aggregate = 0;
            self.resources.set(resources);
        }
        if resources.tap != 0 {
            if native.destroy_tap(resources.tap).is_err() {
                return false;
            }
            resources.tap = 0;
            self.resources.set(resources);
        }
        true
    }

    pub(super) fn refresh(&self, native: &impl Native) -> Result<(), TapError> {
        // SAFETY: serialized control retains this owner; lifecycle itself is atomic.
        if self.lifecycle.retiring() {
            return Err(TapError::code(1));
        }
        let observed = self.lifecycle.epoch();
        let discovery = self.discover(native)?;
        let bundle_recertification = discovery.is_some();
        if let Some(Discovery {
            candidate,
            retrack: true,
        }) = discovery
        {
            return self.retrack(native, observed, candidate);
        }
        // SAFETY: gate remains live throughout native format discovery and commit.
        let result = native.format(self.resources.get().tap);
        let compatible = result.as_ref().is_ok_and(|format| self.compatible(format));
        let unchanged = result.as_ref().is_ok_and(|format| {
            format.sample_rate.to_bits() == self.sample_rate.load(Ordering::Acquire)
        });
        // SAFETY: only atomic gate publication overlaps native listeners.
        if compatible
            && unchanged
            && (self.lifecycle.current(observed) || bundle_recertification)
            && (!self.resources.get().resume || self.running.load(Ordering::Acquire))
        {
            // The same exact process identities and unchanged format require
            // only epoch re-certification, never a hardware restart. A newer
            // notification during discovery makes this commit fail safely.
            if self.lifecycle.current(observed) || self.lifecycle.commit(observed) {
                return Ok(());
            }
            return Err(TapError::code(9));
        }
        if self.lifecycle.epoch() != observed {
            return Err(TapError::code(9));
        }
        // SAFETY: invalidation precedes unregister and prevents new ring producers.
        let epoch = self.lifecycle.invalidate(observed);
        let mut resources = self.resources.get();
        resources.resume |= self.running.load(Ordering::Acquire);
        self.resources.set(resources);
        if !self.unregister(native) {
            return Err(TapError::code(9));
        }
        let format = result?;
        if !compatible {
            return Err(TapError::code(5));
        }
        self.prepare_format(native, &format)?;
        // SAFETY: old callback drained; new proc is not started until epoch certification.
        if !self.lifecycle.commit(epoch) {
            return Err(TapError::code(9));
        }
        if self.resources.get().resume {
            if let Err(error) = self.start(native) {
                // SAFETY: failed native start must keep delivery invalid until next refresh.
                self.lifecycle.changed();
                return Err(error);
            }
            let mut resources = self.resources.get();
            resources.resume = false;
            self.resources.set(resources);
        }
        Ok(())
    }

    fn discover(&self, native: &impl Native) -> Result<Option<Discovery>, TapError> {
        let selection = self.selection.borrow();
        let Some(selection) = selection.as_ref().filter(|selection| selection.fallback) else {
            return Ok(None);
        };
        if let (Some(uid), Some(device)) = (&selection.target_device_uid, selection.target_device) {
            let current = native
                .device(uid)
                .inspect_err(|_| self.lifecycle.changed())?;
            if current != device {
                self.lifecycle.changed();
                return Err(TapError::code(9));
            }
        }
        let inventory_epoch = self.inventory_epoch.load(Ordering::Acquire);
        let candidate = native
            .processes(
                &selection.bundle,
                selection.target_device.map(|lease| lease.object),
            )
            .inspect_err(|_| self.lifecycle.changed())?;
        if inventory_epoch != self.inventory_epoch.load(Ordering::Acquire) {
            return Err(TapError::code(9));
        }
        if selection.target_device.is_some() && candidate != selection.selected {
            // A scoped mute is a lease on the admitted process objects, not a
            // bundle-wide permission to suppress later routes or replacements.
            self.lifecycle.changed();
            return Err(TapError::code(9));
        }
        let resources = self.resources.get();
        let retrack = candidate != selection.selected
            || resources.tap == 0
            || resources.aggregate == 0
            || !resources.listener
            || resources.proc.is_null();
        Ok(Some(Discovery { candidate, retrack }))
    }

    fn retrack(
        &self,
        native: &impl Native,
        observed: u64,
        candidate: Vec<ProcessIdentity>,
    ) -> Result<(), TapError> {
        // No callback may deliver old-identity PCM after this invalidation.
        let epoch = self.lifecycle.invalidate(observed);
        let mut resources = self.resources.get();
        resources.resume |= self.running.load(Ordering::Acquire);
        self.resources.set(resources);
        if !self.reclaim_tap(native) {
            return Err(TapError::code(9));
        }
        self.discard_pcm();
        let mut selection = self.selection.borrow_mut();
        let selection = selection.as_mut().ok_or(TapError::code(1))?;
        selection.selected = candidate;
        if selection.selected.is_empty() {
            // No tap with an empty process list is ever created: the native
            // initializer is not assumed to mean silence for that case.
            return Err(TapError::code(9));
        }
        native.rebuild(self, selection)?;
        let format = native.format(self.resources.get().tap)?;
        if !self.compatible(&format) {
            return Err(TapError::code(5));
        }
        self.prepare_format(native, &format)?;
        if !self.lifecycle.commit(epoch) {
            return Err(TapError::code(9));
        }
        if self.resources.get().resume {
            if let Err(error) = self.start(native) {
                self.lifecycle.changed();
                return Err(error);
            }
            let mut resources = self.resources.get();
            resources.resume = false;
            self.resources.set(resources);
        }
        Ok(())
    }

    fn compatible(&self, format: &TapFormat) -> bool {
        self.format
            .get()
            .is_some_and(|current| format.supported() && format.same_geometry(current))
    }

    fn prepare_format(&self, native: &impl Native, format: &TapFormat) -> Result<(), TapError> {
        self.discard_pcm();
        native.register(self)?;
        self.sample_rate
            .store(format.sample_rate.to_bits(), Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn discard_pcm(&self) {
        let mut discarded = [0.0; 64 * 64];
        // SAFETY: callback unregistration and active count have drained the only
        // producer; control owns the sole consumer and scratch covers 64 channels.
        while unsafe { mr_audio_ring_read_interleaved(self.ring(), discarded.as_mut_ptr(), 64) }
            != 0
        {}
        self.peaks.clear();
    }
}
