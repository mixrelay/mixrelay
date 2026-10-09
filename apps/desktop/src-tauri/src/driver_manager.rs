use std::{collections::HashMap, sync::Mutex, time::Duration};

use mixrelay_driver_control::{DriverClient, DriverSnapshot};
use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

mod choices;
mod menu;

use choices::SettingAction;
use menu::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
enum UpdateResult {
    Current,
    NotPublished,
    CheckFailed,
    Download { version: String, url: String },
    DownloadFailed,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum QuitState {
    #[default]
    Idle,
    Stopping,
    Stopped,
}

#[derive(Default)]
struct MenuState {
    locale: String,
    snapshot: Option<DriverSnapshot>,
    busy: bool,
    quit_state: QuitState,
    startup_cancelled: bool,
    error: Option<String>,
    update_result: Option<UpdateResult>,
    updates_busy: bool,
    generation: u64,
    actions: HashMap<String, SettingAction>,
}

/// Native free Driver controls, independent of graph and loopback process ownership.
pub(crate) struct DriverMenu {
    app: tauri::AppHandle,
    state: Mutex<MenuState>,
    lifecycle: tokio::sync::Mutex<()>,
    status: MenuItem<tauri::Wry>,
    tray: Menu<tauri::Wry>,
    application: Submenu<tauri::Wry>,
    updates: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

impl DriverMenu {
    /// Relabels native controls and their current values for the selected UI locale.
    pub(crate) fn set_labels(&self, locale: &str) -> tauri::Result<()> {
        if let Ok(mut state) = self.state.lock() {
            locale.clone_into(&mut state.locale);
        }
        self.refresh_menu()
    }

    /// Publishes shared Driver mutation readback to native menus.
    pub(crate) fn observe_snapshot(&self, snapshot: &DriverSnapshot) {
        match self.state.lock() {
            Ok(mut state) => {
                state.snapshot = Some(snapshot.clone());
                state.error = None;
            }
            Err(error) => tracing::error!(%error, "Driver mutation readback could not be observed"),
        }
        let _ = self.refresh_menu();
    }

    fn refresh_menu(&self) -> tauri::Result<()> {
        let app = self.app.clone();
        let result = self.app.run_on_main_thread(move || {
            let menu = app.state::<DriverMenu>();
            if let Err(error) = menu.render() {
                tracing::error!(%error, "Driver menu could not be refreshed");
            }
        });
        if let Err(error) = &result {
            tracing::error!(%error, "Driver menu refresh could not be scheduled");
        }
        result
    }

    fn render(&self) -> tauri::Result<()> {
        let Ok(mut state) = self.state.lock() else {
            tracing::error!("Driver menu state lock is unavailable");
            return Ok(());
        };
        let text = labels(&state.locale);
        self.updates
            .set_text(menu::update_label(state.updates_busy, &text))?;
        self.updates.set_enabled(!state.updates_busy)?;
        self.quit.set_text(text.quit)?;
        self.quit
            .set_enabled(state.quit_state != QuitState::Stopping)?;
        self.status.set_text(menu::status_text(&state))?;
        state.generation = state.generation.wrapping_add(1);
        state.actions.clear();
        for item in self.tray.items()? {
            self.tray.remove(&item)?;
        }
        for item in self.application.items()? {
            self.application.remove(&item)?;
        }
        self.append(&self.status)?;
        let devices = menu::device_menus(&self.app, &mut state, &text)?;
        if !devices.is_empty() {
            self.append(&PredefinedMenuItem::separator(&self.app)?)?;
        }
        for device in devices {
            self.append(&device)?;
        }
        self.append(&PredefinedMenuItem::separator(&self.app)?)?;
        self.append(&self.updates)?;
        if let Some(result) = &state.update_result {
            let item = MenuItem::with_id(
                &self.app,
                "driver:download",
                menu::update_result_label(result, &text),
                matches!(result, UpdateResult::Download { .. }) && !state.updates_busy,
                None::<&str>,
            )?;
            self.append(&item)?;
        }
        self.append(&PredefinedMenuItem::separator(&self.app)?)?;
        self.append(&self.quit)
    }

