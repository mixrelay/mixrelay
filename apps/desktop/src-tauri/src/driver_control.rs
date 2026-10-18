use mixrelay_audio_io::VirtualEndpointSettings;
use mixrelay_driver_control::{
    DriverClient, DriverCommand, DriverError, DriverSnapshot, DriverState,
};
use tauri::Manager as _;

/// The Driver client of this signed app. It claims paid access when the signed-in user's
/// license allows it; the macOS Driver service cannot read that keychain itself and
/// accepts the claim only from a process signed by its own team.
pub(crate) fn session_client() -> Result<DriverClient, DriverError> {
    Ok(DriverClient::session()?.claiming_paid_access(claimed_paid_access()))
}

/// Each new request observes the current lease, including expiry and remote revocation.
fn claimed_paid_access() -> bool {
    mixrelay_licensing::has_local_paid_access()
}

/// Read the installed Driver owner; free device settings do not require a license.
#[tauri::command]
pub(crate) async fn driver_status(app: tauri::AppHandle) -> Result<DriverSnapshot, String> {
    let client = session_client().map_err(|error| error.to_string())?;
    let snapshot = match client.snapshot().await {
        Ok(snapshot) => Ok(snapshot),
        Err(DriverError::Http(error)) if error.is_connect() => {
            tauri::async_runtime::spawn_blocking(unavailable_snapshot)
                .await
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }?;
    Ok(visible_snapshot(&app, snapshot).await)
}

/// Apply free Driver settings; creating, removing or renaming devices requires a license.
#[tauri::command]
pub(crate) async fn driver_mutate(
    app: tauri::AppHandle,
    command: DriverCommand,
) -> Result<DriverSnapshot, String> {
    let client = session_client().map_err(|error| error.to_string())?;
    let rename = if let DriverCommand::Configure { spec, .. } = &command {
        client
            .snapshot()
            .await
            .map_err(|error| error.to_string())?
            .settings
            .devices
            .iter()
            .find(|device| device.spec.id == spec.id)
            .is_some_and(|device| device.spec.display_name != spec.display_name)
    } else {
        false
    };
    if (rename || requires_paid_inventory_access(&command)) && !paid_access(&app).await? {
        return Err("Adding, removing or renaming devices requires a MixRelay license".into());
    }
    let snapshot = client
        .mutate(&command)
        .await
        .map_err(|error| error.to_string())?;
    let snapshot = visible_snapshot(&app, snapshot).await;
    if let Some(menu) = app.try_state::<crate::status_menu::StatusMenu>() {
        menu.observe_snapshot(&snapshot);
    }
    Ok(snapshot)
}

async fn visible_snapshot(app: &tauri::AppHandle, mut snapshot: DriverSnapshot) -> DriverSnapshot {
    if !paid_access(app).await.unwrap_or(false) {
        snapshot.settings.can_create = false;
        snapshot.settings.can_rename = false;
    }
    snapshot
}

fn requires_paid_inventory_access(command: &DriverCommand) -> bool {
    matches!(
        command,
        DriverCommand::Create { .. } | DriverCommand::Remove { .. }
    )
}

async fn paid_access(app: &tauri::AppHandle) -> Result<bool, String> {
    let state = std::sync::Arc::clone(
        app.try_state::<std::sync::Arc<crate::licensing::LicensingState>>()
            .ok_or("MixRelay license state is unavailable")?
            .inner(),
    );
    crate::licensing::has_paid_access(state)
        .await
        .map_err(|error| error.to_string())
}

fn unavailable_snapshot() -> DriverSnapshot {
    let installed = installed_driver_path().is_file();
    absent_owner_snapshot(installed, installed_driver_version())
}

fn absent_owner_snapshot(installed: bool, installed_version: Option<String>) -> DriverSnapshot {
    DriverSnapshot {
        protocol_version: 1,
        revision: 0,
        state: if installed {
            DriverState::InstalledNotLoaded
        } else {
            DriverState::NotInstalled
        },
        installed_version,
        owner_process_id: None,
        settings: VirtualEndpointSettings {
            can_create: false,
            can_rename: false,
            render_channels: Vec::new(),
            capture_channels: Vec::new(),
            creation_template: None,
            devices: Vec::new(),
        },
        persistence_error: None,
    }
}

#[cfg(target_os = "macos")]
fn installed_driver_path() -> &'static std::path::Path {
    std::path::Path::new("/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver/Contents/Info.plist")
}

