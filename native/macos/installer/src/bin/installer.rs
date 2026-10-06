//! Root-only install/remove entry point; never elevates itself or signs payloads.

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
        CoordinatedBackend, FilesystemLayout, NativeServiceLifecycle, SigningTeam,
        TransactionRequest, cleanup_user_data, execute, initialize_lifecycle_storage,
        require_lifecycle_storage, require_root, resolve_install_account, resolve_removal_account,
    };

    let arguments = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    if arguments.as_slice() == [OsString::from("--help")] {
        print_usage();
        return Ok(());
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "cleanup-user-data")
    {
        let (home, delete_settings, wait_for_commit) = parse_cleanup(&arguments)?;
        return cleanup_user_data(&home, delete_settings, wait_for_commit);
    }
    let (request, team, delete_user_data) = parse_request(&arguments)?;
    require_root()?;
    let team = SigningTeam::new(team)?;
    let account = match &request {
        TransactionRequest::Install(_) => {
            let account = resolve_install_account()?;
            // Explicit audio-reload consent was validated before creating the
            // root-owned lease directory/inodes on first install.
            initialize_lifecycle_storage(&account)?;
            account
        }
        TransactionRequest::Remove => {
            let account = resolve_removal_account()?;
            require_lifecycle_storage(&account)?;
            account
        }
    };
    let layout = FilesystemLayout::new("/")?;
    let service = NativeServiceLifecycle::new(team, account)
        .with_coreaudio_reload_approval(true)
        .with_removal_data_cleanup(delete_user_data);
    let mut backend = CoordinatedBackend::new(layout, service);
    let outcome = execute(&mut backend, &request)?;
    tracing::info!(operation = ?outcome.operation(), state = ?outcome.state(),
        "MixRelay installation transaction completed");
    Ok(())
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
        "install" if arguments.len() == 7 && arguments[6] == "--reload-coreaudio" => {
            let team = required_text(&arguments[1], "Team ID")?;
            let sources = InstallSources::new(
                PathBuf::from(&arguments[2]),
                PathBuf::from(&arguments[3]),
                PathBuf::from(&arguments[4]),
                PathBuf::from(&arguments[5]),
            );
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
        "Usage:\n  mixrelay-macos-installer install TEAM_ID MANAGER.app HAL.driver DRIVER PLIST_TEMPLATE --reload-coreaudio\n  mixrelay-macos-installer remove TEAM_ID --reload-coreaudio [--delete-user-data]\n\nInstall/remove require an already-authorized root process and explicit approval to reload CoreAudio, interrupting active audio without rebooting the Mac. Removal optionally clears Driver device settings as the verified installation account. App graphs, recordings and paid authentication are retained. Updates preserve all user data. This command never invokes sudo, signs, or notarizes code."
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
