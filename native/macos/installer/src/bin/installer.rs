//! Root-only install/remove entry point; never elevates itself or signs payloads.

#[cfg(target_os = "macos")]
#[path = "installer/manager_session.rs"]
mod manager_session;
#[cfg(any(target_os = "macos", test))]
#[path = "installer/session.rs"]
mod session;

fn main() {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    if let Err(error) = run() {
        tracing::error!("MixRelay installation failed: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn run() -> anyhow::Result<()> {
    anyhow::bail!("mixrelay-macos-installer is available only on macOS")
}

#[cfg(target_os = "macos")]
fn run() -> anyhow::Result<()> {
    use std::ffi::OsString;

    use mixrelay_macos_installer::{
        SigningTeam, TransactionRequest, cleanup_user_data, require_root, resolve_install_account,
        resolve_removal_account,
    };

    let arguments = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    if arguments.as_slice() == [OsString::from("--help")] {
        print_usage();
        return Ok(());
    }
    if arguments.as_slice() == [OsString::from("check-coreaudio")] {
        return mixrelay_macos_installer::check_coreaudio_response().map_err(Into::into);
    }
    if arguments.as_slice() == [OsString::from("probe-hal-absence")] {
        // A separate read-only process makes blocking CoreAudio calls cancellable.
        let loaded = mixrelay_macos_installer::hal_is_loaded()?;
        std::process::exit(if loaded { 2 } else { 0 });
    }
    for app in session::SessionApp::ALL {
        if arguments.as_slice() == [OsString::from(app.quit_action())] {
            return objc2::rc::autoreleasepool(|_| {
                println!("{}", manager_session::quit(app)?);
                Ok(())
            });
        }
        if arguments.as_slice() == [OsString::from(app.restore_action())] {
            return objc2::rc::autoreleasepool(|_| manager_session::restore(app));
        }
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "cleanup-user-data")
    {
        let (home, delete_settings, wait_for_commit) = parse_cleanup(&arguments)?;
        return cleanup_user_data(&home, delete_settings, wait_for_commit);
    }
    if let [command, team] = arguments.as_slice()
        && command == "removal-helper"
    {
        require_root()?;
        return mixrelay_macos_installer::removal::run_helper_listener(&SigningTeam::new(
            required_text(team, "Team ID")?,
        )?);
    }
    let (request, team, delete_user_data) = parse_request(&arguments)?;
    require_root()?;
    let team = SigningTeam::new(team)?;
    let account = match &request {
        TransactionRequest::Install(_) => resolve_install_account()?,
        TransactionRequest::Remove => resolve_removal_account()?,
    };
    run_transaction(&request, team, account, delete_user_data)
}

#[cfg(target_os = "macos")]
fn run_transaction(
    request: &mixrelay_macos_installer::TransactionRequest,
    team: mixrelay_macos_installer::SigningTeam,
    account: mixrelay_macos_installer::ResolvedServiceAccount,
    delete_user_data: bool,
) -> anyhow::Result<()> {
    use mixrelay_macos_installer::{
        CoordinatedBackend, FilesystemLayout, NativeServiceLifecycle, TransactionRequest, execute,
    };
    use session::{SessionApp, run_with_apps_closed};
    let session_account = account.clone();
    let layout = FilesystemLayout::new("/")?;
    let recovery_verifier = mixrelay_macos_installer::ReleasePayloadVerifier::new(
        team.clone(),
        account.account().clone(),
    );
    let install = matches!(request, TransactionRequest::Install(_));
    if let TransactionRequest::Install(sources) = request {
        // Reject invalid update inputs before interrupting either application.
        // The transaction verifies its private copies again before commit.
        recovery_verifier.verify_sources(sources)?;
    }
    let mut running = Vec::new();
    for app in SessionApp::ALL {
        // Removal closes only Driver; it never changes the graph App session.
        if (install || app == SessionApp::Driver) && app_running(&account, app)? {
            running.push(app);
        }
    }
    let service = NativeServiceLifecycle::new(team, account)
        .with_coreaudio_reload_approval(true)
        .with_saved_settings_removal(delete_user_data);
    let mut backend = CoordinatedBackend::new(layout, service);
    let may_relaunch_apps = std::cell::Cell::new(true);
    let outcome = run_with_apps_closed(
        &running,
        install,
        |action| user_session_action(&session_account, action),
        || {
            // Running apps hold exclusive lifecycle leases. Initialize/rebind
            // only after their acknowledged normal-user Quit has completed.
            mixrelay_macos_installer::initialize_lifecycle_storage(&session_account)?;
            execute(&mut backend, request).map_err(|error| {
                may_relaunch_apps.set(!matches!(
                    &error,
                    mixrelay_macos_installer::TransactionError::MutationRecovery {
                        recovery: mixrelay_macos_installer::RecoveryStatus::Failed { .. },
                        ..
                    } | mixrelay_macos_installer::TransactionError::Cleanup(_)
                ));
                error.into()
            })
        },
        || {
            may_relaunch_apps.get()
                && recovery_verifier
                    .verify_installed(
                        mixrelay_macos_installer::APP_PATH,
                        mixrelay_macos_installer::HAL_PATH,
                        mixrelay_macos_installer::DRIVER_PATH,
                        mixrelay_macos_installer::LAUNCHD_PATH,
                    )
                    .is_ok()
        },
    )?;
    tracing::info!(operation = ?outcome.operation(), state = ?outcome.state(),
        "MixRelay installation transaction and app session restoration completed");
    Ok(())
}

#[cfg(target_os = "macos")]
fn app_running(
    account: &mixrelay_macos_installer::ResolvedServiceAccount,
    app: session::SessionApp,
) -> anyhow::Result<bool> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "uid=,comm="])
        .output()?;
    anyhow::ensure!(output.status.success(), "could not inspect app owners");
    let expected_uid = account.uid().to_string();
    let executable = format!("{}/Contents/MacOS/mixrelay-desktop", app.bundle());
    Ok(String::from_utf8(output.stdout)?.lines().any(|line| {
        let line = line.trim();
        line.find(char::is_whitespace).is_some_and(|split| {
            line[..split] == expected_uid && line[split..].trim() == executable
        })
    }))
}

