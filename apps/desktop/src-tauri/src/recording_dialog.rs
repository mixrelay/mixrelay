use std::path::{Path, PathBuf};
use tauri::Manager;

/// Resolves the desktop in the interactive user's OS session, not the service's
/// working directory. Naming does not create or reserve a recording file.
#[tauri::command]
pub(crate) async fn default_recording_path(
    app: tauri::AppHandle,
    file_name: String,
) -> Result<String, String> {
    // Desktop lookup may read the user's XDG configuration on Linux.
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = app
            .path()
            .desktop_dir()
            .map_err(|error| format!("cannot locate Desktop: {error}"))?;
        desktop_recording_path(&desktop, &file_name)?
            .into_os_string()
            .into_string()
            .map_err(|_| "Desktop path is not valid Unicode".into())
    })
    .await
    .map_err(|error| format!("Desktop lookup failed: {error}"))?
}

fn desktop_recording_path(desktop: &Path, file_name: &str) -> Result<PathBuf, String> {
    if !desktop.is_absolute()
        || file_name.is_empty()
        || file_name.contains(['/', '\\', ':'])
        || !Path::new(file_name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return Err("recording filename must be a WAV filename without directories".into());
    }
    Ok(desktop.join(file_name))
}

/// Selects a WAV destination in the desktop user's session. The engine opens it
/// exclusively on Record, so accepting this dialog never overwrites an existing file.
#[tauri::command]
pub(crate) async fn select_recording_path(
    app: tauri::AppHandle,
    default_path: Option<String>,
    title: Option<String>,
) -> Option<String> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title.as_deref().unwrap_or("Save recording"))
        .add_filter("WAV", &["wav"])
        .set_file_name("MixRelay.wav");
    if let Ok(desktop) = app.path().desktop_dir() {
        dialog = dialog.set_directory(desktop);
    }
    if let Some(path) = default_path {
        let path = std::path::Path::new(&path);
        if let Some(parent) = path.parent() {
            dialog = dialog.set_directory(parent);
        }
        if let Some(name) = path.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
    }
    dialog
        .save_file()
        .await
        .map(|file| file.path().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::desktop_recording_path;
    use std::path::Path;

    #[test]
    fn uses_resolved_desktop_without_creating_files() {
        let desktop = std::env::current_dir().unwrap().join("바탕 화면");
        let name = "MixRelay-2026-09-09_12-34-56-789_1.wav";
        assert_eq!(
            desktop_recording_path(&desktop, name).unwrap(),
            desktop.join(name)
        );
        assert_eq!(
            desktop_recording_path(&desktop, "Take.WAV").unwrap(),
            desktop.join("Take.WAV")
        );
    }

    #[test]
    fn rejects_directory_components_and_invalid_names_on_all_platforms() {
        let desktop = std::env::current_dir().unwrap();
        for name in [
            "",
            "../take.wav",
            "/take.wav",
            "..\\take.wav",
            "C:take.wav",
            "take.mp3",
        ] {
            assert!(desktop_recording_path(&desktop, name).is_err());
        }
        assert!(desktop_recording_path(Path::new("relative"), "take.wav").is_err());
    }
}
