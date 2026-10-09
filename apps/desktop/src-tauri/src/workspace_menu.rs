use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, Submenu},
};

use crate::tray_menu::NativeMenuLabels;

pub(crate) struct WorkspaceMenu {
    file: Submenu<tauri::Wry>,
    new: MenuItem<tauri::Wry>,
    open: MenuItem<tauri::Wry>,
    save: MenuItem<tauri::Wry>,
    save_as: MenuItem<tauri::Wry>,
    #[cfg(target_os = "macos")]
    application_quit: MenuItem<tauri::Wry>,
    #[cfg(target_os = "linux")]
    quit: MenuItem<tauri::Wry>,
}

impl WorkspaceMenu {
    pub(crate) fn set_labels(&self, labels: &NativeMenuLabels) -> tauri::Result<()> {
        self.file.set_text(labels.workspace_file)?;
        self.new.set_text(labels.workspace_new)?;
        self.open.set_text(labels.workspace_open)?;
        self.save.set_text(labels.workspace_save)?;
        self.save_as.set_text(labels.workspace_save_as)?;
        #[cfg(target_os = "macos")]
        self.application_quit.set_text(labels.app_quit)?;
        #[cfg(target_os = "linux")]
        self.quit.set_text(labels.tray_quit)?;
        Ok(())
    }
}

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    #[cfg(target_os = "macos")]
    let application_quit = install_app_menu(app, &menu)?;
    let file = menu
        .items()?
        .into_iter()
        .filter_map(|item| item.as_submenu().cloned())
        .find(|submenu| submenu.text().is_ok_and(|text| text == "File"));
    let file = if let Some(file) = file {
        file
    } else {
        let file = Submenu::new(app, "File", true)?;
        menu.prepend(&file)?;
        file
    };
    let labels = crate::tray_menu::labels("en");
    let new = MenuItem::with_id(
        app,
        "workspace:new",
        labels.workspace_new,
        true,
        Some("CmdOrCtrl+N"),
    )?;
    let open = MenuItem::with_id(
        app,
        "workspace:open",
        labels.workspace_open,
        true,
        Some("CmdOrCtrl+O"),
    )?;
    let save = MenuItem::with_id(
        app,
        "workspace:save",
        labels.workspace_save,
        true,
        Some("CmdOrCtrl+S"),
    )?;
    let save_as = MenuItem::with_id(
        app,
        "workspace:save_as",
        labels.workspace_save_as,
        true,
        Some("CmdOrCtrl+Shift+S"),
    )?;
    file.insert(&new, 0)?;
    file.insert(&open, 1)?;
    file.insert(&save, 2)?;
    file.insert(&save_as, 3)?;
    #[cfg(target_os = "linux")]
    let quit = {
        let quit = MenuItem::with_id(app, "app:quit", labels.tray_quit, true, Some("Ctrl+Q"))?;
        file.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
        file.append(&quit)?;
        quit
    };
    app.set_menu(menu)?;
    app.manage(WorkspaceMenu {
        file,
        new,
        open,
        save,
        save_as,
        #[cfg(target_os = "macos")]
        application_quit,
        #[cfg(target_os = "linux")]
        quit,
    });
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "app:quit" {
            app.exit(0);
            return;
        }
        if let Some(action) = event.id().as_ref().strip_prefix("workspace:") {
            let _ = app.emit("workspace-file", action);
        }
    });
    Ok(())
}

/// Native predefined Quit calls `NSApp` terminate directly, bypassing `ExitRequested`.
/// Route the menu and shortcut through the same preventable exit event as the tray.
#[cfg(target_os = "macos")]
fn install_app_menu(
    app: &tauri::App,
    menu: &Menu<tauri::Wry>,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, PredefinedMenuItem};
    let quit = MenuItem::with_id(
        app,
        "app:quit",
        crate::tray_menu::labels("en").app_quit,
        true,
        Some("Cmd+Q"),
    )?;
    let application = Submenu::with_items(
        app,
        "MixRelay",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                None,
                Some(AboutMetadata {
                    name: Some("MixRelay".into()),
                    version: Some(app.package_info().version.to_string()),
                    ..Default::default()
                }),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    // Menu::default on macOS starts with its application submenu.
    menu.remove_at(0)?;
    menu.prepend(&application)?;
    Ok(quit)
}