#[cfg(target_os = "macos")]
fn user_session_action(
    account: &mixrelay_macos_installer::ResolvedServiceAccount,
    action: &str,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        session::SessionApp::ALL
            .iter()
            .any(|app| action == app.quit_action() || action == app.restore_action()),
        "invalid session action"
    );
    let output = std::process::Command::new("/bin/launchctl")
        .args([
            "asuser",
            &account.uid().to_string(),
            "/usr/bin/sudo",
            "-n",
            "-u",
            &format!("#{}", account.uid()),
            "--",
        ])
        .arg(std::env::current_exe()?)
        .arg(action)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", account.account().home())
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "manager session action failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(any(target_os = "macos", test))]
fn parse_cleanup(
    arguments: &[std::ffi::OsString],
) -> anyhow::Result<(std::path::PathBuf, bool, bool)> {
    let (home, settings, wait_for_commit) = match arguments {
        [command, home, settings] if command == "cleanup-user-data" => (home, settings, false),
        [command, home, settings, wait]
            if command == "cleanup-user-data" && wait == "--wait-for-commit" =>
        {
            (home, settings, true)
        }
        _ => anyhow::bail!("invalid cleanup arguments"),
    };
    let delete_settings = match settings.to_str() {
        Some("--delete-user-data") => true,
        Some("--preserve-user-data") => false,
        _ => anyhow::bail!("invalid cleanup settings selection"),
    };
    Ok((
        std::path::PathBuf::from(home),
        delete_settings,
        wait_for_commit,
    ))
}