#[cfg(target_os = "linux")]
fn installed_driver_path() -> &'static std::path::Path {
    std::path::Path::new("/usr/bin/mixrelay-driver-service")
}

/// Reads the installed Driver artifact's version, never the graph application's version.
#[cfg(target_os = "macos")]
pub(crate) fn installed_driver_version() -> Option<String> {
    installed_plist_version(installed_driver_path())
}

#[cfg(target_os = "macos")]
fn installed_plist_version(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    installed_version_text(&String::from_utf8(output.stdout).ok()?)
}

/// Reads the installed app bundle's version.
#[cfg(target_os = "macos")]
pub(crate) fn installed_mixrelay_version() -> Option<String> {
    installed_plist_version(std::path::Path::new(
        mixrelay_macos_transport::install_contract::APP_INFO_PATH,
    ))
}

/// Reads the package actually owning the graph app executable, without starting that app.
#[cfg(target_os = "linux")]
pub(crate) fn installed_mixrelay_version() -> Option<String> {
    const EXECUTABLE: &str = "/usr/bin/mixrelay-desktop";
    if !std::path::Path::new(EXECUTABLE).is_file() {
        return None;
    }
    let deb_version = std::process::Command::new("dpkg-query")
        .args(["--search", EXECUTABLE])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|owners| {
            let owner = owners
                .lines()
                .find_map(|line| line.strip_suffix(": /usr/bin/mixrelay-desktop"))?;
            package_version("dpkg-query", &["--show", "--showformat=${Version}", owner])
        });
    deb_version.or_else(|| {
        package_version(
            "rpm",
            &[
                "--query",
                "--file",
                EXECUTABLE,
                "--queryformat",
                "%{VERSION}",
            ],
        )
    })
}

#[cfg(target_os = "linux")]
fn package_version(program: &str, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    installed_version_text(&String::from_utf8(output.stdout).ok()?)
}

/// Reads the receipt shipped with the installed Linux Driver package.
#[cfg(target_os = "linux")]
pub(crate) fn installed_driver_version() -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Receipt {
        version: String,
    }
    if !installed_driver_path().is_file() {
        return None;
    }
    let bytes = std::fs::read("/usr/share/mixrelay-driver/version.json").ok()?;
    let receipt: Receipt = serde_json::from_slice(&bytes).ok()?;
    installed_version_text(&receipt.version)
}

fn installed_version_text(value: &str) -> Option<String> {
    let value = value.trim();
    let version = semver::Version::parse(value).ok()?;
    (version.pre.is_empty() && version.build.is_empty() && version.to_string() == value)
        .then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_changes_require_paid_access() {
        let command = DriverCommand::Remove {
            expected_revision: 0,
            endpoint_id: mixrelay_audio_io::VirtualEndpointId::from("device"),
        };
        assert!(requires_paid_inventory_access(&command));
    }

    #[test]
    fn absent_service_never_reports_ready_or_mutation_capabilities() {
        for installed in [true, false] {
            let snapshot = absent_owner_snapshot(installed, None);
            assert_ne!(snapshot.state, DriverState::Ready);
            assert!(!snapshot.settings.can_create);
            assert!(!snapshot.settings.can_rename);
            assert!(snapshot.settings.creation_template.is_none());
            assert_eq!(snapshot.installed_version, None);
            assert!(snapshot.persistence_error.is_none());
        }
    }

    #[test]
    fn installed_artifact_version_is_preserved_without_a_running_service() {
        let snapshot = absent_owner_snapshot(true, Some("0.2.3".into()));
        assert_eq!(snapshot.state, DriverState::InstalledNotLoaded);
        assert_eq!(snapshot.installed_version.as_deref(), Some("0.2.3"));
    }

    #[test]
    fn missing_or_invalid_versions_are_not_guessed() {
        assert_eq!(installed_version_text(" 0.2.3\n"), Some("0.2.3".into()));
        for value in ["", "unknown", "0.2", "0.2.3-dev", "0.2.3+local"] {
            assert_eq!(installed_version_text(value), None);
        }
    }
}
