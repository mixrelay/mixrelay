/// Opens a native directory picker for an external processor search root.
///
/// Selection belongs to the logged-in desktop process. The audio service only receives the
/// resulting explicit directory and never attempts to infer a user's home directory.
#[tauri::command]
pub(crate) async fn select_processor_directory() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("오디오 플러그인 폴더 선택")
        .pick_folder()
        .await
        .map(|handle| handle.path().to_string_lossy().into_owned())
}
