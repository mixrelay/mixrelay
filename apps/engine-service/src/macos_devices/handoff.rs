//! Resource handoff has two activation boundaries. Geometry changes accompany
//! a DSP plan; same-spec recovery may activate on any block. Format preparation
//! reserves both lanes until its token is committed or cancelled.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tonenari_audio_io::recovery::RecoveryMailbox;

const IDLE: u8 = 0;
const FORMAT: u8 = 3;

#[derive(Clone, Copy)]
pub(super) enum Publication {
    Graph = 1,
    Recovery = 2,
}

pub(super) struct ResourceHandoff<T: Send> {
    graph: RecoveryMailbox<T>,
    recovery: RecoveryMailbox<T>,
    state: Arc<AtomicU8>,
    completed: AtomicBool,
}

impl<T: Send> Default for ResourceHandoff<T> {
    fn default() -> Self {
        Self {
            graph: RecoveryMailbox::default(),
            recovery: RecoveryMailbox::default(),
            state: Arc::new(AtomicU8::new(IDLE)),
            completed: AtomicBool::new(false),
        }
    }
}

pub(super) struct FormatReservation(Arc<AtomicU8>);
impl Drop for FormatReservation {
    fn drop(&mut self) {
        self.0.store(IDLE, Ordering::Release);
    }
}

impl<T: Send> ResourceHandoff<T> {
    pub(super) fn idle(&self) -> bool {
        self.state.load(Ordering::Acquire) == IDLE
    }

    pub(super) fn has_pending(&self) -> bool {
        matches!(self.state.load(Ordering::Acquire), 1 | 2)
            && !self.completed.load(Ordering::Acquire)
    }

    pub(super) fn reserve_format(&self) -> Option<FormatReservation> {
        self.state
            .compare_exchange(IDLE, FORMAT, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| FormatReservation(self.state.clone()))
    }

    /// Control mutations are serialized by the device cache lock. Reserve
    /// before publication, so the consumer's pending-to-retired transfer cannot
    /// transiently look idle to a concurrent format request.
    pub(super) fn publish(&self, lane: Publication, value: T) -> Result<(), T> {
        if self
            .state
            .compare_exchange(IDLE, lane as u8, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(value);
        }
        if let Err(value) = self.mailbox(lane).publish(0, value) {
            self.state.store(IDLE, Ordering::Release);
            return Err(value);
        }
        Ok(())
    }

    fn mailbox(&self, lane: Publication) -> &RecoveryMailbox<T> {
        match lane {
            Publication::Graph => &self.graph,
            Publication::Recovery => &self.recovery,
        }
    }

    /// # Safety
    /// Exactly one serialized audio consumer. Graph lane is called only with
    /// the matching prepared DSP plan; recovery lane only contains identical specs.
    pub(super) unsafe fn activate(&self, lane: Publication, active: &mut T) {
        let mut swapped = false;
        unsafe {
            self.mailbox(lane).activate(0, |next| {
                std::mem::swap(active, next);
                swapped = true;
            });
        }
        if swapped {
            // Published only AFTER the mailbox has installed its retired owner.
            self.completed.store(true, Ordering::Release);
        }
    }

    /// Control thread, under the cache lock. Never destroy displaced owners on DSP.
    pub(super) fn reclaim(&self) {
        if self.completed.swap(false, Ordering::AcqRel) {
            self.graph.reclaim();
            self.recovery.reclaim();
            self.state.store(IDLE, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_activates_without_a_graph_revision_but_graph_changes_wait() {
        let handoff = ResourceHandoff::default();
        let mut active = 1;
        handoff.publish(Publication::Recovery, 2).unwrap();
        unsafe {
            handoff.activate(Publication::Recovery, &mut active);
        }
        assert_eq!(active, 2);
        assert!(!handoff.idle());
        handoff.reclaim();
        handoff.publish(Publication::Graph, 3).unwrap();
        unsafe {
            handoff.activate(Publication::Recovery, &mut active);
        }
        assert_eq!(active, 2);
        assert!(handoff.reserve_format().is_none());
        assert!(handoff.publish(Publication::Recovery, 4).is_err());
        unsafe {
            handoff.activate(Publication::Graph, &mut active);
        }
        assert_eq!(active, 3);
        handoff.reclaim();
        assert!(handoff.idle());
    }

    #[test]
    fn format_token_excludes_both_lanes_and_drop_releases_reservation() {
        let handoff = ResourceHandoff::default();
        let token = handoff.reserve_format().unwrap();
        assert!(handoff.reserve_format().is_none());
        assert!(handoff.publish(Publication::Graph, 1).is_err());
        assert!(handoff.publish(Publication::Recovery, 2).is_err());
        handoff.reclaim();
        assert!(!handoff.idle());
        drop(token);
        assert!(handoff.idle());
        handoff.publish(Publication::Recovery, 3).unwrap();
    }
}
