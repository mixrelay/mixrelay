use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

/// Native items retained for updates from the UI's resolved language.
pub(crate) struct TrayMenu {
    open: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

fn labels(locale: &str) -> (&'static str, &'static str) {
    if locale.starts_with("ko") {
        ("열기", "종료")
    } else {
        ("Open", "Quit")
    }
}

/// Installs the embedded brand icon and native menu, returning native setup errors.
pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let (open_label, quit_label) = labels("en");
    let title = MenuItem::with_id(app, "tray:title", "MixRelay", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "tray:open", open_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray:quit", quit_label, true, None::<&str>)?;
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

/// Applies the UI's resolved locale; returns a native error if either update fails.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri injects State by value; a borrowed State is not a command argument"
)]
pub(crate) fn set_tray_locale(
    locale: &str,
    menu: tauri::State<'_, TrayMenu>,
) -> Result<(), String> {
    let (open, quit) = labels(locale);
    menu.open
        .set_text(open)
        .map_err(|error| error.to_string())?;
    menu.quit.set_text(quit).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_menu_uses_concise_action_labels() {
        assert_eq!(labels("ko-KR"), ("열기", "종료"));
        assert_eq!(labels("ko"), labels("ko-KR"));
    }

    #[test]
    fn other_locales_use_english_like_the_ui() {
        for locale in ["en", "en-US", "ja", "pseudo", ""] {
            assert_eq!(labels(locale), ("Open", "Quit"));
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
