//! Prepares child reaping before startup and retains ownership through OS exit.
use std::{
    io,
    process::Child,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

// A startup failure closes the lease before allowing a short cooperative drain.
const STARTUP_FAILURE_GRACE: Duration = Duration::from_millis(500);
const READY_EXIT_POLL: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub(super) struct Reaper {
    handoff: Arc<Handoff>,
    _thread: JoinHandle<()>,
}

#[derive(Debug, Default)]
struct Handoff {
    state: Mutex<Pending>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct Pending {
    child: Option<Child>,
    closed: bool,
}

impl Reaper {
    pub(super) fn prepare() -> io::Result<Self> {
        Self::prepare_with(|handoff| {
            thread::Builder::new()
                .name("mixrelay-engine-reaper".into())
                .spawn(move || wait_for_owned_child(&handoff))
        })
    }

    fn prepare_with(
        spawn: impl FnOnce(Arc<Handoff>) -> io::Result<JoinHandle<()>>,
    ) -> io::Result<Self> {
        let handoff = Arc::new(Handoff::default());
        let thread = spawn(Arc::clone(&handoff))?;
        Ok(Self {
            handoff,
            _thread: thread,
        })
    }

    #[cfg(test)]
    pub(super) fn refuse_preparation() -> io::Result<Self> {
        Self::prepare_with(|_| {
            Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                "reaper unavailable",
            ))
        })
    }

    #[cfg(test)]
    pub(super) fn observe_exit() -> io::Result<(Self, std::sync::mpsc::Receiver<()>)> {
        let (ended, observed) = std::sync::mpsc::channel();
        let reaper = Self::prepare_with(|handoff| {
            thread::Builder::new()
                .name("mixrelay-test-reaper".into())
                .spawn(move || {
                    wait_for_owned_child(&handoff);
                    let _ = ended.send(());
                })
        })?;
        Ok((reaper, observed))
    }

    /// Moves the only child into the prepared worker without creating a thread.
    pub(super) fn schedule(self, child: Child) {
        self.handoff
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .child = Some(child);
        self.handoff.changed.notify_one();
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        self.handoff
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
        self.handoff.changed.notify_one();
        // Dropping the handle detaches the prepared worker. It owns any handed-off
        // child until exit; an unused worker exits after observing closure.
    }
}

fn wait_for_owned_child(handoff: &Handoff) {
    let child = {
        let mut state = handoff
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.child.is_none() && !state.closed {
            state = handoff
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.child.take()
    };
    // The handoff lock is released before waiting for the engine's audio drain.
    if let Some(mut child) = child {
        let pid = child.id();
        if let Err(error) = child.wait() {
            tracing::error!(pid, %error, "owned controller engine wait failed");
        }
    }
}

pub(super) fn after_startup_failure(mut child: Child, reaper: Reaper) -> io::Result<()> {
    let deadline = Instant::now() + STARTUP_FAILURE_GRACE;
    let mut probe_error = None;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => {
                probe_error = Some(error);
                break;
            }
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        thread::sleep((deadline - now).min(READY_EXIT_POLL));
    }

    let mut cleanup_error = probe_error;
    match child.kill() {
        Ok(()) => {}
        Err(error) => {
            combine_error(&mut cleanup_error, &error, "kill owned startup child");
        }
    }
    reaper.schedule(child);
    cleanup_error.map_or(Ok(()), Err)
}

fn combine_error(slot: &mut Option<io::Error>, error: &io::Error, operation: &str) {
    let message = format!("{operation}: {error}");
    if let Some(existing) = slot.take() {
        *slot = Some(io::Error::new(
            existing.kind(),
            format!("{existing}; {message}"),
        ));
    } else {
        *slot = Some(io::Error::other(message));
    }
}

#[cfg(test)]
mod tests;
