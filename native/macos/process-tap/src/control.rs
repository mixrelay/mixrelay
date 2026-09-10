use super::{TapError, TapSession, native::Native};
use crate::process_tap_input::TapFormat;
use mixrelay_macos_transport::capture_ring::mr_audio_ring_read_interleaved;
use std::sync::atomic::Ordering;

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
        // SAFETY: gate remains live throughout native discovery and commit.
        let observed = self.lifecycle.epoch();
        let result = native.format(self.resources.get().tap);
        let compatible = result.as_ref().is_ok_and(|format| self.compatible(format));
        let unchanged = result.as_ref().is_ok_and(|format| {
            format.sample_rate.to_bits() == self.sample_rate.load(Ordering::Acquire)
        });
        // SAFETY: only atomic gate publication overlaps native listeners.
        if compatible && unchanged && self.lifecycle.current(observed) {
            return Ok(());
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

    fn compatible(&self, format: &TapFormat) -> bool {
        self.format
            .get()
            .is_some_and(|current| format.supported() && format.same_geometry(current))
    }

    fn prepare_format(&self, native: &impl Native, format: &TapFormat) -> Result<(), TapError> {
        let mut discarded = [0.0; 64 * 64];
        // SAFETY: callback unregistration and active count have drained the only
        // producer; control owns the sole consumer and scratch covers 64 channels.
        while unsafe { mr_audio_ring_read_interleaved(self.ring(), discarded.as_mut_ptr(), 64) }
            != 0
        {}
        self.peaks.clear();
        native.register(self)?;
        self.sample_rate
            .store(format.sample_rate.to_bits(), Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
}
