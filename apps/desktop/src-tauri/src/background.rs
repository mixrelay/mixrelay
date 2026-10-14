//! One user-owned controller keeps audio alive independently of editor windows.
use mixrelay_control_protocol::ControlTelemetry;
use serde_json::{Value, json};
use std::os::unix::process::CommandExt as _;
use std::{
    os::unix::fs::MetadataExt,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::Manager as _;
use tokio::net::UnixStream;

mod dispatch;
mod link;
pub(crate) mod menu;
pub(crate) mod startup;
pub(crate) use dispatch::{dispatch, is_service_command};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(3);
/// Covers the runtime opening the engine stream (`local::meters::OPEN_TIMEOUT`) and its
/// first window.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(8);
/// The editor↔runtime link version: one connection multiplexes commands and streams.
pub(crate) const LINK_PROTOCOL: u64 = 3;

/// How long an idle link stays reusable: well inside the controller's idle timeout, so a
/// reused link is never one it is closing for idleness. A controller exiting between the
/// check and the write fails that request as an exit mid-request would; it is never
/// replayed.
const REUSE_WINDOW: Duration = Duration::from_secs(10);
const _: () = assert!(
    REUSE_WINDOW.as_secs() * 2 < crate::control_proxy::local::socket::IDLE_TIMEOUT.as_secs()
);

#[derive(Clone)]
pub(crate) struct Connection {
    path: PathBuf,
    uid: u32,
    link: Arc<tokio::sync::Mutex<Option<Arc<link::Link>>>>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Connection {
    pub(crate) fn for_app(app: &tauri::AppHandle) -> Result<Self, String> {
        let uid = std::fs::metadata(app.path().home_dir().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .uid();
        let directory = runtime_directory(app)?;
        Ok(Self {
            path: directory.join("control/control.sock"),
            uid,
            link: Arc::default(),
        })
    }

    pub(crate) async fn invoke(&self, name: &str, arguments: Value) -> Result<Value, Value> {
        self.exchange(name, arguments)
            .await
            .map_err(Value::String)?
    }

    async fn connect(&self) -> Result<UnixStream, String> {
        let stream = tokio::time::timeout(EXCHANGE_TIMEOUT, UnixStream::connect(&self.path))
            .await
            .map_err(|_| "background connection timed out")?
            .map_err(|e| e.to_string())?;
        if !stream.peer_cred().is_ok_and(|peer| peer.uid() == self.uid) {
            return Err("background controller belongs to a different user".into());
        }
        Ok(stream)
    }

    /// Registers a route on the current link, opening a link when there is none or the
    /// current one can no longer take new work.
    async fn open(
        &self,
        id: u64,
        route: &impl Fn() -> link::Route,
    ) -> Result<Arc<link::Link>, String> {
        let mut current = self.link.lock().await;
        if let Some(link) = current.as_ref()
            && link.register(id, route(), REUSE_WINDOW)
        {
            return Ok(Arc::clone(link));
        }
        let link = link::Link::start(self.connect().await?, EXCHANGE_TIMEOUT);
        if !link.register(id, route(), REUSE_WINDOW) {
            return Err("background connection closed".into());
        }
        *current = Some(Arc::clone(&link));
        Ok(link)
    }

    /// Registers a route and queues its request. A request the runtime cannot have
    /// received is queued once more on a fresh link; nothing else is ever replayed.
    async fn start(
        &self,
        id: u64,
        frame: &[u8],
        route: impl Fn() -> link::Route,
    ) -> Result<Arc<link::Link>, String> {
        if frame.len() as u64 > crate::control_proxy::local::socket::FRAME_LIMIT {
            return Err("background request is too large".into());
        }
        for _ in 0..2 {
            let link = self.open(id, &route).await?;
            if link.send(frame.to_vec(), EXCHANGE_TIMEOUT).await {
                return Ok(link);
            }
            // Forget the route first: closing must not end what the retry registers anew.
            link.remove(id);
            link.close();
        }
        Err("background connection unavailable".into())
    }

    /// Subscribes to pushed telemetry on the shared link. Succeeds only once the first
    /// window arrives, so an engine or runtime without push is an error the caller can
    /// fall back from, not a stream that ends at once.
    pub(crate) async fn subscribe_telemetry(&self) -> Result<StartedTelemetrySubscription, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let frame = format!(
            "{}\n",
            json!({"id": id, "command": {"type": "subscribe_telemetry"}})
        );
        // Every attempt registers a fresh mailbox: one a failed attempt's link ended must
        // not end the retry. The last one created is the one registered.
        let registered = std::sync::Mutex::new(None);
        let link = self
            .start(id, frame.as_bytes(), || {
                let mailbox = Arc::new(crate::control_proxy::telemetry::Mailbox::default());
                *registered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(Arc::clone(&mailbox));
                link::Route::Telemetry {
                    mailbox,
                    decoder: mixrelay_control_protocol::TelemetryDecoder::default(),
                }
            })
            .await?;
        let mailbox = registered
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ok_or("meter subscription was not registered")?;
        let subscription = TelemetrySubscription { id, link, mailbox };
        // The runtime may first open the engine stream (bounded by its own timeout).
        match tokio::time::timeout(SUBSCRIBE_TIMEOUT, subscription.mailbox.take()).await {
            Ok(Ok(window)) => Ok(StartedTelemetrySubscription {
                first: Some(window),
                stream: subscription,
            }),
            Ok(Err(reason)) => Err(reason),
            Err(_) => Err("meter subscription timed out".to_owned()),
        }
    }

    async fn exchange(&self, name: &str, arguments: Value) -> Result<Result<Value, Value>, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let mut frame = serde_json::to_vec(
            &json!({"id":id,"command":{"type":"invoke","name":name,"arguments":arguments}}),
        )
        .map_err(|e| e.to_string())?;
        frame.push(b'\n');
        let (replies, mut received) = tokio::sync::mpsc::unbounded_channel();
        let link = self
            .start(id, &frame, || link::Route::Command(replies.clone()))
            .await?;
        // The route holds the only sender: a closed link ends `received`.
        drop(replies);
        loop {
            let reply = match tokio::time::timeout(EXCHANGE_TIMEOUT, received.recv()).await {
                Ok(Some(reply)) => reply,
                Ok(None) => return Err("background disconnected; command was not replayed".into()),
                Err(_) => {
                    link.remove(id);
                    return Err("background response is uncertain; command was not replayed".into());
                }
            };
            if reply["status"] == 102 {
                continue;
            }
            if reply["status"] != 200 {
                return Err("background controller unavailable".into());
            }
            return if let Some(value) = reply["body"].get("ok") {
                Ok(Ok(value.clone()))
            } else if let Some(error) = reply["body"].get("error") {
                Ok(Err(error.clone()))
            } else {
                Err("invalid background result".into())
            };
        }
    }

    pub(crate) async fn running(&self) -> bool {
        self.invoke("hello", Value::Null)
            .await
            .is_ok_and(|hello| startup::compatible(&hello))
    }
}

pub(crate) struct ControllerLaunch {
    exited: Arc<AtomicBool>,
}

impl ControllerLaunch {
    fn exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }
}

