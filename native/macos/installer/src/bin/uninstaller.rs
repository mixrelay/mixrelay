//! Native Driver removal consent; the existing signed CLI owns the transaction.

#[cfg(target_os = "macos")]
#[path = "uninstaller/ui.rs"]
mod ui;

fn main() {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    if let Err(error) = run() {
        tracing::error!("MixRelay Driver removal failed: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run() -> anyhow::Result<()> {
    objc2::rc::autoreleasepool(|_| ui::run())
}

#[cfg(not(target_os = "macos"))]
fn run() -> anyhow::Result<()> {
    anyhow::bail!("Uninstall MixRelay Driver is available only on macOS")
}

#[cfg(any(target_os = "macos", test))]
fn validate_bundle_layout(
    bundle: &std::path::Path,
    executable: &std::path::Path,
    helper: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        bundle.is_absolute(),
        "The uninstaller must be an application bundle."
    );
    anyhow::ensure!(
        executable == bundle.join("Contents/MacOS/mixrelay-driver-uninstaller")
            && helper == bundle.join("Contents/Resources/mixrelay-macos-installer"),
        "The uninstaller contains an unexpected executable path."
    );
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn removal_command(
    helper: &std::path::Path,
    team: &str,
    delete_settings: bool,
) -> anyhow::Result<String> {
    let helper = helper
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid helper path."))?;
    anyhow::ensure!(
        helper.starts_with('/') && !helper.chars().any(char::is_control),
        "Invalid helper path."
    );
    mixrelay_macos_installer::SigningTeam::new(team.to_owned())?;
    let mut command = format!(
        "/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C {} remove {} --reload-coreaudio",
        shell_argument(helper),
        shell_argument(team)
    );
    if delete_settings {
        command.push_str(" --delete-user-data");
    }
    Ok(command)
}

#[cfg(any(target_os = "macos", test))]
fn shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(any(target_os = "macos", test))]
fn authorization_script(command: &str) -> String {
    let literal = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{literal}\" with administrator privileges")
}

#[cfg(any(target_os = "macos", test))]
fn authorization_cancelled(error_number: Option<i32>) -> bool {
    error_number == Some(-128)
}

#[cfg(any(target_os = "macos", test))]
fn manager_should_restart(was_running: bool, removal_succeeded: bool) -> bool {
    was_running && !removal_succeeded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn removal_preserves_settings_unless_selected_and_quotes_bundle_paths() {
        let helper = Path::new(
            "/Applications/It's MixRelay.app/Contents/Resources/mixrelay-macos-installer",
        );
        let command = removal_command(helper, "ABCDEFGHIJ", false).unwrap();
        assert!(command.contains("It'\\''s MixRelay.app"));
        assert!(command.ends_with(" remove 'ABCDEFGHIJ' --reload-coreaudio"));
        let selected = removal_command(helper, "ABCDEFGHIJ", true).unwrap();
        assert_eq!(selected, format!("{command} --delete-user-data"));
        assert!(removal_command(helper, "bad team", false).is_err());
        assert!(removal_command(Path::new("/unsafe\npath"), "ABCDEFGHIJ", false).is_err());
    }

    #[test]
    fn only_fixed_bundle_executable_and_resource_helper_are_accepted() {
        let bundle = Path::new("/Applications/Uninstall MixRelay Driver.app");
        let executable = bundle.join("Contents/MacOS/mixrelay-driver-uninstaller");
        let helper = bundle.join("Contents/Resources/mixrelay-macos-installer");
        assert!(validate_bundle_layout(bundle, &executable, &helper).is_ok());
        assert!(validate_bundle_layout(bundle, &executable, Path::new("/tmp/helper")).is_err());
        assert!(validate_bundle_layout(bundle, Path::new("/tmp/gui"), &helper).is_err());
        assert!(validate_bundle_layout(Path::new("relative.app"), &executable, &helper).is_err());
    }

    #[test]
    fn authorization_only_treats_user_cancellation_as_harmless() {
        assert!(authorization_cancelled(Some(-128)));
        assert!(!authorization_cancelled(Some(1)));
        assert!(!authorization_cancelled(None));
        assert_eq!(
            authorization_script("'a\\b\"c'"),
            "do shell script \"'a\\\\b\\\"c'\" with administrator privileges"
        );
    }

    #[test]
    fn cancelled_or_failed_removal_restores_only_a_previously_running_manager() {
        assert!(manager_should_restart(true, false));
        assert!(!manager_should_restart(false, false));
        assert!(!manager_should_restart(true, true));
        assert!(!manager_should_restart(false, true));
    }
}
