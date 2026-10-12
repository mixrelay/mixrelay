//! The background Driver runtime owns the single `MixRelay Driver` status menu.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Manager as _, menu::MenuItem};

pub(crate) struct ServiceMenu {
    stop: MenuItem<tauri::Wry>,
    stopping: AtomicBool,
}

const OPEN_LABELS: [&str; 11] = [
    "Open MixRelay",
    "MixRelay 열기",
    "MixRelayを開く",
    "打开 MixRelay",
    "開啟 MixRelay",
    "Ouvrir MixRelay",
    "MixRelay öffnen",
    "Abrir MixRelay",
    "Abrir MixRelay",
    "Apri MixRelay",
    "Открыть MixRelay",
];

pub(crate) fn open_label(locale: &str) -> &'static str {
    OPEN_LABELS[crate::native_text::resolve_locale(locale).index()]
}

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let stop = crate::driver_manager::install_runtime(app)?;
    app.manage(ServiceMenu {
        stop,
        stopping: AtomicBool::new(false),
    });
    Ok(())
}

pub(crate) fn open_editor(app: &tauri::AppHandle) {
    let target = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::background::startup::editor_executable()
            .and_then(|exe| crate::background::spawn_editor(&exe));
        if let Err(error) = result {
            tracing::error!(%error, "could not open MixRelay editor");
        }
        drop(target);
    });
}

pub(crate) fn stop_all(app: &tauri::AppHandle) {
    let menu = app.state::<ServiceMenu>();
    if menu.stopping.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = menu.stop.set_enabled(false);
    app.state::<crate::driver_manager::DriverMenu>()
        .set_runtime_stopping(true);
    #[cfg(target_os = "macos")]
    {
        // Native/system termination and the menu use the same serialized stop.
        // EngineLifecycle cancels queued activation and waits for the HAL ACK.
        app.exit(0);
    }
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match crate::driver_manager::stop_driver(&app).await {
                Ok(_) => app.exit(0),
                Err(error) => {
                    let menu = app.state::<ServiceMenu>();
                    menu.stopping.store(false, Ordering::Release);
                    let _ = menu.stop.set_enabled(true);
                    app.state::<crate::driver_manager::DriverMenu>()
                        .set_runtime_stopping(false);
                    tracing::error!(%error, "MixRelay could not stop all audio");
                    rfd::AsyncMessageDialog::new()
                        .set_title("MixRelay Driver")
                        .set_description(&error)
                        .set_level(rfd::MessageLevel::Error)
                        .show()
                        .await;
                }
            }
        });
    }
}

pub(crate) fn shutdown_failed(app: &tauri::AppHandle) {
    if let Some(menu) = app.try_state::<ServiceMenu>() {
        menu.stopping.store(false, Ordering::Release);
        let _ = menu.stop.set_enabled(true);
        app.state::<crate::driver_manager::DriverMenu>()
            .set_runtime_stopping(false);
    }
}
