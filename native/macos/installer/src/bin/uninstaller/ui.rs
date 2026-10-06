use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, ensure};
use mixrelay_macos_installer::{
    APP_PATH, DRIVER_PATH, HAL_PATH, LAUNCHD_PATH, ReleasePayloadVerifier, SigningTeam,
    resolve_console_account,
};
use objc2::{
    AnyThread, MainThreadMarker, MainThreadOnly, msg_send, rc::Retained, runtime::AnyObject,
};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSAlertStyle, NSApplication, NSApplicationActivationPolicy,
    NSButton, NSButtonType, NSControlStateValueOff, NSControlStateValueOn, NSRunningApplication,
};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleScript, NSBundle, NSDate, NSDictionary, NSLocale, NSNumber,
    NSPoint, NSRect, NSRunLoop, NSSize, NSString,
};

use super::{
    authorization_cancelled, authorization_script, manager_should_restart, removal_command,
    validate_bundle_layout,
};

#[derive(Clone, Copy)]
struct NativeUi {
    main: MainThreadMarker,
    korean: bool,
}

impl NativeUi {
    fn text(self, korean: &'static str, english: &'static str) -> &'static str {
        if self.korean { korean } else { english }
    }
}

/// Presents removal consent and diagnostics on the process's main thread.
pub(super) fn run() -> Result<()> {
    let main =
        MainThreadMarker::new().ok_or_else(|| anyhow!("AppKit requires the main thread."))?;
    let ui = NativeUi {
        main,
        korean: NSLocale::preferredLanguages()
            .iter()
            .next()
            .is_some_and(|language| language.to_string().starts_with("ko")),
    };
    let application = NSApplication::sharedApplication(main);
    if application.activationPolicy() != NSApplicationActivationPolicy::Regular {
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    }
    ensure!(
        application.activationPolicy() == NSApplicationActivationPolicy::Regular,
        "Could not activate the uninstaller."
    );
    application.finishLaunching();
    application.activate();
    let result = remove_driver(ui);
    if let Err(error) = &result {
        tracing::error!("Driver removal failed: {error:#}");
        message(
            ui,
            ui.text(
                "MixRelay Driver를 제거하지 못했습니다.",
                "MixRelay Driver could not be removed.",
            ),
            &bounded_detail(&format!("{error:#}")),
            NSAlertStyle::Warning,
        );
    }
    result
}

fn remove_driver(ui: NativeUi) -> Result<()> {
    let Some(delete_settings) = confirm_removal(ui) else {
        return Ok(());
    };
    if !driver_is_installed()? {
        message(
            ui,
            ui.text(
                "MixRelay Driver가 설치되어 있지 않습니다.",
                "MixRelay Driver is not installed.",
            ),
            ui.text("제거할 항목이 없습니다.", "There is nothing to remove."),
            NSAlertStyle::Informational,
        );
        return Ok(());
    }
    ensure!(
        running_applications("io.mixrelay.desktop").is_empty(),
        "{}", ui.text(
            "MixRelay에서 작업을 저장하고 앱을 종료한 뒤 제거 앱을 다시 실행하십시오.",
            "Quit MixRelay before removing its Driver. Save your work and run the uninstaller again."
        )
    );
    let (bundle, helper, team) = bundled_helper()?;
    // Signature policy needs only an OS-resolved identity; the authorized CLI
    // separately resolves the installed service account under its root boundary.
    let account = resolve_console_account()?;
    let verifier =
        ReleasePayloadVerifier::new(SigningTeam::new(team.clone())?, account.account().clone());
    verifier
        .verify_uninstaller_bundle(&bundle)
        .context("The uninstaller's signature could not be verified. Download it again.")?;
    verifier
        .verify_installer_executable(&helper)
        .context("The bundled removal tool's signature could not be verified.")?;
    let command = removal_command(&helper, &team, delete_settings)?;
    if remove_with_manager(ui, &command)? {
        message(
            ui,
            ui.text(
                "MixRelay Driver를 제거했습니다.",
                "MixRelay Driver removed.",
            ),
            ui.text(
                "MixRelay 그래프와 녹음 파일은 그대로 유지됩니다.",
                "Your MixRelay graphs and recordings are unchanged.",
            ),
            NSAlertStyle::Informational,
        );
    }
    Ok(())
}

fn remove_with_manager(ui: NativeUi, command: &str) -> Result<bool> {
    let was_running = !running_applications("io.mixrelay.driver.desktop").is_empty();
    let removal = quit_driver_manager(ui).and_then(|()| authorize_removal(command));
    if manager_should_restart(was_running, removal.as_ref().is_ok_and(|removed| *removed))
        && let Err(restoration) = restore_driver_manager(ui)
    {
        return match removal {
            Ok(_) => Err(restoration),
            Err(error) => Err(error.context(format!("{restoration:#}"))),
        };
    }
    removal
}

fn restore_driver_manager(ui: NativeUi) -> Result<()> {
    match fs::symlink_metadata(APP_PATH) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("Could not locate MixRelay Driver Manager."),
    }
    let diagnostic = ui.text(
        "MixRelay Driver 관리 앱을 다시 열지 못했습니다. 응용 프로그램에서 직접 실행하십시오.",
        "Could not reopen MixRelay Driver Manager. Open it from Applications.",
    );
    let status = Command::new("/usr/bin/open")
        .args(["-a", APP_PATH])
        .status()
        .context(diagnostic)?;
    ensure!(status.success(), "{diagnostic} ({status})");
    Ok(())
}

