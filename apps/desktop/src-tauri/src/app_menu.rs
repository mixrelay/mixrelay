use tauri::Manager;

use crate::workspace_menu::WorkspaceMenu;

#[derive(Clone, Copy)]
pub(crate) struct NativeMenuLabels {
    pub(crate) app_about: &'static str,
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "Only macOS has the application menu item")
    )]
    pub(crate) app_quit: &'static str,
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "Only Linux has Quit in the File menu")
    )]
    pub(crate) quit: &'static str,
    pub(crate) workspace_file: &'static str,
    pub(crate) workspace_new: &'static str,
    pub(crate) workspace_open: &'static str,
    pub(crate) workspace_save: &'static str,
    pub(crate) workspace_save_as: &'static str,
}

const LABELS: [NativeMenuLabels; 11] = [
    NativeMenuLabels {
        app_about: "About MixRelay",
        app_quit: "Quit MixRelay",
        quit: "Quit",
        workspace_file: "File",
        workspace_new: "New Workspace",
        workspace_open: "Open…",
        workspace_save: "Save",
        workspace_save_as: "Save As…",
    },
    NativeMenuLabels {
        app_about: "MixRelay 정보",
        app_quit: "MixRelay 종료",
        quit: "종료",
        workspace_file: "파일",
        workspace_new: "새 작업 공간",
        workspace_open: "열기…",
        workspace_save: "저장",
        workspace_save_as: "다른 이름으로 저장…",
    },
    NativeMenuLabels {
        app_about: "MixRelayについて",
        app_quit: "MixRelayを終了",
        quit: "終了",
        workspace_file: "ファイル",
        workspace_new: "新規ワークスペース",
        workspace_open: "開く…",
        workspace_save: "保存",
        workspace_save_as: "名前を付けて保存…",
    },
    NativeMenuLabels {
        app_about: "关于 MixRelay",
        app_quit: "退出 MixRelay",
        quit: "退出",
        workspace_file: "文件",
        workspace_new: "新建工作区",
        workspace_open: "打开…",
        workspace_save: "保存",
        workspace_save_as: "另存为…",
    },
    NativeMenuLabels {
        app_about: "關於 MixRelay",
        app_quit: "結束 MixRelay",
        quit: "結束",
        workspace_file: "檔案",
        workspace_new: "新增工作區",
        workspace_open: "開啟…",
        workspace_save: "儲存",
        workspace_save_as: "另存新檔…",
    },
    NativeMenuLabels {
        app_about: "À propos de MixRelay",
        app_quit: "Quitter MixRelay",
        quit: "Quitter",
        workspace_file: "Fichier",
        workspace_new: "Nouvel espace de travail",
        workspace_open: "Ouvrir…",
        workspace_save: "Enregistrer",
        workspace_save_as: "Enregistrer sous…",
    },
    NativeMenuLabels {
        app_about: "Über MixRelay",
        app_quit: "MixRelay beenden",
        quit: "Beenden",
        workspace_file: "Datei",
        workspace_new: "Neuer Arbeitsbereich",
        workspace_open: "Öffnen…",
        workspace_save: "Speichern",
        workspace_save_as: "Speichern unter…",
    },
    NativeMenuLabels {
        app_about: "Acerca de MixRelay",
        app_quit: "Salir de MixRelay",
        quit: "Salir",
        workspace_file: "Archivo",
        workspace_new: "Nuevo espacio de trabajo",
        workspace_open: "Abrir…",
        workspace_save: "Guardar",
        workspace_save_as: "Guardar como…",
    },
    NativeMenuLabels {
        app_about: "Sobre o MixRelay",
        app_quit: "Sair do MixRelay",
        quit: "Sair",
        workspace_file: "Arquivo",
        workspace_new: "Novo espaço de trabalho",
        workspace_open: "Abrir…",
        workspace_save: "Salvar",
        workspace_save_as: "Salvar como…",
    },
    NativeMenuLabels {
        app_about: "Informazioni su MixRelay",
        app_quit: "Esci da MixRelay",
        quit: "Esci",
        workspace_file: "File",
        workspace_new: "Nuovo spazio di lavoro",
        workspace_open: "Apri…",
        workspace_save: "Salva",
        workspace_save_as: "Salva con nome…",
    },
    NativeMenuLabels {
        app_about: "О MixRelay",
        app_quit: "Выйти из MixRelay",
        quit: "Выйти",
        workspace_file: "Файл",
        workspace_new: "Новое рабочее пространство",
        workspace_open: "Открыть…",
        workspace_save: "Сохранить",
        workspace_save_as: "Сохранить как…",
    },
];

