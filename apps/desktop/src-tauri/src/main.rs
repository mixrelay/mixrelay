use tauri::{
    Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

mod control_proxy;
mod processor_dialog;
mod recording_dialog;

#[cfg(target_os = "macos")]
use tonenari_macos_transport as _;

#[cfg(target_os = "linux")]
mod linux_audio;
#[cfg(target_os = "macos")]
mod macos_audio;
#[cfg(target_os = "windows")]
mod windows_audio;

#[cfg(target_os = "linux")]
use linux_audio as platform_audio;
#[cfg(target_os = "macos")]
use macos_audio as platform_audio;
#[cfg(target_os = "windows")]
use windows_audio as platform_audio;

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_desktop(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if tauri::is_dev()
        && let Some(window) = app.get_webview_window("main")
    {
        window.set_title("ToneNari (Dev)")?;
    }
    let open = MenuItem::with_id(app, "open", "Open ToneNari", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit ToneNari", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    TrayIconBuilder::with_id("main")
        .title("TN")
        .tooltip("ToneNari")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default().manage(control_proxy::ControlProxyClient::default());
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(macos_audio::CaptureProbeState::default())
        .manage(std::sync::Arc::new(
            macos_audio::ApplicationCaptureState::default(),
        ));
    let builder = builder.invoke_handler(tauri::generate_handler![
        control_proxy::control_get_status,
        control_proxy::control_reconfigure_session,
        control_proxy::control_set_virtual_endpoint_sample_rate,
        control_proxy::control_get_graph,
        control_proxy::control_get_graph_catalog,
        control_proxy::control_get_graph_layout,
        control_proxy::control_set_graph_layout,
        control_proxy::control_mutate_graph_catalog,
        control_proxy::control_get_graph_status,
        control_proxy::control_get_meters,
        control_proxy::control_get_processors,
        control_proxy::control_get_processor_editor,
        control_proxy::control_get_processor_editor_frame,
        control_proxy::control_send_processor_editor_input,
        control_proxy::control_rescan_processors,
        control_proxy::control_set_processor_parameter,
        control_proxy::control_set_processor_editor,
        control_proxy::control_apply_graph,
        platform_audio::list_capturable_applications,
        platform_audio::list_audio_devices,
        #[cfg(target_os = "macos")]
        macos_audio::start_capture_probe,
        #[cfg(target_os = "macos")]
        macos_audio::capture_probe_stats,
        #[cfg(target_os = "macos")]
        macos_audio::stop_capture_probe,
        processor_dialog::select_processor_directory,
        recording_dialog::select_recording_path,
        recording_dialog::default_recording_path,
        control_proxy::recorder::control_get_recorder_status,
        control_proxy::recorder::control_set_recorder,
    ]);

    builder
        .setup(setup_desktop)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run ToneNari desktop application");
}
