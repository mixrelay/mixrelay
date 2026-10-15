use tauri::{Manager, WindowEvent};

mod app_menu;
mod app_mode;
mod background;
mod control_proxy;
mod diagnostics;
mod driver_control;
mod licensing;
mod logging;
mod native_text;
mod processor_dialog;
mod recording_dialog;
mod status_menu;
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
    #[cfg(target_os = "macos")]
    app.manage(native_termination::install(app.handle())?);
    if *app.state::<app_mode::AppMode>() == app_mode::AppMode::Graph {
        start_audio(app.handle())?;
        workspace_menu::install(app)?;
        if tauri::is_dev()
            && let Some(window) = app.get_webview_window("main")
        {
            window.set_title("MixRelay (Dev)")?;
        }
        return Ok(());
    }
    control_proxy::recording_settings::initialize(app)?;
    app.state::<std::sync::Arc<licensing::LicensingState>>()
        .set_journal_path(background::settings_directory(app.handle())?.join(
            if cfg!(debug_assertions) {
                "license-remote-change-dev.json"
            } else {
                "license-remote-change.json"
            },
        ));
    let access = licensing::now().and_then(|now| {
        app.state::<std::sync::Arc<licensing::LicensingState>>()
            .snapshot(now)
    });
    if let Err(error) = access {
        tracing::warn!(%error, "paid access unavailable; starting free edition");
    }
    // Publish control identity only after lifecycle ownership is admitted.
    // A duplicate cold launch must never bind ahead of the real controller.
    start_audio(app.handle())?;
    install_local_controls(app.handle())?;
    status_menu::install(app)?;
    #[cfg(target_os = "macos")]
    native_termination::observe_background_stop(app.handle())?;
    background::restore_driver(app.handle());
    restore_captures(app.handle());
    Ok(())
}

