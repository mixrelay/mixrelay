//! Fixed `MixRelay` app actions performed in its normal user's GUI session.

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::{NSDate, NSRunLoop, NSString};

use super::session::SessionApp;

pub(super) fn quit(app: SessionApp) -> Result<bool> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "app actions require a normal user"
    );
    let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(app.identifier()),
    );
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
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-a", app.bundle()])
        .status()?;
    ensure!(status.success(), "could not restore app");
    // LaunchServices accepting a request is not proof that the app launched.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(app.identifier()),
        );
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