pub(crate) fn labels(locale: &str) -> NativeMenuLabels {
    LABELS[crate::native_text::resolve_locale(locale).index()]
}

/// Hides the main window without stopping audio or removing its Dock presence.
/// Returns lookup, window or activation-policy errors without requesting exit.
pub(crate) fn hide_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?;
    window.hide()?;
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

/// Applies the UI's resolved locale to its role's native menu.
///
/// Returns a native error when an item label cannot be updated.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri injects State by value; a borrowed State is not a command argument"
)]
pub(crate) async fn set_native_menu_locale(
    locale: &str,
    app: tauri::AppHandle,
) -> Result<(), String> {
    crate::native_text::set_locale(locale);
    if let Some(menu) = app.try_state::<crate::driver_manager::DriverMenu>() {
        return menu.set_labels(locale).map_err(|error| error.to_string());
    }
    let workspace = app.state::<WorkspaceMenu>();
    let labels = labels(locale);
    workspace
        .set_labels(&labels)
        .map_err(|error| error.to_string())?;
    let connection = app
        .try_state::<crate::background::Connection>()
        .map(|state| state.inner().clone());
    if let Some(connection) = connection {
        connection
            .invoke(
                "set_native_menu_locale",
                serde_json::json!({"locale":locale}),
            )
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_supported_menus_have_translated_actions_and_chinese_aliases() {
        for (tag, file, quit) in [
            ("en", "File", "Quit"),
            ("ko-KR", "파일", "종료"),
            ("ja-JP", "ファイル", "終了"),
            ("zh-CN", "文件", "退出"),
            ("zh-HK", "檔案", "結束"),
            ("fr-FR", "Fichier", "Quitter"),
            ("de-DE", "Datei", "Beenden"),
            ("es-ES", "Archivo", "Salir"),
            ("pt-BR", "Arquivo", "Sair"),
            ("it-IT", "File", "Esci"),
            ("ru-RU", "Файл", "Выйти"),
        ] {
            let text = labels(tag);
            assert_eq!(text.workspace_file, file);
            assert_eq!(text.quit, quit);
            assert!(text.app_quit.contains("MixRelay"));
            assert!(text.app_about.contains("MixRelay"));
            for action in [
                text.workspace_new,
                text.workspace_open,
                text.workspace_save,
                text.workspace_save_as,
            ] {
                assert!(!action.is_empty());
            }
        }
        assert_eq!(labels("zh-Hans-TW").workspace_file, "文件");
        assert_eq!(labels("zh-Hant-CN").workspace_file, "檔案");
    }

    #[test]
    fn korean_menu_uses_concise_action_labels() {
        let korean = labels("ko-KR");
        assert_eq!(korean.app_about, "MixRelay 정보");
        assert_eq!(korean.quit, "종료");
        assert_eq!(korean.workspace_file, "파일");
        assert_eq!(korean.workspace_new, "새 작업 공간");
        assert_eq!(korean.workspace_open, "열기…");
        assert_eq!(korean.workspace_save, "저장");
        assert_eq!(korean.workspace_save_as, "다른 이름으로 저장…");

        assert_eq!(labels("ko").workspace_file, korean.workspace_file);
    }

    #[test]
    fn other_locales_use_english_like_the_ui() {
        for locale in ["en", "en-US", "pseudo", ""] {
            let english = labels(locale);
            assert_eq!(english.app_about, "About MixRelay");
            assert_eq!(english.quit, "Quit");
            assert_eq!(english.workspace_file, "File");
            assert_eq!(english.workspace_new, "New Workspace");
            assert_eq!(english.workspace_open, "Open…");
            assert_eq!(english.workspace_save, "Save");
            assert_eq!(english.workspace_save_as, "Save As…");
        }
    }
}
