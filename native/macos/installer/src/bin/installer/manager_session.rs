//! Fixed `MixRelay` app actions performed in its normal user's GUI session.

use std::os::unix::process::CommandExt as _;
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, ensure};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::{NSDate, NSRunLoop, NSString};

use mixrelay_macos_installer::process_exit::{ProcessIdentity, wait_for_exit};

use super::session::SessionApp;

pub(super) fn quit(app: SessionApp) -> Result<bool> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "app actions require a normal user"
    );
    let applications = matching_applications(app)?;
    // Identify each process by its kernel audit token first; the exit is judged from
    // that identity, not from `isTerminated`, which a command-line process may never
    // see change. The PID-based checks below are bound to the identity by re-checking
    // it afterwards, and an app that exits meanwhile is skipped.
    let role = app.quit_action();
    let mut targets = Vec::new();
    for application in &applications {
        let pid = application.processIdentifier();
        let Some(process) = ProcessIdentity::capture(pid)
            .with_context(|| format!("{role}: could not identify pid {pid}"))?
        else {
            continue;
        };
        let owner = std::process::Command::new("/bin/ps")
            .args(["-ww", "-p", &pid.to_string(), "-o", "uid=", "-o", "args="])
            .output()?;
        let observed = String::from_utf8(owner.stdout)?;
        let (uid, command) = observed.trim().split_once(' ').unwrap_or_default();
        let path = application.bundleURL().and_then(|url| url.path());
        if process
            .has_exited()
            .with_context(|| format!("{role}: pid {pid}"))?
        {
            continue;
        }
        ensure!(
            owner.status.success() && uid == unsafe { libc::geteuid() }.to_string(),
            "{role}: app belongs to a different user (pid {pid})"
        );
        ensure!(
            app.matches_command(command.trim())
                && path.is_some_and(|path| path.to_string() == app.bundle()),
            "{role}: unexpected app process (pid {pid})"
        );
        targets.push((application, process));
    }
    let was_running = !targets.is_empty();
    for (application, process) in &targets {
        // `terminate` reports false for an app that exited after it was identified.
        ensure!(
            application.terminate()
                || process
                    .has_exited()
                    .with_context(|| format!("{role}: pid {}", process.pid()))?,
            "{role} refused Quit (pid {})",
            process.pid()
        );
    }
    let processes: Vec<_> = targets.iter().map(|(_, process)| *process).collect();
    let pump = |interval: Duration| {
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(
            interval.as_secs_f64(),
        ));
    };
    // The runtime stops audio, finalizes recordings and waits for the Driver before it
    // exits, so a normal Quit gets most of the budget.
    let started = Instant::now();
    let remaining = wait_for_exit(&processes, started + QUIT_GRACE, pump)
        .with_context(|| format!("{role}: waiting for Quit"))?;
    if remaining.is_empty() {
        return Ok(was_running);
    }
    // An app that did not act on the Quit event (for example a busy main thread) gets
    // SIGTERM through its audit token; the runtime routes it through the same
    // acknowledged shutdown, and the editor owns no audio state.
    for process in &remaining {
        process
            .terminate()
            .with_context(|| format!("{role}: SIGTERM to pid {}", process.pid()))?;
    }
    let remaining = wait_for_exit(&remaining, started + QUIT_BUDGET, pump)
        .with_context(|| format!("{role}: waiting for SIGTERM"))?;
    ensure!(
        remaining.is_empty(),
        "{role} did not complete Quit (pid {})",
        remaining
            .iter()
            .map(|process| process.pid().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(was_running)
}

/// Time a normal Quit may take before the installer escalates to `SIGTERM`.
const QUIT_GRACE: Duration = Duration::from_secs(100);
/// The whole Quit budget, unchanged from the previous single wait.
const QUIT_BUDGET: Duration = Duration::from_secs(130);

pub(super) fn restore(app: SessionApp) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "app actions require a normal user"
    );
    let app = if matches!(
        app,
        SessionApp::Driver | SessionApp::Service | SessionApp::LegacyService
    ) && !std::path::Path::new(SessionApp::Driver.bundle()).exists()
    {
        // Commit retired the split manager. Rollback restores it before reaching
        // this branch, so it resumes the original role on the failure path.
        SessionApp::UnifiedRuntime
    } else if app == SessionApp::LegacyService {
        // Successful ownership migration resumes audio in Driver; rollback retains its old owner.
        let version = super::bundle_version(std::path::Path::new(SessionApp::Service.bundle()));
        if version.is_ok_and(|version| {
            mixrelay_macos_installer::optional_driver_install_needed("0.1.2", &[version], true)
                .is_ok_and(|needed| !needed)
        }) {
            SessionApp::Service
        } else {
            app
        }
    } else {
        app
    };
    if matches!(
        app,
        SessionApp::Graph
            | SessionApp::Service
            | SessionApp::LegacyService
            | SessionApp::UnifiedRuntime
    ) {
        let mut command =
            std::process::Command::new(format!("{}/Contents/MacOS/mixrelay-desktop", app.bundle()));
        if matches!(app, SessionApp::Service | SessionApp::UnifiedRuntime) {
            command.arg("--driver-runtime");
        }
        if app == SessionApp::LegacyService {
            command.arg("--background-service");
        }
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    } else {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-a", app.bundle()])
            .status()?;
        ensure!(status.success(), "could not restore app");
    }
    // Accepting a launch request is not proof that the requested role launched.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let applications = matching_applications(app)?;
        for application in &applications {
            if application.isTerminated() || !application.isFinishedLaunching() {
                continue;
            }
            let path = application.bundleURL().and_then(|url| url.path());
            if path.is_none_or(|path| path.to_string() != app.bundle()) {
                continue;
            }
            let owner = std::process::Command::new("/bin/ps")
                .args([
                    "-p",
                    &application.processIdentifier().to_string(),
                    "-o",
                    "uid=",
                ])
                .output()?;
            if owner.status.success()
                && String::from_utf8(owner.stdout)?.trim() == unsafe { libc::geteuid() }.to_string()
                && !application.isTerminated()
            {
                return Ok(());
            }
        }
        ensure!(Instant::now() < deadline, "app relaunch was not observed");
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
}

fn matching_applications(
    app: SessionApp,
) -> Result<Vec<objc2::rc::Retained<NSRunningApplication>>> {
    let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(app.identifier()),
    );
    let mut matching = Vec::new();
    for application in &applications {
        if application.isTerminated() {
            continue;
        }
        let output = std::process::Command::new("/bin/ps")
            .args([
                "-ww",
                "-p",
                &application.processIdentifier().to_string(),
                "-o",
                "args=",
            ])
            .output()?;
        if output.status.success() && app.matches_command(String::from_utf8(output.stdout)?.trim())
        {
            matching.push(application.clone());
        }
    }
    Ok(matching)
}
