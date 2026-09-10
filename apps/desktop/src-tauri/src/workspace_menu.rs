use tauri::{
    Emitter,
    menu::{Menu, MenuItem, Submenu},
};

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
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
        if let Some(action) = event.id().as_ref().strip_prefix("workspace:") {
            let _ = app.emit("workspace-file", action);
        }
    });
    Ok(())
}
