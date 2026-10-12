//! Fixed `MixRelay` app actions performed in its normal user's GUI session.

use std::os::unix::process::CommandExt as _;
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::{NSDate, NSRunLoop, NSString};

use super::session::SessionApp;

pub(super) fn quit(app: SessionApp) -> Result<bool> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "app actions require a normal user"
    );
    let applications = matching_applications(app)?;
    for application in &applications {
        if application.isTerminated() {
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
        ensure!(
            owner.status.success()
                && String::from_utf8(owner.stdout)?.trim()
                    == unsafe { libc::geteuid() }.to_string(),
            "app belongs to a different user"
        );
        let path = application.bundleURL().and_then(|url| url.path());
        ensure!(
            path.is_some_and(|path| path.to_string() == app.bundle()),
            "unexpected app bundle"
        );
    }
    let was_running = applications
        .iter()
        .any(|application| !application.isTerminated());
    // Validate every target before changing any running app.
    for application in &applications {
        ensure!(
            application.isTerminated() || application.terminate(),
            "app refused Quit"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(130);
    while applications
        .iter()
        .any(|application| !application.isTerminated())
    {
        ensure!(Instant::now() < deadline, "app did not complete Quit");
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    Ok(was_running)
}

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