fn driver_is_installed() -> Result<bool> {
    for path in [APP_PATH, HAL_PATH, DRIVER_PATH, LAUNCHD_PATH] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Could not inspect the Driver installation."),
        }
    }
    Ok(false)
}

fn confirm_removal(ui: NativeUi) -> Option<bool> {
    let alert = NSAlert::new(ui.main);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(ui.text(
        "MixRelay Driver를 제거하시겠습니까?",
        "Remove MixRelay Driver?",
    )));
    alert.setInformativeText(&NSString::from_str(ui.text(
        "Driver 장치와 루프백이 중지되며 시스템 오디오가 잠시 끊깁니다. \
         계속하기 전에 녹음과 통화 앱을 종료하십시오.\n\n\
         MixRelay 그래프, 녹음 파일과 구매 인증은 그대로 유지됩니다.",
        "Removing the Driver stops its devices and loopback, and briefly interrupts system audio. \
         Quit recording and call apps before continuing.\n\n\
         Your MixRelay graphs, recordings, and purchase activation are unchanged.",
    )));
    let cancel = alert.addButtonWithTitle(&NSString::from_str(ui.text("취소", "Cancel")));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    alert.addButtonWithTitle(&NSString::from_str(ui.text("Driver 제거", "Remove Driver")));
    let checkbox = NSButton::initWithFrame(
        NSButton::alloc(ui.main),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(340.0, 24.0)),
    );
    checkbox.setButtonType(NSButtonType::Switch);
    checkbox.setTitle(&NSString::from_str(
        ui.text("저장된 Driver 설정 삭제", "Delete saved Driver settings"),
    ));
    checkbox.setState(NSControlStateValueOff);
    alert.setAccessoryView(Some(&checkbox));
    (alert.runModal() == NSAlertSecondButtonReturn)
        .then(|| checkbox.state() == NSControlStateValueOn)
}

fn bundled_helper() -> Result<(PathBuf, PathBuf, String)> {
    let bundle = NSBundle::mainBundle();
    let bundle_path = fs::canonicalize(bundle.bundlePath().to_string())
        .context("Could not locate the uninstaller bundle.")?;
    let executable = fs::canonicalize(std::env::current_exe()?)?;
    let helper = fs::canonicalize(bundle_path.join("Contents/Resources/mixrelay-macos-installer"))
        .context("The bundled removal tool is missing.")?;
    validate_bundle_layout(&bundle_path, &executable, &helper)?;
    let team = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("MixRelaySigningTeam"))
        .and_then(|value| value.downcast::<NSString>().ok())
        .ok_or_else(|| anyhow!("The uninstaller's publisher identity is missing."))?
        .to_string();
    Ok((bundle_path, helper, team))
}

fn running_applications(
    identifier: &str,
) -> Retained<objc2_foundation::NSArray<NSRunningApplication>> {
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(identifier))
}

fn quit_driver_manager(ui: NativeUi) -> Result<()> {
    let applications = running_applications("io.mixrelay.driver.desktop");
    for application in &applications {
        ensure!(
            application.isTerminated() || application.terminate(),
            "{}",
            ui.text(
                "MixRelay Driver 관리 앱을 종료한 뒤 제거 앱을 다시 실행하십시오.",
                "Quit MixRelay Driver Manager and run the uninstaller again."
            )
        );
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while applications
        .iter()
        .any(|application| !application.isTerminated())
    {
        ensure!(
            Instant::now() < deadline,
            "{}", ui.text(
                "MixRelay Driver 관리 앱이 아직 실행 중입니다. 종료한 뒤 제거 앱을 다시 실행하십시오.",
                "MixRelay Driver Manager is still running. Quit it and run the uninstaller again."
            )
        );
        // AppKit updates NSRunningApplication termination state when its run loop advances.
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    Ok(())
}

fn authorize_removal(command: &str) -> Result<bool> {
    let source = NSString::from_str(&authorization_script(command));
    let script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
        .ok_or_else(|| anyhow!("Could not prepare administrator authorization."))?;
    let mut error: Option<Retained<NSDictionary<NSString, AnyObject>>> = None;
    // SAFETY: NSAppleScript documents this out parameter as NSDictionary<NSString, id>.
    // The local retained dictionary slot remains valid for this synchronous call. The
    // nullable descriptor return matches Apple's API, including authorization errors.
    let descriptor: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&script, executeAndReturnError: &mut error] };
    if let Some(error) = error {
        let number = error
            .objectForKey(&NSString::from_str("NSAppleScriptErrorNumber"))
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .map(|value| value.intValue());
        if authorization_cancelled(number) {
            return Ok(false);
        }
        let detail = error
            .objectForKey(&NSString::from_str("NSAppleScriptErrorMessage"))
            .and_then(|value| value.downcast::<NSString>().ok())
            .map_or_else(
                || "Administrator authorization failed.".to_owned(),
                |value| value.to_string(),
            );
        anyhow::bail!("{}", bounded_detail(&detail));
    }
    ensure!(descriptor.is_some(), "The removal tool returned no result.");
    Ok(true)
}

fn message(ui: NativeUi, title: &str, detail: &str, style: NSAlertStyle) {
    let alert = NSAlert::new(ui.main);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str(ui.text("확인", "OK")));
    alert.runModal();
}

fn bounded_detail(detail: &str) -> String {
    detail.chars().take(600).collect()
}
