use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use mixrelay_macos_transport::app_lifecycle::{
    APP_LEASE, ENGINE_LEASE, EngineWake, ProcessLease, engine_exit_record, is_owned,
};
use tauri::Manager;

/// Desktop ownership, independent of window visibility and graph power state.
pub(crate) struct EngineLifecycle {
    owner: Mutex<Option<DesktopOwner>>,
    quitting: AtomicBool,
    finished: AtomicBool,
}

struct DesktopOwner {
    wake: Option<EngineWake>,
    lease: Option<ProcessLease>,
}

impl Drop for DesktopOwner {
    fn drop(&mut self) {
        self.wake.take();
        self.lease.take();
    }
}

impl EngineLifecycle {
    pub(crate) fn start() -> Result<Self, String> {
        validate_installation()?;
        let lease = ProcessLease::acquire(APP_LEASE)
            .map_err(|error| format!("could not acquire desktop ownership: {error}"))?;
        if is_owned(ENGINE_LEASE).map_err(|error| error.to_string())? {
            return Err(
                "The audio server is still shutting down. Try opening MixRelay again shortly."
                    .into(),
            );
        }
        let wake = EngineWake::start().map_err(|error| error.to_string())?;
        Ok(Self {
            owner: Mutex::new(Some(DesktopOwner {
                wake: Some(wake),
                lease: Some(lease),
            })),
            quitting: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        })
    }

    pub(crate) fn exit_requested(&self, app: &tauri::AppHandle, api: &tauri::ExitRequestApi) {
        if self.finished.load(Ordering::Acquire) {
            return;
        }
        api.prevent_exit();
        if self.quitting.swap(true, Ordering::AcqRel) {
            return;
        }
        app.state::<crate::control_proxy::ControlProxyClient>()
            .close_admission();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match finish_local_controls(&app).await {
                Ok(warning) => {
                    if let Some(warning) = warning {
                        rfd::AsyncMessageDialog::new()
                            .set_title("MixRelay")
                            .set_description(&warning)
                            .set_level(rfd::MessageLevel::Error)
                            .show()
                            .await;
                    }
                    app.state::<Self>().finished.store(true, Ordering::Release);
                    app.exit(0);
                }
                Err(error) => {
                    app.state::<Self>().quitting.store(false, Ordering::Release);
                    tracing::error!(%error, "audio server exit is not confirmed");
                    let _ = crate::tray_menu::show_main_window(&app);
                    rfd::AsyncMessageDialog::new()
                        .set_title("MixRelay")
                        .set_description(&error)
                        .set_level(rfd::MessageLevel::Error)
                        .show()
                        .await;
                }
            }
        });
    }
}

async fn finish_local_controls(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    app.state::<crate::control_proxy::local::socket::LocalSocket>()
        .shutdown()
        .await?;
    app.state::<crate::control_proxy::ControlProxyClient>()
        .drain()
        .await?;
    app.state::<EngineLifecycle>()
        .owner
        .lock()
        .map_err(|_| "desktop lifecycle ownership lock failed".to_owned())?
        .take();
    wait_for_engine_exit(app).await
}

fn validate_installation() -> Result<(), String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args([
            "-convert",
            "json",
            "-o",
            "-",
            "/Library/LaunchDaemons/io.mixrelay.audio.engine.plist",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    let config: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "MixRelay audio service installation is missing or invalid.".to_owned())?;
    if !is_app_owned_configuration(&config) {
        return Err("MixRelay audio service needs the app-owned lifecycle installation before opening. Run scripts/install-macos-dev-audio.sh once with administrator approval.".into());
    }
    Ok(())
}

fn is_app_owned_configuration(config: &serde_json::Value) -> bool {
    config["Label"] == "io.mixrelay.audio.engine"
        && config["KeepAlive"] == false
        && config["RunAtLoad"] == false
        && config["MachServices"]["io.mixrelay.audio.engine"] == true
}

enum ExitObservation {
    Pending,
    Stopped(String),
}

async fn observe_engine_exit() -> Result<ExitObservation, String> {
    let owned = is_owned(ENGINE_LEASE).map_err(|error| error.to_string())?;
    let connected = tokio::time::timeout(
        Duration::from_millis(250),
        tokio::net::TcpStream::connect("127.0.0.1:19341"),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    if owned || connected {
        return Ok(ExitObservation::Pending);
    }
    engine_exit_record()
        .map(ExitObservation::Stopped)
        .map_err(|error| error.to_string())
}

async fn wait_for_engine_exit(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    await_engine_exit(observe_engine_exit, || {
        tracing::warn!("waiting for native audio server teardown; exit remains pending");
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_title("MixRelay — 오디오 서버 종료 중");
        }
    })
    .await
}

async fn await_engine_exit(
    mut observe: impl AsyncFnMut() -> Result<ExitObservation, String>,
    mut waiting: impl FnMut(),
) -> Result<Option<String>, String> {
    let notice_at = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut notified = false;
    loop {
        if let ExitObservation::Stopped(record) = observe().await? {
            return Ok(exit_warning(&record));
        }
        if !notified && tokio::time::Instant::now() >= notice_at {
            notified = true;
            waiting();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn exit_warning(record: &str) -> Option<String> {
    if record == "stopped" {
        return None;
    }
    Some(format!(
        "오디오 서버는 종료되었지만 녹음·설정 마무리 성공을 확인하지 못했습니다.\n{}",
        record.strip_prefix("failed\n").unwrap_or(record)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn slow_native_exit_completes_without_a_second_quit_request() {
        let start = tokio::time::Instant::now();
        let mut notices = 0;
        let warning = await_engine_exit(
            async || {
                Ok(if start.elapsed() < Duration::from_secs(65) {
                    ExitObservation::Pending
                } else {
                    ExitObservation::Stopped("stopped".into())
                })
            },
            || notices += 1,
        )
        .await
        .unwrap();
        assert!(warning.is_none());
        assert_eq!(notices, 1);
        assert!(start.elapsed() >= Duration::from_secs(65));
    }

    #[tokio::test(start_paused = true)]
    async fn exit_probe_failure_is_not_reported_as_a_stopped_engine() {
        let result = await_engine_exit(
            async || Err("lease inaccessible".into()),
            || panic!("immediate error needs no delayed notice"),
        )
        .await;
        assert_eq!(result, Err("lease inaccessible".into()));
    }

    #[test]
    fn unfinished_or_failed_record_is_not_reported_as_clean_shutdown() {
        assert!(exit_warning("stopped").is_none());
        assert!(exit_warning("running").is_some());
        assert!(exit_warning("").is_some());
        assert!(
            exit_warning("failed\ndisk full")
                .expect("warning")
                .contains("disk full")
        );
    }

    #[test]
    fn app_owned_service_preserves_native_registration_without_autorestart() {
        let mut config = serde_json::json!({
            "Label": "io.mixrelay.audio.engine", "KeepAlive": false, "RunAtLoad": false,
            "MachServices": { "io.mixrelay.audio.engine": true },
        });
        assert!(is_app_owned_configuration(&config));
        config["KeepAlive"] = true.into();
        assert!(!is_app_owned_configuration(&config));
        config["KeepAlive"] = false.into();
        config["RunAtLoad"] = true.into();
        assert!(!is_app_owned_configuration(&config));
        config["RunAtLoad"] = false.into();
        config["MachServices"] = serde_json::Value::Null;
        assert!(!is_app_owned_configuration(&config));
    }
}
