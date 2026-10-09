//! One-operation authorization and mutually authenticated XPC for the installed remover.
//!
//! The UI remains a normal user process. Only the PKG-installed root helper runs
//! the existing fixed-path removal transaction. It accepts no path, command or UID.

use std::{
    ffi::{CString, c_char, c_int, c_void},
    fs,
    os::unix::fs::MetadataExt,
    path::Path,
    process::Command,
    ptr::NonNull,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    REMOVAL_LAUNCHD_PATH, ReleasePayloadVerifier, ServiceAccount, SigningTeam, UNINSTALLER_PATH,
    require_root,
};
use anyhow::{Context, Result, ensure};

/// Fixed system Mach service used only for authenticated Driver removal.
pub const SERVICE: &str = "io.mixrelay.driver.removal";
/// Root-owned CLI executable embedded in the separately installed remover.
pub const HELPER: &str =
    "/Applications/Uninstall MixRelay Driver.app/Contents/Helpers/mixrelay-macos-installer";

mod executable;
use executable::RemovalExecutable;

static LISTENER_EXECUTABLE: Mutex<Option<RemovalExecutable>> = Mutex::new(None);
static LISTENER_SIGNING_TEAM: Mutex<Option<SigningTeam>> = Mutex::new(None);

unsafe extern "C" {
    fn MRRemovalRightRegister() -> c_int;
    fn MRRemovalRightRemove() -> c_int;
    fn MRRemovalAuthorize(prompt: *const c_char, output: *mut *mut c_void) -> c_int;
    fn MRRemovalAuthorizationFree(reference: *mut c_void);
    fn MRRemovalRequest(
        reference: *mut c_void,
        team: *const c_char,
        delete_settings: bool,
    ) -> c_int;
    fn MRRemovalServer(
        team: *const c_char,
        remove: extern "C" fn(bool) -> c_int,
        finish: extern "C" fn(),
        idle: extern "C" fn(),
    ) -> c_int;
}

/// An authorization for the named removal right, destroyed when its request finishes.
pub struct RemovalAuthorization(NonNull<c_void>);

// SAFETY: Authorization Services references are thread-safe. Ownership moves to
// one worker; the reference is not cloned or accessed concurrently.
unsafe impl Send for RemovalAuthorization {}

impl RemovalAuthorization {
    /// Runs system authentication. Cancellation returns no authorization and changes no Driver state.
    /// # Errors
    /// Returns an OS authorization failure or invalid prompt.
    pub fn request(prompt: &str) -> Result<Option<Self>> {
        let prompt = CString::new(prompt)?;
        let mut reference = std::ptr::null_mut();
        // SAFETY: both pointers are valid for the synchronous call; C creates an owned reference.
        let status = unsafe { MRRemovalAuthorize(prompt.as_ptr(), &raw mut reference) };
        if status == -60006 {
            return Ok(None);
        }
        ensure!(status == 0, "removal authorization failed: {status}");
        Ok(Some(Self(
            NonNull::new(reference).context("missing removal authorization")?,
        )))
    }

    /// Sends only the authorization and settings Bool over mutually signature-checked XPC.
    /// # Errors
    /// Returns helper rejection, IPC failure, timeout, or incomplete helper retirement.
    pub fn remove(self, team: &SigningTeam, delete_settings: bool) -> Result<()> {
        let team = CString::new(team.as_str())?;
        // SAFETY: the reference remains owned until this bounded call returns; C copies its blob.
        let result = unsafe { MRRemovalRequest(self.0.as_ptr(), team.as_ptr(), delete_settings) };
        ensure!(
            result == 0,
            "authenticated removal helper did not report committed success"
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while helper_is_registered()? {
            ensure!(
                Instant::now() < deadline,
                "removal helper registration remained after commit"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        for target in crate::OwnedTarget::all() {
            ensure!(
                fs::symlink_metadata(target.path())
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "owned target remains after removal"
            );
        }
        Ok(())
    }
}

impl Drop for RemovalAuthorization {
    fn drop(&mut self) {
        // SAFETY: uniquely owned native reference, freed exactly once.
        unsafe {
            MRRemovalAuthorizationFree(self.0.as_ptr());
        }
    }
}

/// Validates the exact fixed root helper configuration; it cannot supply arbitrary argv.
/// # Errors
/// Returns malformed, oversized or out-of-contract configuration data.
pub fn verify_removal_launchd(path: &Path, team: &SigningTeam) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.len() <= 16 * 1024,
        "invalid removal helper plist"
    );
    let actual: plist::Value = plist::from_bytes(&fs::read(path)?)?;
    let expected: plist::Value = plist::from_bytes(removal_launchd_template(team).as_bytes())?;
    ensure!(
        actual == expected,
        "removal helper plist has an unexpected operation or destination"
    );
    Ok(())
}

/// Returns the closed helper plist for one validated signing team.
#[must_use]
pub fn removal_launchd_template(team: &SigningTeam) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{SERVICE}</string>
<key>ProgramArguments</key><array><string>{HELPER}</string><string>removal-helper</string><string>{}</string></array>
<key>MachServices</key><dict><key>{SERVICE}</key><true/></dict>
<key>AssociatedBundleIdentifiers</key><array><string>io.mixrelay.driver.uninstaller</string></array>
</dict></plist>
"#,
        team.as_str()
    )
}

