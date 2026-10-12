//! Explicit editor connection to the installed Driver's user-session runtime.
//! Read-only health checks never restart audio or replay mutations.
use super::{Connection, EXCHANGE_TIMEOUT, spawn_controller};
use serde::Serialize;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tauri::Manager as _;

static CONNECT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const START_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeStatus {
    Ready,
    NotInstalled,
    Incompatible,
    Unavailable,
}

pub(super) fn compatible(hello: &Value) -> bool {
    hello["protocolVersion"] == 2
        && hello["product"] == "mixrelay-driver"
        && hello["version"].as_str().is_some_and(supported_version)
}

fn supported_version(version: &str) -> bool {
    version == env!("CARGO_PKG_VERSION")
}

async fn readiness(connection: &Connection) -> RuntimeStatus {
    let Ok(hello) = connection.invoke("hello", Value::Null).await else {
        return RuntimeStatus::Unavailable;
    };
    if !compatible(&hello) {
        return RuntimeStatus::Incompatible;
    }
    // A socket alone is not readiness: the owned engine must answer as well.
    match tokio::time::timeout(
        EXCHANGE_TIMEOUT,
        connection.invoke("control_get_status", Value::Null),
    )
    .await
    {
        Ok(Ok(response)) if response["status"] == 200 => RuntimeStatus::Ready,
        _ => RuntimeStatus::Unavailable,
    }
}

#[tauri::command]
pub(crate) async fn runtime_status(app: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    let connection = app.state::<Connection>();
    let status = readiness(&connection).await;
    let status = if status == RuntimeStatus::Unavailable && !driver_executable().is_file() {
        RuntimeStatus::NotInstalled
    } else {
        status
    };
    publish_readiness(&app, status);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn runtime_connect(app: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    let _connect = CONNECT.lock().await;
    let connection = app.state::<Connection>();
    let status = readiness(&connection).await;
    if status != RuntimeStatus::Unavailable {
        publish_readiness(&app, status);
        return Ok(status);
    }
    publish_readiness(&app, status);
    let executable = driver_executable();
    if !executable.is_file() {
        return Ok(RuntimeStatus::NotInstalled);
    }
    #[cfg(target_os = "macos")]
    if !tauri::is_dev() {
        let version = installed_runtime_version().await;
        if !version.as_deref().is_some_and(supported_version) {
            return Ok(RuntimeStatus::Incompatible);
        }
    }
    let status = connect_until_ready(
        &connection,
        &executable,
        START_TIMEOUT,
        controller_admission_available,
    )
    .await?;
    publish_readiness(&app, status);
    Ok(status)
}

// Cold launch can race a finishing update/previous runtime. Only a child whose
// exit we actually reaped may be retried, inside this explicit Connect action.
async fn connect_until_ready(
    connection: &Connection,
    executable: &std::path::Path,
    timeout: Duration,
    admission_available: impl Fn() -> Result<bool, String>,
) -> Result<RuntimeStatus, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut launched: Option<super::ControllerLaunch> = None;
    let mut attempts = 0;
    let mut next_launch = tokio::time::Instant::now();
    loop {
        let status = tokio::time::timeout_at(deadline, readiness(connection)).await;
        match status {
            Ok(RuntimeStatus::Ready | RuntimeStatus::Incompatible) => {
                let status = status.expect("matched completed health check");
                return Ok(status);
            }
            Err(_) => return Ok(RuntimeStatus::Unavailable),
            _ => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(RuntimeStatus::Unavailable);
        }
        if launched
            .as_ref()
            .is_none_or(super::ControllerLaunch::exited)
            && tokio::time::Instant::now() >= next_launch
        {
            if attempts == 3 {
                return Ok(RuntimeStatus::Unavailable);
            }
            // A controller answering hello with an unavailable engine remains
            // owned by that process. Health polling never restarts it.
            let running = tokio::time::timeout_at(deadline, connection.running()).await;
            let Ok(running) = running else {
                return Ok(RuntimeStatus::Unavailable);
            };
            if !running && admission_available()? {
                launched = Some(spawn_controller(executable).map_err(|error| error.to_string())?);
                attempts += 1;
                next_launch = tokio::time::Instant::now() + Duration::from_secs(1);
            }
        }
        tokio::time::sleep_until(
            deadline.min(tokio::time::Instant::now() + Duration::from_millis(200)),
        )
        .await;
    }
}

fn controller_admission_available() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    if !tauri::is_dev() {
        use mixrelay_macos_transport::app_lifecycle::{CONTROLLER_LEASE, ENGINE_LEASE, is_owned};
        return Ok(
            !is_owned(CONTROLLER_LEASE).map_err(|error| error.to_string())?
                && !is_owned(ENGINE_LEASE).map_err(|error| error.to_string())?,
        );
    }
    Ok(true)
}

fn publish_readiness(app: &tauri::AppHandle, status: RuntimeStatus) {
    if let Some(menu) = app.try_state::<crate::workspace_menu::WorkspaceMenu>()
        && let Err(error) = menu.set_ready(status == RuntimeStatus::Ready)
    {
        tracing::error!(%error, "could not update editor menu readiness");
    }
}

fn driver_executable() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Applications/MixRelay.app/Contents/MacOS/mixrelay-desktop");
    #[cfg(target_os = "linux")]
    return PathBuf::from("/usr/bin/mixrelay-desktop");
}

pub(crate) fn editor_executable() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let path = PathBuf::from("/Applications/MixRelay.app/Contents/MacOS/mixrelay-desktop");
    #[cfg(target_os = "linux")]
    let path = PathBuf::from("/usr/bin/mixrelay-desktop");
    if path.is_file() {
        Ok(path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "MixRelay is not installed",
        ))
    }
}

