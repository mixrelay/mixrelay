//! Bounded control-to-audio recovery handoff. Native preparation and destruction
//! stay on control threads; stale generations cannot resurrect removed resources.
use std::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

struct Prepared<T> {
    generation: u64,
    value: T,
}

pub struct RecoveryMailbox<T: Send> {
    pending: AtomicPtr<Prepared<T>>,
    retired: AtomicPtr<Prepared<T>>,
}

impl<T: Send> Default for RecoveryMailbox<T> {
    fn default() -> Self {
        Self {
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

impl<T: Send> RecoveryMailbox<T> {
    #[must_use]
    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::Acquire).is_null()
            && self.retired.load(Ordering::Acquire).is_null()
    }

    /// Control thread only. Failed publication retains caller ownership.
    /// # Errors
    /// Returns the prepared value when another recovery is still pending.
    pub fn publish(&self, generation: u64, value: T) -> Result<(), T> {
        let pointer = Box::into_raw(Box::new(Prepared { generation, value }));
        if self
            .pending
            .compare_exchange(
                ptr::null_mut(),
                pointer,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            // SAFETY: failed publication leaves sole ownership on this thread.
            return Err(unsafe { Box::from_raw(pointer) }.value);
        }
        Ok(())
    }

    /// Exactly one audio consumer may call this. The closure must only swap
    /// prepared owners, never allocate, block, panic, or destroy native resources.
    /// Generation mismatch is retired without invoking the closure.
    ///
    /// # Safety
    /// Calls must be serialized on one audio thread, with no reentrant invocation.
    pub unsafe fn activate(&self, generation: u64, apply: impl FnOnce(&mut T)) {
        if !self.retired.load(Ordering::Acquire).is_null() {
            return;
        }
        let pointer = self.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        if pointer.is_null() {
            return;
        }
        // SAFETY: this sole consumer removed the only published owner. Keep the
        // allocation in the mailbox even on stale generations; never drop on RT.
        let prepared = unsafe { &mut *pointer };
        if prepared.generation == generation {
            apply(&mut prepared.value);
        }
        self.retired.store(pointer, Ordering::Release);
    }

    /// Control thread only; drains displaced and stale native owners.
    pub fn reclaim(&self) {
        let pointer = self.retired.swap(ptr::null_mut(), Ordering::AcqRel);
        if !pointer.is_null() {
            // SAFETY: swap transfers the unique retired owner to this caller.
            drop(unsafe { Box::from_raw(pointer) });
        }
    }
}

impl<T: Send> Drop for RecoveryMailbox<T> {
    fn drop(&mut self) {
        for slot in [&mut self.pending, &mut self.retired] {
            let pointer = *slot.get_mut();
            if !pointer.is_null() {
                // SAFETY: exclusive destruction after the audio consumer drains.
                drop(unsafe { Box::from_raw(pointer) });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Owner(Arc<Mutex<Vec<std::thread::ThreadId>>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(std::thread::current().id());
        }
    }
    #[test]
    fn stale_and_replaced_owners_are_destroyed_only_by_control() {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let mailbox = Arc::new(RecoveryMailbox::default());
        assert!(mailbox.publish(1, Owner(drops.clone())).is_ok());
        let rt = mailbox.clone();
        std::thread::spawn(move || {
            // SAFETY: this test has exactly one serialized audio consumer.
            unsafe {
                rt.activate(2, |_| panic!("stale resource activated"));
            }
        })
        .join()
        .unwrap();
        assert!(drops.lock().unwrap().is_empty());
        mailbox.reclaim();
        assert_eq!(*drops.lock().unwrap(), vec![std::thread::current().id()]);
        assert!(mailbox.idle());
        assert!(mailbox.publish(3, Owner(drops.clone())).is_ok());
        assert!(mailbox.publish(3, Owner(drops.clone())).is_err());
        let mut active = Owner(drops.clone());
        // SAFETY: previous consumer joined; this is now the sole consumer.
        unsafe {
            mailbox.activate(3, |next| std::mem::swap(&mut active, next));
        }
        mailbox.reclaim();
        drop(active);
        assert_eq!(drops.lock().unwrap().len(), 4);
    }
    #[test]
    fn repeated_recovery_is_bounded_and_never_replays_stale_generation() {
        let mailbox = RecoveryMailbox::default();
        let mut active = 0;
        for generation in 1..1001 {
            mailbox.publish(generation, generation).unwrap();
            // SAFETY: serialized single consumer, only swaps integers.
            unsafe {
                mailbox.activate(generation, |next| std::mem::swap(&mut active, next));
            }
            assert_eq!(active, generation);
            mailbox.reclaim();
            assert!(mailbox.idle());
        }
    }
}