    fn append(&self, item: &dyn tauri::menu::IsMenuItem<tauri::Wry>) -> tauri::Result<()> {
        self.tray.append(item)?;
        self.application.append(item)
    }
}

/// Installs native Driver settings and restores devices stopped by an explicit manager quit.
pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    if !tauri::is_dev() {
        let lease = mixrelay_macos_transport::app_lifecycle::ProcessLease::acquire(
            "/Library/Application Support/MixRelay/lifecycle/driver-manager.owner",
        )?;
        app.manage(lease);
    }
    let (menu, tray) = create_menu(app)?;
    app.manage(menu);
    app.on_menu_event(|app, event| handle_menu_event(app, &event));
    install_tray(app, &tray)?;
    activate_driver(app.handle());
    start_update_checks(app.handle());
    Ok(())
}

fn activate_driver(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let menu = app.state::<DriverMenu>();
        let _lifecycle = menu.lifecycle.lock().await;
        let client = match DriverClient::new() {
            Ok(client) => client,
            Err(error) => {
                finish_snapshot(&app, Err(error.to_string()));
                return;
            }
        };
        // Login/installation can start the manager before the native owner is ready.
        // The bounded retry is cancelled before a requested stop.
        for attempt in 0..20 {
            if menu
                .state
                .lock()
                .map_or(true, |state| state.startup_cancelled)
            {
                return;
            }
            let result = client.set_active(true).await;
            let waiting = matches!(&result,
                Err(mixrelay_driver_control::DriverError::Http(error)) if error.is_connect()
            ) || matches!(
                &result,
                Err(mixrelay_driver_control::DriverError::Remote { status: 503, .. })
            );
            if waiting && attempt < 19 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            finish_snapshot(&app, result.map_err(|error| error.to_string()));
            return;
        }
    });
}

/// Every explicit exit waits for acknowledged native stop and saved restart state.
/// Failed stops leave the manager available for another attempt instead of silently exiting.
pub(crate) fn exit_requested(app: &tauri::AppHandle, api: &tauri::ExitRequestApi) {
    let menu = app.state::<DriverMenu>();
    let Ok(mut state) = menu.state.lock() else {
        api.prevent_exit();
        return;
    };
    if state.quit_state == QuitState::Stopped {
        return;
    }
    api.prevent_exit();
    if state.quit_state == QuitState::Stopping {
        return;
    }
    state.quit_state = QuitState::Stopping;
    // A queued startup must not revive devices after either a successful stop
    // or a failed stop/save followed by dismissal of the retry dialog.
    state.startup_cancelled = true;
    state.busy = true;
    let locale = state.locale.clone();
    drop(state);
    let _ = menu.refresh_menu();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let menu = app.state::<DriverMenu>();
        let _lifecycle = menu.lifecycle.lock().await;
        let result = stop_driver(&app).await;
        let menu = app.state::<DriverMenu>();
        if let Ok(mut state) = menu.state.lock() {
            state.quit_state = if result.is_ok() {
                QuitState::Stopped
            } else {
                QuitState::Idle
            };
            state.busy = false;
        }
        match result {
            Ok(_) => app.exit(0),
            Err(error) => {
                tracing::error!(%error, "Driver did not complete explicit quit");
                let _ = menu.refresh_menu();
                let (title, body) = crate::native_text::driver_quit_failed(&locale);
                rfd::AsyncMessageDialog::new()
                    .set_title(title)
                    .set_description(body)
                    .set_buttons(rfd::MessageButtons::OkCustom(
                        crate::native_text::dialogs::text(
                            crate::native_text::resolve_locale(&locale),
                            crate::native_text::dialogs::Text::Ok,
                        )
                        .into(),
                    ))
                    .set_level(rfd::MessageLevel::Error)
                    .show()
                    .await;
            }
        }
    });
}

async fn stop_driver(app: &tauri::AppHandle) -> Result<DriverSnapshot, String> {
    let result = DriverClient::new()
        .map_err(|error| error.to_string())?
        .set_active(false)
        .await;
    if matches!(&result, Err(mixrelay_driver_control::DriverError::Http(error)) if error.is_connect())
        && let Ok(snapshot) = crate::driver_control::driver_status(app.clone()).await
        && snapshot.state == mixrelay_driver_control::DriverState::NotInstalled
        && snapshot.settings.devices.is_empty()
    {
        // An uninstalled Driver has nothing to stop. A merely unloaded or
        // unreachable installed backend is not proof that native audio stopped.
        return Ok(snapshot);
    }
    result.map_err(|error| error.to_string())
}

fn create_menu(app: &tauri::App) -> tauri::Result<(DriverMenu, Menu<tauri::Wry>)> {
    let text = labels("en");
    let status = MenuItem::with_id(app, "driver:status", "MixRelay Driver", false, None::<&str>)?;
    let updates = MenuItem::with_id(
        app,
        "driver:updates",
        text.check_updates,
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "driver:quit", text.quit, true, Some("CmdOrCtrl+Q"))?;
    let tray = Menu::with_items(app, &[&status, &updates, &quit])?;
    let application =
        Submenu::with_items(app, "MixRelay Driver", true, &[&status, &updates, &quit])?;
    app.set_menu(Menu::with_items(app, &[&application])?)?;
    let menu = DriverMenu {
        app: app.handle().clone(),
        state: Mutex::new(MenuState::default()),
        lifecycle: tokio::sync::Mutex::new(()),
        status,
        tray: tray.clone(),
        application,
        updates,
        quit,
    };
    Ok((menu, tray))
}