/// Installs the operation's authorization policy and bootstraps its idle on-demand job.
/// # Errors
/// Returns root/trust, policy registration or launchd failure.
pub fn register_helper(team: &SigningTeam) -> Result<()> {
    require_root()?;
    verify_installed_remover(team)?;
    // SAFETY: no caller pointers; this installs only the fixed right in the native shim.
    let status = unsafe { MRRemovalRightRegister() };
    ensure!(
        status == 0,
        "could not register removal operation authorization: {status}"
    );
    if !helper_is_registered()? {
        let status = Command::new("/bin/launchctl")
            .args(["bootstrap", "system", REMOVAL_LAUNCHD_PATH])
            .status()?;
        ensure!(status.success(), "could not register removal helper");
    }
    Ok(())
}

/// Stops the fixed helper job before an installer replaces its executable.
/// # Errors
/// Returns root or launchd failure.
pub fn unregister_helper() -> Result<()> {
    require_root()?;
    if helper_is_registered()? {
        let status = Command::new("/bin/launchctl")
            .args(["bootout", &format!("system/{SERVICE}")])
            .status()?;
        ensure!(status.success(), "could not retire removal helper job");
    }
    Ok(())
}

/// Removes the named operation policy after a completed removal.
/// # Errors
/// Returns root or authorization database failure.
pub fn remove_right() -> Result<()> {
    require_root()?;
    // SAFETY: fixed authorization right only, no caller-provided identifier.
    let status = unsafe { MRRemovalRightRemove() };
    ensure!(
        status == 0,
        "could not retire removal authorization: {status}"
    );
    Ok(())
}

fn helper_is_registered() -> Result<bool> {
    let output = Command::new("/bin/launchctl")
        .args(["print", &format!("system/{SERVICE}")])
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("Could not find service"),
        "could not inspect removal helper registration"
    );
    Ok(false)
}

/// Verifies the root-owned installed app, embedded CLI and exact launchd definition.
/// # Errors
/// Returns any ownership, symlink, code-signing or fixed-path violation.
pub fn verify_installed_remover(team: &SigningTeam) -> Result<()> {
    validate_root_tree(Path::new(UNINSTALLER_PATH))?;
    validate_root_file(Path::new(REMOVAL_LAUNCHD_PATH), 0o644)?;
    crate::native::validate_destination_ancestors(Path::new("/"))?;
    ReleasePayloadVerifier::new(team.clone(), verification_account()?)
        .verify_uninstaller_bundle(UNINSTALLER_PATH)?;
    verify_removal_launchd(Path::new(REMOVAL_LAUNCHD_PATH), team)
}

/// Verifies remaining remover components independently for an idempotent removal.
/// # Errors
/// Returns unsafe ownership, present code signatures or launchd definitions.
pub fn verify_remaining_remover(team: &SigningTeam) -> Result<()> {
    crate::native::validate_destination_ancestors(Path::new("/"))?;
    for path in [UNINSTALLER_PATH, REMOVAL_LAUNCHD_PATH] {
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("inspect remaining remover component"),
        }
        if path == UNINSTALLER_PATH {
            validate_root_tree(Path::new(path))?;
            ReleasePayloadVerifier::new(team.clone(), verification_account()?)
                .verify_uninstaller_bundle(path)?;
        } else {
            validate_root_file(Path::new(path), 0o644)?;
            verify_removal_launchd(Path::new(path), team)?;
        }
    }
    Ok(())
}

fn validate_root_file(path: &Path, mode: u32) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.nlink() == 1
            && metadata.mode() & 0o777 == mode,
        "untrusted removal helper file"
    );
    Ok(())
}