/// The controller restores capture ownership even when no editor is open, at startup and
/// after a lost engine is replaced.
pub(crate) fn restore_captures(app: &tauri::AppHandle) {
    let controller = app.clone();
    tauri::async_runtime::spawn(async move {
        for _ in 0..30 {
            if background::dispatch(
                &controller,
                "control_get_graph_catalog",
                serde_json::json!({}),
            )
            .await
            .is_ok_and(|result| result["status"] == 200)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

/// Free audio remains available independently of paid entitlement.
fn start_audio(app: &tauri::AppHandle) -> Result<(), String> {
    if *app.state::<app_mode::AppMode>() == app_mode::AppMode::Graph {
        if app.try_state::<background::Connection>().is_none() {
            let connection = background::Connection::for_app(app)?;
            *app.state::<control_proxy::ControlProxyClient>()
                .background
                .lock()
                .map_err(|_| "background connection lock poisoned")? = Some(connection.clone());
            app.manage(connection);
        }
        return Ok(());
    }
    if app
        .try_state::<engine_lifecycle::EngineLifecycle>()
        .is_some()
    {
        // The macOS runtime is admitted before its event loop; watch it once here.
        #[cfg(target_os = "macos")]
        engine_lifecycle::monitor(app);
        return Ok(());
    }
    app.manage(engine_lifecycle::EngineLifecycle::start()?);
    engine_lifecycle::monitor(app);
    Ok(())
}

fn install_local_controls(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::MetadataExt;
    let directory = background::runtime_directory(app)?;
    std::fs::create_dir_all(&directory)?;
    let uid = std::fs::metadata(app.path().home_dir()?)?.uid();
    let control = control_proxy::local::LocalControl::new(
        app.state::<control_proxy::ControlProxyClient>()
            .inner()
            .clone(),
        #[cfg(target_os = "macos")]
        std::sync::Arc::clone(
            app.state::<std::sync::Arc<macos_audio::ApplicationCaptureState>>()
                .inner(),
        ),
    )?
    .with_app(app.clone());
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
    logging::init(
        if arguments
            .iter()
            .any(|argument| argument == "--driver-runtime")
        {
            "driver-runtime"
        } else {
            "desktop"
        },
    );
    if arguments
        .iter()
        .any(|argument| argument == "--background-service")
    {
        tracing::error!("MixRelay Driver owns audio; the retired App service entry cannot start");
        std::process::exit(2);
    }
    let mode = app_mode::AppMode::detect(arguments.iter().map(std::ffi::OsString::as_os_str));
    mode.configure(context.config_mut());
    let builder = tauri::Builder::default().manage(mode);
    #[cfg(target_os = "macos")]
    let builder = if mode == app_mode::AppMode::Service {
        // Ownership contention is an expected rejected launch. Tauri panics on
        // setup-hook errors, so admit the controller before starting its loop.
        match engine_lifecycle::EngineLifecycle::start() {
            Ok(lifecycle) => builder.manage(lifecycle),
            Err(error) => {
                tracing::error!(%error, "Driver runtime startup was not admitted");
                std::process::exit(1);
            }
        }
    } else {
        builder
    };
    let builder = if mode == app_mode::AppMode::Graph {
        builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            background::reopen_editor(app);
        }))
    } else {
        builder
    };
    let application = graph_builder(builder)
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
    if mode == app_mode::AppMode::Service {
        // Setup runs after Tao applies its initial policy at application launch.
        application.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    application.run(|app, event| handle_application_event(app, &event));
}

fn graph_builder(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let builder = builder
        .manage(std::sync::Arc::new(licensing::LicensingState::default()))
        .manage(control_proxy::ControlProxyClient::default())
        .manage(control_proxy::workspace::WorkspaceFiles::default())
        .manage(control_proxy::telemetry::TelemetrySubscriptions::default());
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(macos_audio::CaptureProbeState::default())
        .manage(std::sync::Arc::new(
            macos_audio::ApplicationCaptureState::default(),
        ));
    let handler: Box<dyn Fn(tauri::ipc::Invoke) -> bool + Send + Sync> =
        Box::new(tauri::generate_handler![
            logging::webview_error,
            background::startup::runtime_connect,
            background::startup::runtime_status,
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
            app_menu::set_native_menu_locale,
            control_proxy::control_get_status,
            control_proxy::workspace::workspace_file,
            control_proxy::control_reconfigure_session,
            control_proxy::control_set_virtual_endpoint_buffer_frames,
            control_proxy::control_set_engine_power,
            control_proxy::control_set_virtual_endpoint_sample_rate,
            control_proxy::control_get_graph,
            control_proxy::control_get_graph_catalog,
            control_proxy::control_get_graph_layout,
            control_proxy::control_set_graph_layout,
            control_proxy::control_mutate_graph_catalog,
            control_proxy::control_get_graph_status,
            control_proxy::control_get_meters,
            control_proxy::telemetry::control_subscribe_telemetry,
            control_proxy::telemetry::control_telemetry_credit,
            control_proxy::telemetry::control_unsubscribe_telemetry,
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
            platform_audio::list_device_app_activity,
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
        ]);
    builder.invoke_handler(move |invoke| match background::forward(invoke) {
        None => true,
        Some(invoke) => handler(invoke),
    })
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == "main"
    {
        #[cfg(target_os = "linux")]
        if *window.app_handle().state::<app_mode::AppMode>() == app_mode::AppMode::Graph {
            api.prevent_close();
            window.app_handle().exit(0);
            return;
        }
        api.prevent_close();
        if let Err(error) = app_menu::hide_main_window(window.app_handle()) {
            tracing::error!(%error, "could not hide main window");
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
        && *app.state::<app_mode::AppMode>() == app_mode::AppMode::Service
    {
        // With no editor running, reopening MixRelay targets this process, and
        // LaunchServices promotes it to a Dock app because the shared bundle is a
        // regular app. The runtime stays in the background; the editor it opens is
        // the one Dock app.
        if let Err(error) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
            tracing::error!(%error, "could not keep the runtime in the background");
        }
        status_menu::open_editor();
    }
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event
        && *app.state::<app_mode::AppMode>() == app_mode::AppMode::Graph
    {
        background::reopen_editor(app);
    }
}
