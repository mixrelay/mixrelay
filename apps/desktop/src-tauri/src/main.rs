use tauri::{Manager, WindowEvent};

mod control_proxy;
mod processor_dialog;
mod recording_dialog;
mod tray_menu;
mod workspace_menu;

#[cfg(target_os = "macos")]
mod engine_lifecycle;
#[cfg(target_os = "macos")]
mod native_termination;

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
    #[cfg(target_os = "macos")]
    match engine_lifecycle::EngineLifecycle::start() {
        Ok(lifecycle) => {
            app.manage(lifecycle);
            app.manage(native_termination::install(app.handle())?);
            install_local_controls(app)?;
        }
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("MixRelay")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(error.into());
        }
    }
    workspace_menu::install(app)?;
    if tauri::is_dev()
        && let Some(window) = app.get_webview_window("main")
    {
        window.set_title("MixRelay (Dev)")?;
    }
    tray_menu::install(app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_local_controls(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::MetadataExt;
    let directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&directory)?;
    let uid = std::fs::metadata(app.path().home_dir()?)?.uid();
    let control = control_proxy::local::LocalControl::new(
        app.state::<control_proxy::ControlProxyClient>()
            .inner()
            .clone(),
        std::sync::Arc::clone(
            app.state::<std::sync::Arc<macos_audio::ApplicationCaptureState>>()
                .inner(),
        ),
    )?;
    let socket = tauri::async_runtime::block_on(control_proxy::local::socket::LocalSocket::start(
        &directory.join("control"),
        uid,
        control,
    ))?;
    app.manage(socket);
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default()
        .manage(control_proxy::ControlProxyClient::default())
        .manage(control_proxy::workspace::WorkspaceFiles::default());
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(macos_audio::CaptureProbeState::default())
        .manage(std::sync::Arc::new(
            macos_audio::ApplicationCaptureState::default(),
        ));
    let builder = builder.invoke_handler(tauri::generate_handler![
        control_proxy::node_control::control_get_node_controls,
        control_proxy::node_control::control_update_node_control,
        tray_menu::set_tray_locale,
        control_proxy::control_get_status,
        control_proxy::workspace::workspace_file,
        control_proxy::control_reconfigure_session,
        control_proxy::control_set_engine_power,
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

    let application = builder
        .setup(setup_desktop)
        .on_window_event(handle_window_event)
        .build(tauri::generate_context!());
    let application = match application {
        Ok(application) => application,
        Err(error) => {
            #[cfg(target_os = "macos")]
            native_termination::release();
            tracing::error!(%error, "could not initialize MixRelay desktop");
            return;
        }
    };
    application.run(|app, event| handle_application_event(app, &event));
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == "main"
    {
        api.prevent_close();
        if let Err(error) = tray_menu::hide_main_window(window.app_handle()) {
            tracing::error!(%error, "could not hide main window to tray");
        }
    }
}

fn handle_application_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    #[cfg(target_os = "macos")]
    if matches!(event, tauri::RunEvent::Exit) {
        native_termination::release();
    }
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::ExitRequested { api, .. } = event {
        app.state::<engine_lifecycle::EngineLifecycle>()
            .exit_requested(app, api);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event
        && let Err(error) = tray_menu::show_main_window(app)
    {
        tracing::error!(%error, "could not reopen MixRelay main window");
    }
}