#[cfg(target_os = "macos")]
async fn installed_runtime_version() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let output = std::process::Command::new("/usr/bin/plutil")
            .args([
                "-extract",
                "CFBundleShortVersionString",
                "raw",
                "-o",
                "-",
                "/Applications/MixRelay.app/Contents/Info.plist",
            ])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::MetadataExt as _;
    use tokio::{
        io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
        net::UnixListener,
    };

    async fn serve_connection(listener: UnixListener, status: u16) {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut frame = String::new();
            reader.read_line(&mut frame).await.unwrap();
            let request: Value = serde_json::from_str(&frame).unwrap();
            let body = if request["command"]["name"] == "hello" {
                json!({"protocolVersion":2,"product":"mixrelay-driver","version":env!("CARGO_PKG_VERSION")})
            } else {
                json!({"status":status})
            };
            reader
                .get_mut()
                .write_all(
                    format!(
                        "{}\n",
                        json!({"id":request["id"],"status":200,"body":{"ok":body}})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    }

    fn fixture(directory: &std::path::Path, script: &str) -> (Connection, PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let executable = directory.join("controller.sh");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\ncd \"$(dirname \"$0\")\"\n{script}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        (
            Connection {
                path: directory.join("control.sock"),
                uid: std::fs::metadata(directory).unwrap().uid(),
            },
            executable,
        )
    }

    #[tokio::test]
    async fn exited_first_launch_is_retried_during_the_same_explicit_connection() {
        let directory = tempfile::tempdir().unwrap();
        let (connection, executable) = fixture(
            directory.path(),
            "printf attempt >> attempts\nif test ! -f first; then touch first; exit 2; fi\ntouch ready",
        );
        let path = connection.path.clone();
        let ready = directory.path().join("ready");
        let server = tokio::spawn(async move {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            serve_connection(UnixListener::bind(path).unwrap(), 200).await;
        });
        assert_eq!(
            connect_until_ready(&connection, &executable, Duration::from_secs(4), || Ok(
                true
            ))
            .await
            .unwrap(),
            RuntimeStatus::Ready
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("attempts")).unwrap(),
            "attemptattempt"
        );
        server.abort();
    }

    #[tokio::test]
    async fn busy_owner_and_a_living_failed_engine_are_never_duplicated() {
        let directory = tempfile::tempdir().unwrap();
        let (connection, executable) = fixture(directory.path(), "touch launched");
        assert_eq!(
            connect_until_ready(&connection, &executable, Duration::from_millis(100), || Ok(
                false
            ))
            .await
            .unwrap(),
            RuntimeStatus::Unavailable
        );
        assert!(!directory.path().join("launched").exists());
        let server = tokio::spawn(serve_connection(
            UnixListener::bind(&connection.path).unwrap(),
            503,
        ));
        assert_eq!(
            connect_until_ready(&connection, &executable, Duration::from_millis(100), || Ok(
                true
            ))
            .await
            .unwrap(),
            RuntimeStatus::Unavailable
        );
        assert!(!directory.path().join("launched").exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_broken_executable_has_a_fixed_retry_limit() {
        let directory = tempfile::tempdir().unwrap();
        let (connection, executable) =
            fixture(directory.path(), "printf attempt >> attempts\nexit 2");
        assert_eq!(
            connect_until_ready(&connection, &executable, Duration::from_secs(5), || Ok(
                true
            ))
            .await
            .unwrap(),
            RuntimeStatus::Unavailable
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("attempts")).unwrap(),
            "attemptattemptattempt"
        );
    }

    #[test]
    fn old_app_controller_and_future_protocol_are_not_driver_connections() {
        for hello in [
            json!({"protocolVersion":1,"version":"0.1.2"}),
            json!({"protocolVersion":2,"product":"mixrelay-app","version":"0.1.2"}),
            json!({"protocolVersion":3,"product":"mixrelay-driver","version":"0.1.2"}),
            json!({"protocolVersion":2,"product":"mixrelay-driver","version":"0.1.1"}),
            json!({"protocolVersion":2,"product":"mixrelay-driver","version":"0.1.2"}),
            json!({"protocolVersion":2,"product":"mixrelay-driver","version":"0.2.0"}),
        ] {
            assert!(!compatible(&hello));
        }
        assert!(compatible(
            &json!({"protocolVersion":2,"product":"mixrelay-driver","version":env!("CARGO_PKG_VERSION")})
        ));
    }

    #[tokio::test]
    async fn controller_with_unavailable_engine_never_opens_editor() {
        for code in [200, 503] {
            let directory = tempfile::tempdir().unwrap();
            let connection = Connection {
                path: directory.path().join("control.sock"),
                uid: std::fs::metadata(directory.path()).unwrap().uid(),
            };
            let listener = UnixListener::bind(&connection.path).unwrap();
            let server = tokio::spawn(async move {
                for result in [
                    json!({"protocolVersion":2,"product":"mixrelay-driver","version":env!("CARGO_PKG_VERSION")}),
                    json!({"status":code}),
                ] {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut reader = BufReader::new(stream);
                    let mut frame = String::new();
                    reader.read_line(&mut frame).await.unwrap();
                    let request: Value = serde_json::from_str(&frame).unwrap();
                    reader
                        .get_mut()
                        .write_all(
                            format!(
                                "{}\n",
                                json!({"id":request["id"],"status":200,"body":{"ok":result}})
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                }
            });
            assert_eq!(
                readiness(&connection).await,
                if code == 200 {
                    RuntimeStatus::Ready
                } else {
                    RuntimeStatus::Unavailable
                }
            );
            server.await.unwrap();
        }
    }
}
