use std::ffi::OsStr;

/// The installed manager and development switch share one isolated desktop entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppMode {
    Graph,
    Driver,
    Service,
}

impl AppMode {
    pub(crate) fn detect<'a>(identifier: &str, arguments: impl Iterator<Item = &'a OsStr>) -> Self {
        let arguments: Vec<_> = arguments.collect();
        if arguments.contains(&OsStr::new("--driver-runtime")) {
            Self::Service
        } else if identifier == "io.mixrelay.driver.desktop"
            || arguments.iter().any(|argument| *argument == "--driver")
        {
            Self::Driver
        } else {
            Self::Graph
        }
    }

    /// The editor and background controller can author graph work; Driver-only mode cannot.
    pub(crate) const fn starts_graph(self) -> bool {
        matches!(self, Self::Graph | Self::Service)
    }

    /// Applies Driver prompt defaults to the main window without changing the bundle identity.
    pub(crate) fn configure(self, config: &mut tauri::Config) {
        if self == Self::Service {
            // The controller has no editor webview or Dock window.
            config.app.windows.clear();
        } else if self == Self::Driver {
            config.product_name = Some("MixRelay".into());
            if let Some(window) = config
                .app
                .windows
                .iter_mut()
                .find(|window| window.label == "main")
            {
                window.title = "MixRelay".into();
                window.url = tauri::WebviewUrl::App("index.html?mode=driver".into());
                window.width = 400.0;
                window.height = 180.0;
                window.min_width = Some(400.0);
                window.min_height = Some(180.0);
                window.resizable = false;
                window.visible = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_runtime_has_no_editor_window_and_editor_cannot_select_it() {
        let args = [OsStr::new("--driver-runtime")];
        let mode = AppMode::detect("io.mixrelay.driver.desktop", args.into_iter());
        assert_eq!(mode, AppMode::Service);
        let mut config = tauri::Config::default();
        config
            .app
            .windows
            .push(tauri::utils::config::WindowConfig::default());
        mode.configure(&mut config);
        assert!(config.app.windows.is_empty());
        assert_eq!(
            AppMode::detect("io.mixrelay.desktop", args.into_iter()),
            AppMode::Service
        );
    }

    #[test]
    fn installed_driver_manager_does_not_need_a_cli_switch() {
        let mode = AppMode::detect("io.mixrelay.driver.desktop", std::iter::empty());
        assert_eq!(mode, AppMode::Driver);
        assert!(!mode.starts_graph());
    }

    #[test]
    fn development_switch_selects_driver_without_affecting_graph_default() {
        assert_eq!(
            AppMode::detect("io.mixrelay.desktop", std::iter::empty()),
            AppMode::Graph
        );
        assert_eq!(
            AppMode::detect("io.mixrelay.desktop", [OsStr::new("--driver")].into_iter()),
            AppMode::Driver
        );
    }

    #[test]
    fn manager_webview_uses_driver_route_and_preserves_bundle_identity() {
        let mut config = tauri::Config {
            identifier: "io.mixrelay.desktop".into(),
            ..Default::default()
        };
        config
            .app
            .windows
            .push(tauri::utils::config::WindowConfig::default());
        AppMode::Driver.configure(&mut config);
        assert_eq!(config.identifier, "io.mixrelay.desktop");
        assert_eq!(config.product_name.as_deref(), Some("MixRelay"));
        assert!(!config.app.windows[0].visible);
        assert!(!config.app.windows[0].resizable);
        assert_eq!(config.app.windows[0].width.to_bits(), 400.0_f64.to_bits());
        assert_eq!(config.app.windows[0].height.to_bits(), 180.0_f64.to_bits());
        assert_eq!(config.app.windows[0].min_height, Some(180.0));
        assert_eq!(
            config.app.windows[0].url,
            tauri::WebviewUrl::App("index.html?mode=driver".into())
        );
    }

    #[test]
    fn graph_mode_preserves_its_visible_resizable_window() {
        let mut config = tauri::Config::default();
        let window = tauri::utils::config::WindowConfig {
            width: 1180.0,
            height: 780.0,
            ..Default::default()
        };
        config.app.windows.push(window);
        AppMode::Graph.configure(&mut config);
        assert!(config.app.windows[0].visible);
        assert!(config.app.windows[0].resizable);
        assert_eq!(config.app.windows[0].width.to_bits(), 1180.0_f64.to_bits());
        assert_eq!(config.app.windows[0].height.to_bits(), 780.0_f64.to_bits());
    }
}
