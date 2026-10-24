//! The editor's one connection to the runtime. Commands and telemetry subscriptions share
//! it, each frame tagged with its request ID, so a slow command never holds up another
//! command or a stream and the editor needs one runtime connection slot.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mixrelay_control_protocol::TelemetryDecoder;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

use crate::control_proxy::telemetry::Mailbox;

/// Frames queued for the writer; a sender that waits longer is told the runtime stalled.
const OUTBOUND_FRAMES: usize = 32;

pub(super) enum Route {
    /// Every reply frame of one command, the last one terminal.
    Command(mpsc::UnboundedSender<Value>),
    /// Decoded windows of one subscription; the stream's own decoder holds its bindings.
    Telemetry {
        mailbox: Arc<Mailbox>,
        decoder: TelemetryDecoder,
    },
}

#[derive(Default)]
struct Routes {
    live: HashMap<u64, Route>,
    closed: bool,
    /// When the last route ended; `None` while any route is open.
    idle_since: Option<Instant>,
}

pub(super) struct Link {
    routes: Mutex<Routes>,
    outbound: mpsc::Sender<Vec<u8>>,
    tasks: Mutex<Vec<tokio::task::AbortHandle>>,
}

impl Link {
    /// Starts the reader and writer for a verified stream.
    pub(super) fn start(stream: UnixStream, write_timeout: Duration) -> Arc<Self> {
        let (read, write) = stream.into_split();
        let (outbound, frames) = mpsc::channel(OUTBOUND_FRAMES);
        let link = Arc::new(Self {
            routes: Mutex::new(Routes {
                idle_since: Some(Instant::now()),
                ..Routes::default()
            }),
            outbound,
            tasks: Mutex::default(),
        });
        let reader = tokio::spawn(Arc::clone(&link).read(read));
        let writer = tokio::spawn(Arc::clone(&link).write(write, frames, write_timeout));
        *link.tasks.lock().unwrap_or_else(PoisonError::into_inner) =
            vec![reader.abort_handle(), writer.abort_handle()];
        // A peer that left at once may have closed the link before the tasks were
        // registered; closing again stops them now that they are.
        if link.routes().closed {
            link.close();
        }
        link
    }

    fn routes(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a route unless the link closed or idled past `reuse`; a link idle that
    /// long may be one the runtime is about to close, so it is retired instead.
    pub(super) fn register(&self, id: u64, route: Route, reuse: Duration) -> bool {
        let mut routes = self.routes();
        if routes
            .idle_since
            .is_some_and(|since| since.elapsed() >= reuse)
        {
            drop(routes);
            self.close();
            return false;
        }
        if routes.closed {
            return false;
        }
        routes.live.insert(id, route);
        routes.idle_since = None;
        true
    }

    pub(super) fn remove(&self, id: u64) {
        let mut routes = self.routes();
        routes.live.remove(&id);
        if routes.live.is_empty() && routes.idle_since.is_none() {
            routes.idle_since = Some(Instant::now());
        }
    }

    /// Queues a frame; `false` means the writer never took it, so it was never sent.
    /// The caller decides what a refused frame means for its route and this link.
    pub(super) async fn send(&self, frame: Vec<u8>, wait: Duration) -> bool {
        matches!(
            tokio::time::timeout(wait, self.outbound.send(frame)).await,
            Ok(Ok(()))
        )
    }

    /// Ends a subscription: forgets its route and tells the runtime. A full queue sends
    /// the cancel once it drains; if it cannot, the link closes, which the runtime also
    /// treats as the end of every subscription on it.
    pub(super) fn cancel(self: &Arc<Self>, id: u64, wait: Duration) {
        self.remove(id);
        let frame = json!({"id": 0, "command": {"type": "cancel", "subscription": id}});
        let frame = format!("{frame}\n").into_bytes();
        let Err(mpsc::error::TrySendError::Full(frame)) = self.outbound.try_send(frame) else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return self.close();
        };
        let link = Arc::clone(self);
        runtime.spawn(async move {
            if !link.send(frame, wait).await {
                link.close();
            }
        });
    }

    /// Fails every open route and stops both tasks. Commands learn the runtime may have
    /// received them; subscriptions end.
    pub(super) fn close(&self) {
        let live = {
            let mut routes = self.routes();
            routes.closed = true;
            std::mem::take(&mut routes.live)
        };
        for route in live.into_values() {
            if let Route::Telemetry { mailbox, .. } = route {
                mailbox.end("runtime connection closed");
            }
        }
        for task in self
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }

    async fn write(
        self: Arc<Self>,
        mut write: OwnedWriteHalf,
        mut frames: mpsc::Receiver<Vec<u8>>,
        timeout: Duration,
    ) {
        while let Some(frame) = frames.recv().await {
            if !matches!(
                tokio::time::timeout(timeout, write.write_all(&frame)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        self.close();
    }

    async fn read(self: Arc<Self>, read: OwnedReadHalf) {
        let limit = crate::control_proxy::local::socket::RESPONSE_LIMIT;
        let mut reader = BufReader::new(read);
        loop {
            let mut line = Vec::new();
            let complete = (&mut reader)
                .take(limit as u64 + 1)
                .read_until(b'\n', &mut line)
                .await
                .is_ok_and(|_| line.len() <= limit && line.last() == Some(&b'\n'));
            let Some(frame) = complete
                .then(|| serde_json::from_slice::<Value>(&line).ok())
                .flatten()
            else {
                break;
            };
            self.route(frame);
        }
        self.close();
    }

    /// Delivers one frame. Frames for routes that already ended are dropped.
    fn route(&self, mut frame: Value) {
        let Some(id) = frame["id"].as_u64() else {
            return;
        };
        let mut routes = self.routes();
        let ended = match routes.live.get_mut(&id) {
            None => return,
            Some(Route::Command(replies)) => {
                let terminal = frame["status"] != 102;
                let _ = replies.send(frame);
                terminal
            }
            Some(Route::Telemetry { mailbox, decoder }) => {
                if frame["status"] == 206 {
                    match decoder.json(frame["body"].take()) {
                        Ok(window) => {
                            mailbox.deliver(window);
                            false
                        }
                        Err(reason) => {
                            mailbox.end(&reason);
                            true
                        }
                    }
                } else {
                    let reason = frame["body"]["parameters"]["reason"].as_str();
                    mailbox.end(reason.unwrap_or("meter push unavailable"));
                    true
                }
            }
        };
        if ended {
            routes.live.remove(&id);
            if routes.live.is_empty() {
                routes.idle_since = Some(Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests;
