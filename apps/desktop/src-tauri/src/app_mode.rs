use std::ffi::OsStr;

/// The installed manager and development switch share one isolated desktop entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppMode {
    Graph,
    Driver,
}

impl AppMode {
    pub(crate) fn detect<'a>(
        identifier: &str,
        mut arguments: impl Iterator<Item = &'a OsStr>,
    ) -> Self {
        if identifier == "io.mixrelay.driver.desktop"
            || arguments.any(|argument| argument == "--driver")
        {
            Self::Driver
        } else {
            Self::Graph
        }
    }

    /// Driver mode owns only its tray and name prompt; it never admits graph engine startup.
    pub(crate) const fn starts_graph(self) -> bool {
        matches!(self, Self::Graph)
    }

    /// Applies Driver prompt defaults to the main window without changing the bundle identity.
    pub(crate) fn configure(self, config: &mut tauri::Config) {
        if self == Self::Driver {
            config.product_name = Some("MixRelayDriver".into());
            if let Some(window) = config
                .app
                .windows
                .iter_mut()
                .find(|window| window.label == "main")
            {
                window.title = "MixRelay Driver".into();
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
        assert_eq!(config.product_name.as_deref(), Some("MixRelayDriver"));
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
