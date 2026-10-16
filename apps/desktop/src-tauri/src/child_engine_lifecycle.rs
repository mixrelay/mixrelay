use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::native_text::{
    current_locale,
    dialogs::{Text, detail, text},
};
use mixrelay_engine_process::ControllerEngine;
use tauri::Manager;

/// Owns only this controller's bundled engine, not an existing localhost listener.
pub(crate) struct EngineLifecycle {
    engine: Mutex<ControllerEngine>,
    quitting: AtomicBool,
    finished: AtomicBool,
}

impl EngineLifecycle {
    pub(crate) fn start() -> Result<Self, String> {
        tauri::async_runtime::block_on(crate::linux_service::start_driver())
            .map_err(|error| error.to_string())?;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let path = bundled_engine_path(&executable)?;
        let engine = ControllerEngine::start(&path)
            .map_err(|error| detail(current_locale(), Text::BundledEngineStartFailed, error))?;
        Ok(Self {
            engine: Mutex::new(engine),
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
            match finish_engine(&app).await {
                Ok(warning) => {
                    if let Some(warning) = warning {
                        rfd::AsyncMessageDialog::new()
                            .set_title("MixRelay")
                            .set_description(&warning)
                            .set_level(rfd::MessageLevel::Error)
                            .set_buttons(rfd::MessageButtons::OkCustom(
                                text(current_locale(), Text::Ok).into(),
                            ))
                            .show()
                            .await;
                    }
                    app.state::<Self>().finished.store(true, Ordering::Release);
                    app.exit(0);
                }
                Err(error) => {
                    app.state::<Self>().quitting.store(false, Ordering::Release);
                    tracing::error!(%error, "owned audio engine shutdown failed");
                    crate::status_menu::shutdown_failed(&app).await;
                }
            }
        });
    }
}

fn bundled_engine_path(desktop: &Path) -> Result<PathBuf, String> {
    let directory = desktop
        .parent()
        .ok_or_else(|| text(current_locale(), Text::DesktopExecutableParentMissing))?;
    let engine = directory.join("mixrelay-engine-service");
    if !engine.is_file() {
        return Err(detail(
            current_locale(),
            Text::BundledEngineMissing,
            engine.display(),
        ));
    }
    Ok(engine)
}

/// Readiness is a point-in-time acknowledgement, not a promise of future life.
/// Stop accepting commands if our exact child dies. An explicit retry starts a new
/// owned child; it never attaches to another process on the same control port.
pub(crate) fn monitor(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let lifecycle = app.state::<EngineLifecycle>();
            if lifecycle.finished.load(Ordering::Acquire) {
                return;
            }
            if lifecycle.quitting.load(Ordering::Acquire) {
                continue;
            }
            let result = lifecycle
                .engine
                .lock()
                .map_err(|_| text(current_locale(), Text::EngineOwnershipLockFailed).to_owned())
                .and_then(|mut engine| engine.try_wait().map_err(|error| error.to_string()));
            let error = match result {
                Ok(None) => continue,
                Ok(Some(status)) => {
                    detail(current_locale(), Text::EngineExitedUnexpectedly, status)
                }
                Err(error) => detail(current_locale(), Text::EngineObserveFailed, error),
            };
            if lifecycle.quitting.load(Ordering::Acquire) {
                continue;
            }
            app.state::<crate::control_proxy::ControlProxyClient>()
                .suspend_admission();
            tracing::error!(%error, "owned audio engine unavailable");
            if !offer_engine_recovery(&app).await {
                return;
            }
        }
    });
}

async fn offer_engine_recovery(app: &tauri::AppHandle) -> bool {
    let mut message = Text::EngineRestartPrompt;
    loop {
        let locale = current_locale();
        let restart = text(locale, Text::EngineRestart);
        let choice = rfd::AsyncMessageDialog::new()
            .set_title("MixRelay")
            .set_description(text(locale, message))
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                restart.into(),
                text(locale, Text::Cancel).into(),
            ))
            .show()
            .await;
        if choice != rfd::MessageDialogResult::Custom(restart.into()) {
            return false;
        }
        match restart_owned_engine(app).await {
            Ok(restarted) => return restarted,
            Err(error) => {
                tracing::error!(%error, "owned audio engine restart failed");
                message = Text::EngineRestartFailedPrompt;
            }
        }
    }
}

async fn restart_owned_engine(app: &tauri::AppHandle) -> Result<bool, String> {
    let lifecycle = app.state::<EngineLifecycle>();
    let client = app.state::<crate::control_proxy::ControlProxyClient>();
    client.drain().await?;
    if lifecycle.quitting.load(Ordering::Acquire) {
        return Ok(false);
    }
    {
        let mut engine = lifecycle
            .engine
            .lock()
            .map_err(|_| text(current_locale(), Text::EngineOwnershipLockFailed).to_owned())?;
        if engine
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err(text(current_locale(), Text::PreviousEngineRunning).to_owned());
        }
    }
    crate::linux_service::start_driver()
        .await
        .map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = bundled_engine_path(&executable)?;
    let replacement = tauri::async_runtime::spawn_blocking(move || ControllerEngine::start(&path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    {
        let mut engine = lifecycle
            .engine
            .lock()
            .map_err(|_| text(current_locale(), Text::EngineOwnershipLockFailed).to_owned())?;
        if lifecycle.quitting.load(Ordering::Acquire) {
            // Dropping the uninstalled child closes only its private shutdown lease.
            return Ok(false);
        }
        *engine = replacement;
    }
    Ok(client.resume_admission().await)
}

async fn finish_engine(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    app.state::<crate::control_proxy::local::socket::LocalSocket>()
        .shutdown()
        .await?;
    app.state::<crate::control_proxy::ControlProxyClient>()
        .drain()
        .await?;
    {
        let lifecycle = app.state::<EngineLifecycle>();
        lifecycle
            .engine
            .lock()
            .map_err(|_| text(current_locale(), Text::EngineOwnershipLockFailed))?
            .request_shutdown();
    }
    let notice = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut notified = false;
    loop {
        let status = app
            .state::<EngineLifecycle>()
            .engine
            .lock()
            .map_err(|_| text(current_locale(), Text::EngineOwnershipLockFailed))?
            .try_wait()
            .map_err(|error| error.to_string())?;
        if let Some(status) = status {
            return Ok((!status.success())
                .then(|| detail(current_locale(), Text::EngineFinalizationFailed, status)));
        }
        if !notified && tokio::time::Instant::now() >= notice {
            notified = true;
            tracing::warn!("waiting for owned audio engine finalization");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(text(current_locale(), Text::EngineStoppingTitle));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_the_adjacent_bundled_engine() {
        let directory = tempfile::tempdir().unwrap();
        let desktop = directory.path().join("MixRelay");
        assert!(bundled_engine_path(&desktop).is_err());
        let expected = directory.path().join("mixrelay-engine-service");
        std::fs::write(&expected, b"fixture").unwrap();
        assert_eq!(bundled_engine_path(&desktop).unwrap(), expected);
    }
}