pub(crate) fn spawn_controller(executable: &std::path::Path) -> std::io::Result<ControllerLaunch> {
    #[cfg(target_os = "macos")]
    if !tauri::is_dev()
        && executable
            == std::path::Path::new("/Applications/MixRelay.app/Contents/MacOS/mixrelay-desktop")
    {
        retire_or_confirm_absent()?;
        let launcher = std::env::current_exe()?
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or_else(|| std::io::Error::other("installed app bundle unavailable"))?
            .join("Resources/mixrelay-driver-launcher");
        return spawn_reaped_controller(&launcher, false);
    }
    spawn_reaped_controller(executable, true)
}

fn spawn_reaped_controller(
    executable: &std::path::Path,
    runtime_argument: bool,
) -> std::io::Result<ControllerLaunch> {
    let mut command = std::process::Command::new(executable);
    if runtime_argument {
        command.arg("--driver-runtime");
    }
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let id = child.id();
    let exited = Arc::new(AtomicBool::new(false));
    let observed_exit = Arc::clone(&exited);
    // There is no editor-owned shutdown pipe. Reaping never changes controller lifetime.
    std::thread::Builder::new()
        .name("mixrelay-controller-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) => {
                observed_exit.store(true, Ordering::Release);
                tracing::info!(
                    pid = id,
                    code = status.code(),
                    "launched Driver runtime exited"
                );
            }
            Err(error) => {
                tracing::warn!(%error, pid = id, "could not observe launched Driver runtime exit");
            }
        })?;
    Ok(ControllerLaunch { exited })
}