#[cfg(any(target_os = "macos", test))]
fn parse_request(
    arguments: &[std::ffi::OsString],
) -> anyhow::Result<(mixrelay_macos_installer::TransactionRequest, String, bool)> {
    use mixrelay_macos_installer::{InstallSources, TransactionRequest};
    use std::path::PathBuf;

    let Some(command) = arguments.first().and_then(|value| value.to_str()) else {
        anyhow::bail!("expected install or remove");
    };
    let request_and_team = match command {
        "install" if arguments.len() == 9 && arguments[8] == "--reload-coreaudio" => {
            let team = required_text(&arguments[1], "Team ID")?;
            let sources = InstallSources::new(
                PathBuf::from(&arguments[2]),
                PathBuf::from(&arguments[3]),
                PathBuf::from(&arguments[4]),
                PathBuf::from(&arguments[5]),
            )
            .with_remover(PathBuf::from(&arguments[6]), PathBuf::from(&arguments[7]));
            (TransactionRequest::install(sources), team)
        }
        "remove"
            if (arguments.len() == 3 && arguments[2] == "--reload-coreaudio")
                || (arguments.len() == 4
                    && arguments[2] == "--reload-coreaudio"
                    && arguments[3] == "--delete-user-data") =>
        {
            let team = required_text(&arguments[1], "Team ID")?;
            (TransactionRequest::remove(), team)
        }
        _ => {
            anyhow::bail!(
                "install/remove require explicit --reload-coreaudio approval and valid arguments"
            );
        }
    };
    let delete_user_data = command == "remove" && arguments.len() == 4;
    Ok((request_and_team.0, request_and_team.1, delete_user_data))
}

#[cfg(any(target_os = "macos", test))]
fn required_text(argument: &std::ffi::OsStr, label: &str) -> anyhow::Result<String> {
    argument
        .to_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{label} must be nonempty UTF-8"))
}

#[cfg(target_os = "macos")]
fn print_usage() {
    eprintln!(
        "Usage:\n  mixrelay-macos-installer install TEAM_ID MANAGER.app HAL.driver DRIVER PLIST_TEMPLATE UNINSTALLER.app REMOVAL_PLIST --reload-coreaudio\n  mixrelay-macos-installer remove TEAM_ID --reload-coreaudio [--delete-user-data]\n\nInstall/remove require an already-authorized root process and explicit approval to reload CoreAudio, interrupting active audio without rebooting the Mac. Removal optionally clears Driver device settings as the verified installation account. Updates preserve all user data. The authorized root command drops to the installed user for manager actions. It never elevates itself, signs, or notarizes code."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_macos_installer::TransactionRequest;
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn cleanup_accepts_only_the_declared_optional_commit_wait() {
        for selected in ["--delete-user-data", "--preserve-user-data"] {
            let direct = arguments(&["cleanup-user-data", "/Users/test", selected]);
            assert!(!parse_cleanup(&direct).expect("direct cleanup").2);
            let mut waiting = direct;
            waiting.push(OsString::from("--wait-for-commit"));
            assert!(parse_cleanup(&waiting).expect("prepared cleanup").2);
            waiting.push(OsString::from("extra"));
            assert!(parse_cleanup(&waiting).is_err());
        }
        assert!(
            parse_cleanup(&arguments(&[
                "cleanup-user-data",
                "/Users/test",
                "--wait-for-commit",
                "--delete-user-data"
            ]))
            .is_err()
        );
    }

    #[test]
    fn install_and_remove_require_explicit_reload_approval_in_the_declared_position() {
        let install = [
            "install",
            "TEAM",
            "Manager.app",
            "HAL.driver",
            "Driver",
            "template.plist",
            "Uninstaller.app",
            "removal.plist",
        ];
        assert!(parse_request(&arguments(&install)).is_err());
        let mut approved = arguments(&install);
        approved.push(OsString::from("--reload-coreaudio"));
        assert!(matches!(
            parse_request(&approved).unwrap().0,
            TransactionRequest::Install(_)
        ));
        for invalid in [
            vec!["remove", "TEAM"],
            vec!["remove", "TEAM", "--delete-user-data"],
            vec!["remove", "TEAM", "--delete-user-data", "--reload-coreaudio"],
        ] {
            assert!(parse_request(&arguments(&invalid)).is_err());
        }
        assert!(
            !parse_request(&arguments(&["remove", "TEAM", "--reload-coreaudio"]))
                .unwrap()
                .2
        );
        assert!(
            parse_request(&arguments(&[
                "remove",
                "TEAM",
                "--reload-coreaudio",
                "--delete-user-data"
            ]))
            .unwrap()
            .2
        );
    }
}
