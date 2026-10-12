//! One user-owned controller keeps audio alive independently of editor windows.
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
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

mod dispatch;
pub(crate) mod menu;
pub(crate) mod startup;
pub(crate) use dispatch::{dispatch, is_service_command};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub(crate) struct Connection {
    path: PathBuf,
    uid: u32,
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
        })
    }

    pub(crate) async fn invoke(&self, name: &str, arguments: Value) -> Result<Value, Value> {
        self.exchange(name, arguments)
            .await
            .map_err(Value::String)?
    }

    async fn exchange(&self, name: &str, arguments: Value) -> Result<Result<Value, Value>, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let mut frame = serde_json::to_vec(
            &json!({"id":id,"command":{"type":"invoke","name":name,"arguments":arguments}}),
        )
        .map_err(|e| e.to_string())?;
        frame.push(b'\n');
        if frame.len() as u64 > crate::control_proxy::local::socket::FRAME_LIMIT {
            return Err("background request is too large".into());
        }
        let mut stream = tokio::time::timeout(EXCHANGE_TIMEOUT, UnixStream::connect(&self.path))
            .await
            .map_err(|_| "background connection timed out")?
            .map_err(|e| e.to_string())?;
        if !stream.peer_cred().is_ok_and(|peer| peer.uid() == self.uid) {
            return Err("background controller belongs to a different user".into());
        }
        tokio::time::timeout(EXCHANGE_TIMEOUT, stream.write_all(&frame))
            .await
            .map_err(|_| "background request delivery is uncertain")?
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        loop {
            let mut reply = Vec::new();
            tokio::time::timeout(
                EXCHANGE_TIMEOUT,
                (&mut reader)
                    .take(crate::control_proxy::local::socket::RESPONSE_LIMIT as u64 + 1)
                    .read_until(b'\n', &mut reply),
            )
            .await
            .map_err(|_| "background response is uncertain; command was not replayed")?
            .map_err(|e| e.to_string())?;
            if reply.len() > crate::control_proxy::local::socket::RESPONSE_LIMIT
                || reply.last() != Some(&b'\n')
            {
                return Err("background disconnected; command was not replayed".into());
            }
            let reply: Value =
                serde_json::from_slice(&reply).map_err(|_| "invalid background reply")?;
            if reply["id"] != id {
                return Err("background reply identity mismatch".into());
            }
            if reply["status"] == 102 {
                continue;
            }
            if reply["status"] != 200 {
                return Err("background controller unavailable".into());
            }
            if let Some(value) = reply["body"].get("ok") {
                return Ok(Ok(value.clone()));
            }
            if let Some(error) = reply["body"].get("error") {
                return Ok(Err(error.clone()));
            }
            return Err("invalid background result".into());
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
        retire_background_agent()?;
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
        let result = match mixrelay_driver_control::DriverClient::new() {
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
