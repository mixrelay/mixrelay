//! Graph app installation reuses acknowledged normal-user Quit and relaunch.
use super::{app_running, prepare_graph_logs, required_text, session, user_session_action};
use mixrelay_macos_installer::{SigningTeam, require_root, resolve_removal_account};
use std::{ffi::OsString, path::Path};

pub(super) fn handle_command(arguments: &[OsString]) -> Option<anyhow::Result<()>> {
    if arguments == [OsString::from("prepare-graph-logs")] {
        return Some(create_logs());
    }
    if let [command, team, app] = arguments
        && command == "install-editor"
    {
        return Some(install(required_text(team, "Team ID"), Path::new(app)));
    }
    None
}

fn create_logs() -> anyhow::Result<()> {
    // SAFETY: geteuid only queries this process identity.
    anyhow::ensure!(
        unsafe { libc::geteuid() } != 0,
        "app logs must be created as the service user"
    );
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("missing service home"))?;
    let directory = std::path::PathBuf::from(home).join("Library/Logs/MixRelay");
    std::fs::create_dir_all(&directory)?;
    Ok(())
}

fn install(team: anyhow::Result<String>, app: &Path) -> anyhow::Result<()> {
    require_root()?;
    let team = SigningTeam::new(team?)?;
    let account = resolve_removal_account()?;
    let verifier = mixrelay_macos_installer::ReleasePayloadVerifier::new(
        team.clone(),
        account.account().clone(),
    );
    verifier.verify_graph_bundle(app)?;
    let mut running = Vec::new();
    for app in [session::SessionApp::Graph] {
        if app_running(&account, app)? {
            running.push(app);
        }
    }
    let recovery_team = team.clone();
    session::run_with_apps_closed(
        &running,
        true,
        |action| user_session_action(&account, action),
        || {
            mixrelay_macos_installer::initialize_editor_lifecycle_storage(&account)?;
            prepare_graph_logs(&account)?;
            mixrelay_macos_installer::install_graph_app(team, &account, app)
        },
        || mixrelay_macos_installer::graph_app_ready(recovery_team, &account).unwrap_or(false),
    )
}
