//! Native workers keep their callback owners until `CoreAudio` acknowledges release.
//! Timeout transfers the join handle here; it never detaches an untracked owner.
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

pub(super) static WORKERS: RetiredWorkers = RetiredWorkers::new();

pub(super) struct RetiredWorkers {
    pending: Mutex<Vec<JoinHandle<()>>>,
    failed: AtomicBool,
}

impl RetiredWorkers {
    const fn new() -> Self {
        Self {
            pending: Mutex::new(Vec::new()),
            failed: AtomicBool::new(false),
        }
    }

    pub(super) fn retire(&self, worker: JoinHandle<()>) {
        worker.thread().unpark();
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(worker);
    }

    pub(super) fn reclaim(&self) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extract_if(.., |worker| worker.is_finished())
            .for_each(|worker| {
                if worker.join().is_err() {
                    self.failed.store(true, Ordering::Release);
                    tracing::error!("retired CoreAudio worker panicked");
                }
            });
    }

    /// Called after all graph/device producers have dropped, before recording
    /// server exit. Joining does not hold the retirement mutex or cancel native IO.
    pub(super) fn finish(&self) -> Result<(), String> {
        let workers = std::mem::take(
            &mut *self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let mut failed = self.failed.load(Ordering::Acquire);
        for worker in workers {
            failed |= worker.join().is_err();
        }
        if failed {
            self.failed.store(true, Ordering::Release);
            Err("CoreAudio worker panicked during retirement".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn retirement_does_not_wait_but_finalization_joins_the_native_owner() {
        let workers = Arc::new(RetiredWorkers::new());
        let (release, blocked) = mpsc::channel();
        let owner = Arc::new(());
        let weak = Arc::downgrade(&owner);
        workers.retire(std::thread::spawn(move || {
            blocked.recv().unwrap();
            drop(owner);
        }));
        workers.reclaim();
        assert!(weak.upgrade().is_some());
        let (finished, observed) = mpsc::channel();
        let pending = workers.clone();
        let joining = std::thread::spawn(move || {
            finished.send(pending.finish()).unwrap();
        });
        assert!(matches!(
            observed.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        observed
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        joining.join().unwrap();
        assert!(weak.upgrade().is_none());
        workers.finish().unwrap();
    }

    #[test]
    fn finalization_reports_a_panicked_worker() {
        let workers = RetiredWorkers::new();
        workers.retire(std::thread::spawn(|| {
            panic!("injected native worker failure")
        }));
        assert!(workers.finish().is_err());
    }

    #[test]
    fn reclaimed_worker_failure_remains_visible_at_finalization() {
        let workers = RetiredWorkers::new();
        let worker = std::thread::spawn(|| panic!("injected retired worker failure"));
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        workers.retire(worker);
        workers.reclaim();
        assert!(workers.pending.lock().unwrap().is_empty());
        assert!(workers.finish().is_err());
    }
}
