use crate::{
    RecorderRequest, RecorderState, RecorderStatus, RecorderTap,
    transport::{self, Transport},
    worker::{Command, Worker},
};
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering, mpsc},
    thread::JoinHandle,
};

const MAX_SESSIONS: usize = 16;

/// Control-side owner of at most sixteen recorder workers and their fixed packet
/// pools (at most 1 MiB of PCM per 8-channel recorder). Clones share sessions.
/// The final registry Drop stops/drains and joins all workers; do not drop the
/// registry or call its methods on an audio thread.
#[derive(Clone, Default)]
pub struct RecorderRegistry {
    sessions: Arc<Mutex<BTreeMap<String, Session>>>,
    reap_error: Arc<Mutex<Option<String>>>,
}

impl RecorderRegistry {
    /// Creates an empty control-side registry, without spawning a thread.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepare a stable tap for a graph-local runtime identity, off the audio thread.
    /// Discarding a failed graph preparation never stops an existing active tap.
    /// # Errors
    /// Rejects empty IDs, channel counts outside the shared recording limit, capacity exhaustion,
    /// channel changes while a previous tap/take exists, or worker spawn failure.
    pub fn prepare(&self, id: &str, channels: u16) -> Result<RecorderTap, String> {
        if id.trim().is_empty()
            || !(1..=tonenari_audio_io::formats::MAX_RECORDING_CHANNELS).contains(&channels)
        {
            return Err("invalid recorder identity or channel count".into());
        }
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get(id) {
            if session.transport.channels == usize::from(channels) {
                return Ok(session.tap());
            }
            if session.transport.taps.load(Ordering::SeqCst) != 0
                || matches!(
                    session.status().state,
                    RecorderState::Recording | RecorderState::Paused
                )
            {
                return Err("recorder channels cannot change while a tap or take is active".into());
            }
        }
        if let Some(mut replaced) = sessions.remove(id)
            && let Err(error) = replaced.shutdown()
        {
            store_reap_error(&self.reap_error, replaced.describe_error(id, &error));
        }
        if sessions.len() >= MAX_SESSIONS {
            reap_sessions(&mut sessions, &self.reap_error);
        }
        if sessions.len() >= MAX_SESSIONS {
            return Err("recorder session capacity exceeded; reap retired taps first".into());
        }
        let (session, tap) = Session::new(channels)?;
        sessions.insert(id.to_owned(), session);
        Ok(tap)
    }

    /// Returns the latest writer state; an unknown identity is stopped/unconfigured.
    #[must_use]
    pub fn status(&self, id: &str) -> RecorderStatus {
        self.sessions
            .lock()
            .get(id)
            .map_or_else(RecorderStatus::default, Session::status)
    }

    /// Execute and acknowledge a writer command on the control thread.
    /// Stop/Pause cut off producers, drain accepted packets and flush conversion
    /// tails before returning. Stop also finalizes and closes the file.
    /// # Errors
    /// Reports absent identities, invalid transitions, file/conversion failures,
    /// and disconnected writer threads. These errors never stop the audio graph.
    pub fn command(&self, id: &str, request: RecorderRequest) -> Result<RecorderStatus, String> {
        self.sessions
            .lock()
            .get(id)
            .ok_or("recorder identity is not prepared")?
            .command(request)
    }

    /// Stop/drain/join sessions with no live taps after render-plan retirement.
    /// No caller-supplied node list can invalidate an active or pending plan.
    /// # Errors
    /// Reports worker shutdown failures after all eligible sessions are reaped.
    pub fn reap_unused(&self) -> Result<(), String> {
        reap_sessions(&mut self.sessions.lock(), &self.reap_error);
        self.reap_error.lock().take().map_or(Ok(()), Err)
    }

    /// Finalize every take during graceful service shutdown, even while render
    /// plans still own taps. Workers remain registered and all taps stay stopped.
    /// # Errors
    /// Aggregates recorder identities, paths and any drain/finalization failures.
    pub fn stop_all(&self) -> Result<(), String> {
        let mut failures = Vec::new();
        for (id, session) in self.sessions.lock().iter() {
            let result = session
                .command(RecorderRequest {
                    action: crate::RecorderAction::Stop,
                    path: None,
                    sample_rate: None,
                    encoding: None,
                })
                .and_then(|status| status.error.map_or(Ok(()), Err));
            if let Err(error) = result {
                failures.push(session.describe_error(id, &error));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

fn reap_sessions(sessions: &mut BTreeMap<String, Session>, error: &Mutex<Option<String>>) {
    sessions.retain(|id, session| {
        if session.transport.taps.load(Ordering::SeqCst) != 0 {
            return true;
        }
        if let Err(failure) = session.shutdown() {
            store_reap_error(error, session.describe_error(id, &failure));
        }
        false
    });
}

fn store_reap_error(slot: &Mutex<Option<String>>, failure: String) {
    let mut error = slot.lock();
    if let Some(previous) = error.as_mut() {
        previous.push_str("; ");
        previous.push_str(&failure);
    } else {
        *error = Some(failure);
    }
}

struct Session {
    transport: Arc<Transport>,
    status: Arc<Mutex<RecorderStatus>>,
    sender: mpsc::Sender<Command>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    fn new(channels: u16) -> Result<(Self, RecorderTap), String> {
        let transport = Transport::new(channels);
        let status = Arc::new(Mutex::new(RecorderStatus::default()));
        let tap = RecorderTap::new(Arc::clone(&transport));
        let (sender, receiver) = mpsc::channel();
        let worker = Worker::new(Arc::clone(&transport), Arc::clone(&status));
        let handle = std::thread::Builder::new()
            .name("tonenari-wav-writer".into())
            .spawn(move || worker.run(&receiver))
            .map_err(|error| format!("cannot start recording writer: {error}"))?;
        Ok((
            Self {
                transport,
                status,
                sender,
                worker: Some(handle),
            },
            tap,
        ))
    }

    fn tap(&self) -> RecorderTap {
        RecorderTap::new(Arc::clone(&self.transport))
    }

    fn status(&self) -> RecorderStatus {
        let mut status = self.status.lock().clone();
        if let Some(error) = transport::fault(self.transport.gate.load(Ordering::SeqCst)) {
            status.state = RecorderState::Failed;
            status.error = Some(error.into());
        }
        status
    }

    fn command(&self, request: RecorderRequest) -> Result<RecorderStatus, String> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(Command::Request(request, reply))
            .map_err(|_| "recording writer disconnected")?;
        receiver
            .recv()
            .map_err(|_| "recording writer did not acknowledge command")?
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let _ = self.sender.send(Command::Shutdown);
        worker.join().map_err(|_| "recording writer panicked")?;
        let status = self.status();
        status.error.map_or(Ok(()), Err)
    }

    fn describe_error(&self, id: &str, error: &str) -> String {
        let status = self.status();
        format!(
            "recorder {id} [{}]: {error}",
            status.path.as_deref().unwrap_or("no file")
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
