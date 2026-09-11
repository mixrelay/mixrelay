use tauri::{
    Emitter,
    menu::{Menu, MenuItem, Submenu},
};

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    #[cfg(target_os = "macos")]
    install_app_menu(app, &menu)?;
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
    for (index, (action, label, shortcut)) in [
        ("new", "New Workspace", "CmdOrCtrl+N"),
        ("open", "Open…", "CmdOrCtrl+O"),
        ("save", "Save", "CmdOrCtrl+S"),
        ("save_as", "Save As…", "CmdOrCtrl+Shift+S"),
    ]
    .into_iter()
    .enumerate()
    {
        file.insert(
            &MenuItem::with_id(
                app,
                format!("workspace:{action}"),
                label,
                true,
                Some(shortcut),
            )?,
            index,
        )?;
    }
    app.set_menu(menu)?;
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
fn install_app_menu(app: &tauri::App, menu: &Menu<tauri::Wry>) -> tauri::Result<()> {
    use tauri::menu::{AboutMetadata, PredefinedMenuItem};
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
            &MenuItem::with_id(app, "app:quit", "Quit MixRelay", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    // Menu::default on macOS starts with its application submenu.
    menu.remove_at(0)?;
    menu.prepend(&application)
}
