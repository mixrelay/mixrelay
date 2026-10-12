use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, Submenu},
};

use crate::app_menu::NativeMenuLabels;

pub(crate) struct WorkspaceMenu {
    about: MenuItem<tauri::Wry>,
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
    pub(crate) fn set_ready(&self, ready: bool) -> tauri::Result<()> {
        for item in [&self.new, &self.open, &self.save, &self.save_as] {
            item.set_enabled(ready)?;
        }
        Ok(())
    }

    pub(crate) fn set_labels(&self, labels: &NativeMenuLabels) -> tauri::Result<()> {
        self.about.set_text(labels.app_about)?;
        self.file.set_text(labels.workspace_file)?;
        self.new.set_text(labels.workspace_new)?;
        self.open.set_text(labels.workspace_open)?;
        self.save.set_text(labels.workspace_save)?;
        self.save_as.set_text(labels.workspace_save_as)?;
        #[cfg(target_os = "macos")]
        self.application_quit.set_text(labels.app_quit)?;
        #[cfg(target_os = "linux")]
        self.quit.set_text(labels.quit)?;
        Ok(())
    }
}

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    let labels = crate::app_menu::labels("en");
    let about = MenuItem::with_id(app, "app:about", labels.app_about, true, None::<&str>)?;
    #[cfg(target_os = "macos")]
    let application_quit = install_app_menu(app, &menu, &about)?;
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
    let [new, open, save, save_as] = workspace_file_items(app, &labels)?;
    file.insert(&new, 0)?;
    file.insert(&open, 1)?;
    file.insert(&save, 2)?;
    file.insert(&save_as, 3)?;
    #[cfg(target_os = "linux")]
    let quit = {
        let quit = MenuItem::with_id(app, "app:quit", labels.quit, true, Some("Ctrl+Q"))?;
        file.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
        file.append(&about)?;
        file.append(&quit)?;
        quit
    };
    app.set_menu(menu)?;
    app.manage(WorkspaceMenu {
        about,
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
    app.state::<WorkspaceMenu>().set_ready(false)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "app:about" {
            let _ = crate::app_menu::show_main_window(app);
            let _ = app.emit("app-about", ());
            return;
        }
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

fn workspace_file_items(
    app: &tauri::App,
    labels: &NativeMenuLabels,
) -> tauri::Result<[MenuItem<tauri::Wry>; 4]> {
    let item = |id, label, shortcut| MenuItem::with_id(app, id, label, true, Some(shortcut));
    Ok([
        item("workspace:new", labels.workspace_new, "CmdOrCtrl+N")?,
        item("workspace:open", labels.workspace_open, "CmdOrCtrl+O")?,
        item("workspace:save", labels.workspace_save, "CmdOrCtrl+S")?,
        item(
            "workspace:save_as",
            labels.workspace_save_as,
            "CmdOrCtrl+Shift+S",
        )?,
    ])
}

/// Native predefined Quit calls `NSApp` terminate directly, bypassing `ExitRequested`.
/// Route the menu and shortcut through the preventable exit event so the engine can finalize recordings.
#[cfg(target_os = "macos")]
fn install_app_menu(
    app: &tauri::App,
    menu: &Menu<tauri::Wry>,
    about: &MenuItem<tauri::Wry>,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    use tauri::menu::PredefinedMenuItem;
    let quit = MenuItem::with_id(
        app,
        "app:quit",
        crate::app_menu::labels("en").app_quit,
        true,
        Some("Cmd+Q"),
    )?;
    let application = Submenu::with_items(
        app,
        "MixRelay",
        true,
        &[
            about,
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
