use tauri::{Manager, WindowEvent};

mod control_proxy;
mod processor_dialog;
mod recording_dialog;
mod tray_menu;

#[cfg(target_os = "macos")]
use mixrelay_macos_transport as _;

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

fn setup_desktop(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if tauri::is_dev()
        && let Some(window) = app.get_webview_window("main")
    {
        window.set_title("MixRelay (Dev)")?;
    }
    tray_menu::install(app)?;
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
        tray_menu::set_tray_locale,
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
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::error!(%error, "could not hide main window to tray");
                    return;
                }
                #[cfg(target_os = "macos")]
                if let Err(error) = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory)
                {
                    tracing::error!(%error, "could not remove hidden main window from Dock");
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build MixRelay desktop application")
        .run(|app, event| {
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event
                && let Err(error) = tray_menu::show_main_window(app)
            {
                tracing::error!(%error, "could not reopen MixRelay main window");
            }
        });
}
