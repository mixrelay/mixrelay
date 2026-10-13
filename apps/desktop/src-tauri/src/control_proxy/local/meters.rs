//! The runtime's single engine meter stream, fanned out to every subscribed editor.
//! Each editor has its own mailbox, so a slow editor never delays the engine stream or
//! another editor; it keeps the latest window with the interval maximum.
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use mixrelay_control_protocol::MeterTelemetry;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite};
use tokio::sync::watch;

use crate::control_proxy::telemetry::{EventParser, Mailbox, SILENCE_LIMIT};

/// Bounds opening the engine stream, which the shared HTTP client does not.
pub(crate) const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// Where engine meter windows come from: the engine's server-sent event stream.
pub(super) trait MeterSource: Send + Sync + 'static {
    type Chunks: MeterChunks;
    fn open(&self) -> impl Future<Output = Result<Self::Chunks, String>> + Send;
}

/// The body of an open meter stream.
pub(super) trait MeterChunks: Send + 'static {
    /// The next body chunk, `None` at the end of the stream.
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, String>> + Send;
}

impl MeterSource for super::LocalControl {
    type Chunks = reqwest::Response;

    fn open(&self) -> impl Future<Output = Result<Self::Chunks, String>> + Send {
        self.meter_stream()
    }
}

impl MeterChunks for reqwest::Response {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|error| error.to_string())
    }
}

pub(super) struct MeterHub<S> {
    source: Arc<S>,
    stop: watch::Receiver<bool>,
    readers: Mutex<Readers>,
}

#[derive(Default)]
struct Readers {
    mailboxes: Vec<Weak<Mailbox>>,
    /// The running upstream's generation; `None` when no upstream runs.
    upstream: Option<u64>,
    generations: u64,
}

impl<S: MeterSource> MeterHub<S> {
    pub(super) fn new(source: Arc<S>, stop: watch::Receiver<bool>) -> Arc<Self> {
        Arc::new(Self {
            source,
            stop,
            readers: Mutex::default(),
        })
    }

    fn readers(&self) -> std::sync::MutexGuard<'_, Readers> {
        self.readers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a mailbox, opening the engine stream if no other editor holds it open.
    /// Dropping the mailbox unsubscribes; the stream closes with its last reader.
    pub(super) fn subscribe(self: &Arc<Self>) -> Arc<Mailbox> {
        let mailbox = Arc::new(Mailbox::default());
        let mut readers = self.readers();
        readers.mailboxes.push(Arc::downgrade(&mailbox));
        if readers.upstream.is_none() {
            readers.generations += 1;
            let generation = readers.generations;
            readers.upstream = Some(generation);
            tokio::spawn(Arc::clone(self).upstream(generation));
        }
        mailbox
    }

    async fn upstream(self: Arc<Self>, generation: u64) {
        let mut stop = self.stop.clone();
        let reason = tokio::select! {
            biased;
            _ = stop.changed() => "runtime stopped".to_owned(),
            reason = self.relay() => reason,
        };
        let mut readers = self.readers();
        // An upstream that already gave up ownership must not end its successor's readers.
        if readers.upstream == Some(generation) {
            readers.upstream = None;
            for mailbox in readers
                .mailboxes
                .drain(..)
                .filter_map(|weak| weak.upgrade())
            {
                mailbox.end(&reason);
            }
        }
    }

    /// Publishes windows until the engine stream fails, falls silent, or loses its readers.
    async fn relay(&self) -> String {
        let mut chunks = match tokio::time::timeout(OPEN_TIMEOUT, self.source.open()).await {
            Ok(Ok(chunks)) => chunks,
            Ok(Err(reason)) => return reason,
            Err(_) => return "telemetry stream did not open".to_owned(),
        };
        let mut parser = EventParser::default();
        loop {
            let chunk = match tokio::time::timeout(SILENCE_LIMIT, chunks.next_chunk()).await {
                Ok(Ok(Some(chunk))) => chunk,
                Ok(Ok(None)) => return "telemetry stream ended".to_owned(),
                Ok(Err(reason)) => return reason,
                Err(_) => return "telemetry stream stalled".to_owned(),
            };
            let events = match parser.push(&chunk) {
                Ok(events) => events,
                Err(reason) => return reason,
            };
            for event in events {
                let Ok(window) = serde_json::from_str::<MeterTelemetry>(&event) else {
                    continue;
                };
                if !self.publish(&window) {
                    return "no meter readers".to_owned();
                }
            }
        }
    }

    /// Delivers a window to every live mailbox. With none left, gives up the upstream
    /// under the same lock a new reader would take, so that reader starts a fresh one.
    /// Only the running upstream publishes: ownership changes only once it gave up.
    fn publish(&self, window: &MeterTelemetry) -> bool {
        let mut readers = self.readers();
        readers.mailboxes.retain(|weak| match weak.upgrade() {
            Some(mailbox) => {
                mailbox.deliver(window.clone());
                true
            }
            None => false,
        });
        if readers.mailboxes.is_empty() {
            readers.upstream = None;
            return false;
        }
        true
    }
}

/// Writes one editor's windows as `206` frames until the engine stream ends (a terminal
/// `503` frame), the editor closes or writes anything, or the runtime stops.
pub(super) async fn relay<S, R, W>(
    id: u64,
    hub: &Arc<MeterHub<S>>,
    read: &mut R,
    write: &mut W,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()>
where
    S: MeterSource,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mailbox = hub.subscribe();
    let mut probe = [0_u8; 1];
    loop {
        let next = tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            // A subscriber sends nothing more: data or end of file ends the relay.
            _ = read.read(&mut probe) => return Ok(()),
            next = mailbox.take() => next,
        };
        let (frame, last) = match next {
            Ok(window) => (
                serde_json::json!({"id": id, "status": 206, "body": window}),
                false,
            ),
            Err(reason) => (
                serde_json::json!({
                    "id": id,
                    "status": 503,
                    "body": {"code": "control_unavailable", "parameters": {"reason": reason}},
                }),
                true,
            ),
        };
        // A blocked write still yields to the runtime stopping or the editor leaving.
        tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            _ = read.read(&mut probe) => return Ok(()),
            written = super::socket::write_frame(write, &frame) => written?,
        }
        if last {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests;
