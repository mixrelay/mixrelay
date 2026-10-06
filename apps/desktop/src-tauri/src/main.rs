use tauri::{Manager, WindowEvent};

mod app_mode;
mod control_proxy;
mod driver_control;
mod driver_manager;
mod licensing;
mod processor_dialog;
mod recording_dialog;
mod tray_menu;
mod updates;
mod workspace_menu;

#[cfg(target_os = "linux")]
mod child_engine_lifecycle;
#[cfg(target_os = "macos")]
mod engine_lifecycle;
#[cfg(target_os = "linux")]
use child_engine_lifecycle as engine_lifecycle;
#[cfg(target_os = "macos")]
mod native_termination;

#[cfg(target_os = "macos")]
use mixrelay_macos_transport as _;

#[cfg(target_os = "linux")]
mod linux_audio;
#[cfg(target_os = "macos")]
mod macos_audio;

#[cfg(target_os = "linux")]
use linux_audio as platform_audio;
#[cfg(target_os = "macos")]
use macos_audio as platform_audio;

fn setup_desktop(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if !app.state::<app_mode::AppMode>().starts_graph() {
        driver_manager::install(app)?;
        return Ok(());
    }
    control_proxy::recording_settings::initialize(app)?;
    app.state::<std::sync::Arc<licensing::LicensingState>>()
        .set_journal_path(app.path().app_data_dir()?.join(if cfg!(debug_assertions) {
            "license-remote-change-dev.json"
        } else {
            "license-remote-change.json"
        }));
    app.state::<control_proxy::ControlProxyClient>()
        .attach_license_state(std::sync::Arc::clone(
            app.state::<std::sync::Arc<licensing::LicensingState>>()
                .inner(),
        ));
    #[cfg(target_os = "macos")]
    app.manage(native_termination::install(app.handle())?);
    let access = licensing::now().and_then(|now| {
        app.state::<std::sync::Arc<licensing::LicensingState>>()
            .snapshot(now)
    });
    let allowed = access.as_ref().is_ok_and(licensing::allows_audio);
    app.state::<control_proxy::ControlProxyClient>()
        .set_license_admission(allowed);
    if allowed {
        start_audio(app.handle()).map_err(std::io::Error::other)?;
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

/// Start only after local access is established; expired access still permits the license UI.
fn start_audio(app: &tauri::AppHandle) -> Result<(), String> {
    if !app.state::<app_mode::AppMode>().starts_graph() {
        return Err("The Driver manager does not run graph audio".into());
    }
    if app
        .try_state::<engine_lifecycle::EngineLifecycle>()
        .is_some()
    {
        return Ok(());
    }
    let lifecycle = engine_lifecycle::EngineLifecycle::start()?;
    #[cfg(target_os = "linux")]
    {
        app.manage(lifecycle);
        engine_lifecycle::monitor(app);
    }
    #[cfg(target_os = "macos")]
    {
        install_local_controls(app).map_err(|error| error.to_string())?;
        app.manage(lifecycle);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_local_controls(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
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

fn handle_license_maintenance() {
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_some_and(|argument| argument == "--clear-local-license")
    {
        if arguments.next().is_some() {
            std::process::exit(2);
        }
        let result = mixrelay_licensing::clear_local_license();
        if let Err(error) = &result {
            tracing::error!(%error, "could not clear local license authentication");
        }
        std::process::exit(i32::from(result.is_err()));
    }
}

fn main() {
    handle_license_maintenance();
    let mut context = tauri::generate_context!();
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let mode = app_mode::AppMode::detect(
        &context.config().identifier,
        arguments.iter().map(std::ffi::OsString::as_os_str),
    );
    mode.configure(context.config_mut());
    let builder = tauri::Builder::default().manage(mode);
    let builder = if mode.starts_graph() {
        graph_builder(builder)
    } else {
        builder.invoke_handler(tauri::generate_handler![
            updates::updates_check,
            updates::updates_open_download,
            tray_menu::set_native_menu_locale,
            driver_control::driver_status,
            driver_control::driver_mutate,
        ])
    };
    let application = builder
        .setup(setup_desktop)
        .on_window_event(handle_window_event)
        .build(context);
    let application = match application {
        Ok(application) => application,
        Err(error) => {
            #[cfg(target_os = "macos")]
            native_termination::release();
            tracing::error!(%error, "could not initialize MixRelay desktop");
            return;
        }
    };
    #[cfg(target_os = "macos")]
    let mut application = application;
    #[cfg(target_os = "macos")]
    if !mode.starts_graph() {
        // Setup runs after Tao applies its initial policy at application launch.
        application.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    application.run(|app, event| handle_application_event(app, &event));
}

fn graph_builder(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let builder = builder
        .manage(std::sync::Arc::new(licensing::LicensingState::default()))
        .manage(control_proxy::ControlProxyClient::default())
        .manage(control_proxy::workspace::WorkspaceFiles::default());
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(macos_audio::CaptureProbeState::default())
        .manage(std::sync::Arc::new(
            macos_audio::ApplicationCaptureState::default(),
        ));
    builder.invoke_handler(tauri::generate_handler![
        driver_control::driver_status,
        driver_control::driver_mutate,
        updates::updates_check,
        updates::updates_open_download,
        licensing::licensing_status,
        licensing::licensing_open_checkout,
        licensing::licensing_checkout,
        licensing::licensing_activate,
        licensing::licensing_validate,
        licensing::licensing_deactivate,
        control_proxy::node_control::control_get_node_controls,
        control_proxy::node_control::control_update_node_control,
        tray_menu::set_native_menu_locale,
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
        control_proxy::control_retry_processor,
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
        recording_dialog::select_recording_folder,
        control_proxy::recording_settings::recording_settings,
        control_proxy::recording_settings::save_recording_settings,
        control_proxy::recorder::control_get_recorder_status,
        control_proxy::recorder::control_set_recorder,
    ])
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
    if let tauri::RunEvent::ExitRequested { api, .. } = event
        && let Some(lifecycle) = app.try_state::<engine_lifecycle::EngineLifecycle>()
    {
        lifecycle.exit_requested(app, api);
    }
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event
        && app.state::<app_mode::AppMode>().starts_graph()
        && let Err(error) = tray_menu::show_main_window(app)
    {
        tracing::error!(%error, "could not reopen MixRelay main window");
    }
}