fn validate_root_tree(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "untrusted removal helper bundle entry"
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            validate_root_tree(&entry?.path())?;
        }
    } else {
        ensure!(
            metadata.is_file() && metadata.nlink() == 1,
            "invalid removal bundle entry"
        );
    }
    Ok(())
}

/// Runs the fixed system helper. A valid named authorization is checked in C before callback.
/// # Errors
/// Returns an invalid helper location, private executable handoff or listener failure.
pub fn run_helper_listener(team: &SigningTeam) -> Result<()> {
    use std::os::unix::process::CommandExt;

    require_root()?;
    verify_remaining_remover(team)?;
    if std::env::current_exe()? == Path::new(HELPER) {
        // XPC verifies the sender's executable when delivering its reply. The
        // installed remover is removed by this transaction, so run this same
        // signed helper from a private copy until the reply has been accepted.
        // exec preserves the launchd job's PID and Mach-service ownership.
        let stage = RemovalExecutable::create_private_copy(team)?;
        let error = Command::new(&stage.executable)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("LC_ALL", "C")
            .args(["removal-helper", team.as_str()])
            .exec();
        return Err(error).context("could not hand off removal listener to private executable");
    }
    let stage = RemovalExecutable::from_current_process(team)?;
    *LISTENER_EXECUTABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stage);
    *LISTENER_SIGNING_TEAM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(team.clone());
    let team = CString::new(team.as_str())?;
    // SAFETY: C retains no Rust stack pointers beyond this non-returning dispatch loop;
    // callbacks are static, take a Bool only, and invoke a fixed verified executable.
    let status = unsafe {
        MRRemovalServer(
            team.as_ptr(),
            remove_callback,
            finish_callback,
            idle_callback,
        )
    };
    cleanup_listener_executable()?;
    anyhow::bail!("removal listener could not start: {status}")
}

fn cleanup_listener_executable() -> Result<()> {
    let stage = LISTENER_EXECUTABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(stage) = stage {
        stage.remove_private_copy()?;
    }
    Ok(())
}

extern "C" fn finish_callback() {
    if let Err(error) = cleanup_listener_executable() {
        // Keep the registration as failure evidence. The GUI's retirement gate
        // must not report success if the private executable was not cleaned.
        tracing::error!("private removal executable cleanup failed: {error:#}");
        std::process::exit(1);
    }
    if let Err(error) = unregister_helper() {
        tracing::error!("removal helper retirement failed: {error:#}");
    }
    std::process::exit(0);
}

extern "C" fn idle_callback() {
    // An idle/rejected request does not unregister the installed helper job.
    if let Err(error) = cleanup_listener_executable() {
        tracing::error!("idle removal executable cleanup failed: {error:#}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

extern "C" fn remove_callback(delete_settings: bool) -> c_int {
    let result = std::panic::catch_unwind(|| execute_authorized_removal(delete_settings));
    match result {
        Ok(Ok(())) => 0,
        failure => {
            tracing::error!("authorized removal failed: {failure:?}");
            // A cleanup error after commit is a user-visible failure, but the
            // now-unlinked helper must still retire its fixed launchd registration.
            if [UNINSTALLER_PATH, REMOVAL_LAUNCHD_PATH].iter().all(|path| {
                fs::symlink_metadata(path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            }) {
                2
            } else {
                1
            }
        }
    }
}

fn verification_account() -> Result<ServiceAccount> {
    // Bundle verification does not use an account-bound Driver plist. The actual
    // transaction resolves the original account from that verified installed plist.
    Ok(ServiceAccount::new(
        "mixrelay-preflight".into(),
        "staff".into(),
        "/Users/mixrelay-preflight".into(),
    )?)
}

fn execute_authorized_removal(delete_settings: bool) -> Result<()> {
    require_root()?;
    // The listener's validated startup identity survives a user deleting the
    // installed plist or bundle. Missing files must not invalidate authorization.
    let team = LISTENER_SIGNING_TEAM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .context("removal listener has no validated publisher")?;
    verify_remaining_remover(&team)?;
    // The listener's verified private executable also remains available to the
    // normal-user cleanup/manager subprocesses after the remover is deleted.
    let mut command = Command::new(std::env::current_exe()?);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .args(["remove", team.as_str(), "--reload-coreaudio"]);
    if delete_settings {
        command.arg("--delete-user-data");
    }
    ensure!(
        command.status()?.success(),
        "fixed removal transaction failed"
    );
    Ok(())
}