fn install_tray(app: &tauri::App, tray: &Menu<tauri::Wry>) -> tauri::Result<()> {
    TrayIconBuilder::with_id("driver")
        .icon(tauri::include_image!("tray-icons/64x64.png"))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("MixRelay Driver")
        .menu(tray)
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button_state: MouseButtonState::Down,
                    ..
                }
            ) {
                refresh_snapshot(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn start_update_checks(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_hours(24));
        loop {
            interval.tick().await;
            check_updates(&handle, false);
        }
    });
}

fn handle_menu_event(app: &tauri::AppHandle, event: &tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "driver:quit" => app.exit(0),
        "driver:updates" => check_updates(app, true),
        "driver:download" => open_download(app),
        id => {
            let action = app
                .state::<DriverMenu>()
                .state
                .lock()
                .ok()
                .and_then(|state| state.actions.get(id).cloned());
            if let Some(action) = action {
                mutate_setting(app, action);
            }
        }
    }
}

fn begin_snapshot(app: &tauri::AppHandle) -> bool {
    let menu = app.state::<DriverMenu>();
    let Ok(mut state) = menu.state.lock() else {
        return false;
    };
    if state.busy || state.quit_state != QuitState::Idle {
        return false;
    }
    state.busy = true;
    state.error = None;
    drop(state);
    let _ = menu.refresh_menu();
    true
}

fn refresh_snapshot(app: &tauri::AppHandle) {
    if !begin_snapshot(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        finish_snapshot(
            &app,
            crate::driver_control::driver_status(app.clone()).await,
        );
    });
}

fn finish_snapshot(app: &tauri::AppHandle, result: Result<DriverSnapshot, String>) {
    let menu = app.state::<DriverMenu>();
    if let Ok(mut state) = menu.state.lock() {
        state.busy = false;
        match result {
            Ok(snapshot) => {
                state.snapshot = Some(snapshot);
                state.error = None;
            }
            Err(error) => {
                tracing::error!(%error, "Driver menu action failed");
                state.error = Some(error);
            }
        }
    }
    let _ = menu.refresh_menu();
}

fn mutate_setting(app: &tauri::AppHandle, action: SettingAction) {
    if !begin_snapshot(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = apply_setting(&app, action).await;
        finish_snapshot(&app, result);
    });
}

async fn apply_setting(
    app: &tauri::AppHandle,
    action: SettingAction,
) -> Result<DriverSnapshot, String> {
    let snapshot = crate::driver_control::driver_status(app.clone()).await?;
    let command = match choices::command(&snapshot, &action) {
        Ok(command) => command,
        Err(error) => {
            if let Ok(mut state) = app.state::<DriverMenu>().state.lock() {
                state.snapshot = Some(snapshot);
            }
            return Err(error);
        }
    };
    let result = crate::driver_control::driver_mutate(app.clone(), command).await;
    if result.is_err()
        && let Ok(snapshot) = crate::driver_control::driver_status(app.clone()).await
        && let Ok(mut state) = app.state::<DriverMenu>().state.lock()
    {
        state.snapshot = Some(snapshot);
    }
    result
}

fn check_updates(app: &tauri::AppHandle, manual: bool) {
    let menu = app.state::<DriverMenu>();
    let Ok(mut state) = menu.state.lock() else {
        return;
    };
    if state.updates_busy {
        return;
    }
    state.updates_busy = true;
    drop(state);
    let _ = menu.refresh_menu();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let update = crate::updates::updates_check_driver().await;
        let menu = app.state::<DriverMenu>();
        if let Ok(mut state) = menu.state.lock() {
            state.update_result = menu::update_result(
                update.state(),
                update.latest_version().zip(
                    update
                        .downloads()
                        .first()
                        .map(crate::updates::Download::url),
                ),
                manual,
            );
            state.updates_busy = false;
        }
        let _ = menu.refresh_menu();
    });
}

fn open_download(app: &tauri::AppHandle) {
    let menu = app.state::<DriverMenu>();
    let Ok(mut state) = menu.state.lock() else {
        return;
    };
    if state.updates_busy {
        return;
    }
    let Some(UpdateResult::Download { url, .. }) = &state.update_result else {
        return;
    };
    let url = url.clone();
    state.updates_busy = true;
    drop(state);
    let _ = menu.refresh_menu();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result =
            crate::updates::updates_open_download(crate::updates::Product::MixRelayDriver, url)
                .await;
        let menu = app.state::<DriverMenu>();
        if let Err(error) = &result {
            tracing::error!(%error, "Driver update download could not be opened");
        }
        if let Ok(mut state) = menu.state.lock() {
            state.updates_busy = false;
            if result.is_err() {
                state.update_result = Some(UpdateResult::DownloadFailed);
            }
        }
        let _ = menu.refresh_menu();
    });
}
