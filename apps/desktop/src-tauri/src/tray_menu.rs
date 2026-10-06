use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::workspace_menu::WorkspaceMenu;

/// Native items retained for updates from the UI's resolved language.
pub(crate) struct TrayMenu {
    open: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

pub(crate) struct NativeMenuLabels {
    pub(crate) tray_open: &'static str,
    pub(crate) tray_quit: &'static str,
    pub(crate) workspace_file: &'static str,
    pub(crate) workspace_new: &'static str,
    pub(crate) workspace_open: &'static str,
    pub(crate) workspace_save: &'static str,
    pub(crate) workspace_save_as: &'static str,
}

pub(crate) fn labels(locale: &str) -> NativeMenuLabels {
    if locale.starts_with("ko") {
        NativeMenuLabels {
            tray_open: "열기",
            tray_quit: "종료",
            workspace_file: "파일",
            workspace_new: "새 작업 공간",
            workspace_open: "열기…",
            workspace_save: "저장",
            workspace_save_as: "다른 이름으로 저장…",
        }
    } else {
        NativeMenuLabels {
            tray_open: "Open",
            tray_quit: "Quit",
            workspace_file: "File",
            workspace_new: "New Workspace",
            workspace_open: "Open…",
            workspace_save: "Save",
            workspace_save_as: "Save As…",
        }
    }
}

/// Installs the embedded brand icon and native menu, returning native setup errors.
pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let initial_labels = labels("en");
    let title = MenuItem::with_id(app, "tray:title", "MixRelay", false, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        "tray:open",
        initial_labels.tray_open,
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "tray:quit",
        initial_labels.tray_quit,
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&title, &separator, &open, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("tray-icons/64x64.png"))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("MixRelay")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray:open" => {
                if let Err(error) = show_main_window(app) {
                    tracing::error!(%error, "could not restore MixRelay window from tray");
                }
            }
            "tray:quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(TrayMenu { open, quit });
    Ok(())
}

/// Hides the main window and Dock presence without stopping the app or audio.
/// Returns lookup, window or activation-policy errors without requesting exit.
pub(crate) fn hide_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?;
    window.hide()?;
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
    Ok(())
}

/// Restores the main window and its Dock presence, returning native window errors.
pub(crate) fn show_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?;
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Regular)?;
    window.unminimize()?;
    window.show()?;
    window.set_focus()
}

/// Applies the UI's resolved locale to both native menus.
///
/// Returns a native error when an item label cannot be updated.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri injects State by value; a borrowed State is not a command argument"
)]
pub(crate) fn set_native_menu_locale(locale: &str, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(menu) = app.try_state::<crate::driver_manager::DriverMenu>() {
        return menu.set_labels(locale).map_err(|error| error.to_string());
    }
    let tray = app.state::<TrayMenu>();
    let workspace = app.state::<WorkspaceMenu>();
    let labels = labels(locale);
    tray.open
        .set_text(labels.tray_open)
        .map_err(|error| error.to_string())?;
    tray.quit
        .set_text(labels.tray_quit)
        .map_err(|error| error.to_string())?;
    workspace
        .set_labels(&labels)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_menu_uses_concise_action_labels() {
        let korean = labels("ko-KR");
        assert_eq!(korean.tray_open, "열기");
        assert_eq!(korean.tray_quit, "종료");
        assert_eq!(korean.workspace_file, "파일");
        assert_eq!(korean.workspace_new, "새 작업 공간");
        assert_eq!(korean.workspace_open, "열기…");
        assert_eq!(korean.workspace_save, "저장");
        assert_eq!(korean.workspace_save_as, "다른 이름으로 저장…");

        assert_eq!(labels("ko").workspace_file, korean.workspace_file);
    }

    #[test]
    fn other_locales_use_english_like_the_ui() {
        for locale in ["en", "en-US", "ja", "pseudo", ""] {
            let english = labels(locale);
            assert_eq!(english.tray_open, "Open");
            assert_eq!(english.tray_quit, "Quit");
            assert_eq!(english.workspace_file, "File");
            assert_eq!(english.workspace_new, "New Workspace");
            assert_eq!(english.workspace_open, "Open…");
            assert_eq!(english.workspace_save, "Save");
            assert_eq!(english.workspace_save_as, "Save As…");
        }
    }

    #[test]
    fn embedded_brand_icon_has_expected_retina_dimensions() {
        let icon = tauri::include_image!("tray-icons/64x64.png");
        assert_eq!((icon.width(), icon.height()), (64, 64));
        assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
    }
}