/// Removing the agent registration left by earlier builds is cleanup, not a precondition:
/// a failed unregister once kept every Reconnect from starting the runtime. Starting
/// directly is safe only when launchd holds no such job for this user, since a loaded
/// job could relaunch the runtime after Quit releases its lease.
#[cfg(target_os = "macos")]
fn retire_or_confirm_absent() -> std::io::Result<()> {
    let Err(error) = retire_background_agent() else {
        return Ok(());
    };
    // The session owner, as the editor's own control connection resolves it.
    let home = std::env::var_os("HOME").ok_or_else(|| std::io::Error::other("HOME is unset"))?;
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(home)?);
    let target = format!("gui/{uid}/io.mixrelay.driver.runtime");
    let loaded = std::process::Command::new("/bin/launchctl")
        .args(["print", &target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success();
    if loaded {
        tracing::error!(%error, "previous background agent is still loaded and could not be retired");
        return Err(error);
    }
    tracing::warn!(%error, "previous background agent cleanup failed; none is loaded, starting directly");
    Ok(())
}

#[cfg(target_os = "macos")]
fn retire_background_agent() -> std::io::Result<()> {
    unsafe extern "C" {
        fn MRRetireBackgroundAgent(message: *mut std::ffi::c_char, capacity: usize) -> i32;
    }
    let mut message = [0_u8; 1024];
    // SAFETY: the fixed native bridge writes a NUL-terminated diagnostic within
    // this live buffer and retires only the old plist in the calling app bundle.
    let status = unsafe { MRRetireBackgroundAgent(message.as_mut_ptr().cast(), message.len()) };
    if status != 0 {
        let end = message
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(message.len());
        return Err(std::io::Error::other(String::from_utf8_lossy(
            &message[..end],
        )));
    }
    Ok(())
}

pub(crate) fn spawn_editor(executable: &std::path::Path) -> std::io::Result<()> {
    let mut child = std::process::Command::new(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    std::thread::Builder::new()
        .name("mixrelay-editor-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

/// Only the documented native command set is forwarded; dialogs stay in the editor.
pub(crate) fn forward(invoke: tauri::ipc::Invoke) -> Option<tauri::ipc::Invoke> {
    if !is_service_command(invoke.message.command()) {
        return Some(invoke);
    }
    let app = invoke.message.webview().app_handle().clone();
    if *app.state::<crate::app_mode::AppMode>() != crate::app_mode::AppMode::Graph {
        return Some(invoke);
    }
    let name = invoke.message.command().to_owned();
    let tauri::ipc::InvokeBody::Json(arguments) = invoke.message.payload() else {
        invoke
            .resolver
            .reject("background commands require JSON arguments");
        return None;
    };
    let arguments = arguments.clone();
    invoke.resolver.respond_async(async move {
        let connection = app
            .try_state::<Connection>()
            .map(|state| state.inner().clone())
            .ok_or_else(|| {
                tauri::ipc::InvokeError(Value::String("background controller unavailable".into()))
            })?;
        connection
            .invoke(&name, arguments)
            .await
            .map_err(tauri::ipc::InvokeError)
    });
    None
}

/// A fresh controller activation restores installed devices once. Reopening the
/// editor never reapplies Driver state or changes an existing graph.
pub(crate) fn restore_driver(app: &tauri::AppHandle) {
    if crate::driver_control::installed_driver_version().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(target_os = "macos")]
        let result = app
            .state::<crate::engine_lifecycle::EngineLifecycle>()
            .restore_driver()
            .await;
        #[cfg(target_os = "linux")]
        let result = match mixrelay_driver_control::DriverClient::session() {
            Ok(client) => client
                .set_active(true)
                .await
                .map(Some)
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(Some(snapshot)) => {
                if let Some(menu) = app.try_state::<crate::driver_manager::DriverMenu>() {
                    menu.observe_snapshot(&snapshot);
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "Driver activation unavailable; physical audio remains available");
            }
        }
        drop(app);
    });
}

#[cfg(test)]
mod tests;

/// Reconnecting is an explicit editor action; background polling never restarts stopped audio.
pub(crate) fn reopen_editor(app: &tauri::AppHandle) {
    if let Err(error) = crate::app_menu::show_main_window(app) {
        tracing::error!(%error, "could not reopen the editor");
    }
    // The visible startup gate owns explicit reconnect attempts.
}

/// Keep existing user preferences in their established location when runtime ownership moves.
pub(crate) fn settings_directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    shared_directory(app, "io.mixrelay.desktop")
}

pub(crate) fn runtime_directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    shared_directory(app, "io.mixrelay.driver.desktop")
}

fn shared_directory(app: &tauri::AppHandle, identifier: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    Ok(directory
        .parent()
        .ok_or("application data has no parent")?
        .join(identifier))
}

/// Pushed telemetry on the shared runtime link; dropping it cancels the subscription.
pub(crate) struct TelemetrySubscription {
    id: u64,
    link: Arc<link::Link>,
    mailbox: Arc<crate::control_proxy::telemetry::Mailbox>,
}

impl Drop for TelemetrySubscription {
    fn drop(&mut self) {
        self.link.cancel(self.id, EXCHANGE_TIMEOUT);
    }
}

/// A subscription whose first window has been read but not yet returned.
pub(crate) struct StartedTelemetrySubscription {
    first: Option<ControlTelemetry>,
    stream: TelemetrySubscription,
}

impl StartedTelemetrySubscription {
    /// The next window, or `None` once the runtime ends the stream, the link breaks, or it
    /// falls silent although the engine publishes continuously.
    pub(crate) async fn next(&mut self) -> Option<ControlTelemetry> {
        if let Some(window) = self.first.take() {
            return Some(window);
        }
        tokio::time::timeout(
            crate::control_proxy::telemetry::SILENCE_LIMIT,
            self.stream.mailbox.take(),
        )
        .await
        .ok()?
        .ok()
    }
}
