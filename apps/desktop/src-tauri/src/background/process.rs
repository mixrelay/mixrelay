//! Controller and editor child lifetimes remain independent of window ownership.
use std::{
    os::unix::process::CommandExt as _,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) struct ControllerLaunch {
    exited: Arc<AtomicBool>,
}

impl ControllerLaunch {
    pub(super) fn exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }
}

pub(crate) fn spawn_controller(executable: &std::path::Path) -> std::io::Result<ControllerLaunch> {
    #[cfg(target_os = "macos")]
    if !tauri::is_dev()
        && executable
            == std::path::Path::new(mixrelay_macos_transport::install_contract::APP_EXECUTABLE_PATH)
    {
        let launcher = std::env::current_exe()?
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or_else(|| std::io::Error::other("installed app bundle unavailable"))?
            .join("Resources/mixrelay-driver-launcher");
        return spawn_reaped_controller(&launcher, false);
    }
    spawn_reaped_controller(executable, true)
}

fn spawn_reaped_controller(
    executable: &std::path::Path,
    runtime_argument: bool,
) -> std::io::Result<ControllerLaunch> {
    let mut command = std::process::Command::new(executable);
    if runtime_argument {
        command.arg("--driver-runtime");
    }
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let id = child.id();
    let exited = Arc::new(AtomicBool::new(false));
    let observed_exit = Arc::clone(&exited);
    // There is no editor-owned shutdown pipe. Reaping never changes controller lifetime.
    std::thread::Builder::new()
        .name("mixrelay-controller-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) => {
                observed_exit.store(true, Ordering::Release);
                tracing::info!(
                    pid = id,
                    code = status.code(),
                    "launched Driver runtime exited"
                );
            }
            Err(error) => {
                tracing::warn!(%error, pid = id, "could not observe launched Driver runtime exit");
            }
        })?;
    Ok(ControllerLaunch { exited })
}

pub(crate) fn spawn_editor(executable: &std::path::Path) -> std::io::Result<()> {
    let mut child = std::process::Command::new(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    std::thread::Builder::new()
        .name("mixrelay-editor-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(test)]
mod tests;
