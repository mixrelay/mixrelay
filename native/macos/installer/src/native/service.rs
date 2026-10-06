//! Native `ServiceLifecycle` implementation composed with the fixed transaction core.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    time::{Duration, Instant},
};

use core_foundation::{base::TCFType, string::CFString};
use mixrelay_driver_control::{DriverClient, DriverSnapshot, DriverState};

use crate::{
    FilesystemLayout, FilesystemStaged, HAL_PATH, LAUNCHD_PATH, LaunchdConfiguration, NativeError,
    OwnedTarget, ReleasePayloadVerifier, ServiceLifecycle, SigningTeam, TransactionRequest,
};

use super::{
    account::ResolvedServiceAccount,
    launchctl::{self, RegistrationState},
    trust::{
        require_lifecycle_storage, require_root, validate_destination_ancestors,
        validate_owned_target,
    },
};

/// The actual macOS `ServiceLifecycle` boundary for the coordinated transaction.
pub struct NativeServiceLifecycle {
    team: SigningTeam,
    account: ResolvedServiceAccount,
    verifier: ReleasePayloadVerifier,
    expected_registration: Option<RegistrationState>,
    delete_user_data_on_remove: bool,
    coreaudio_reload_approved: bool,
    coreaudio_reload_attempted: bool,
    pending_cleanup: Option<super::user_data::PendingCleanup>,
}

impl NativeServiceLifecycle {
    /// Creates an adapter bound to one Team ID and already OS-resolved account.
    #[must_use]
    pub fn new(team: SigningTeam, account: ResolvedServiceAccount) -> Self {
        let verifier = ReleasePayloadVerifier::new(team.clone(), account.account().clone());
        Self {
            team,
            account,
            verifier,
            expected_registration: None,
            delete_user_data_on_remove: false,
            coreaudio_reload_approved: false,
            coreaudio_reload_attempted: false,
            pending_cleanup: None,
        }
    }

    /// Records explicit administrator consent to interrupt audio during HAL install/remove.
    /// Without consent, preflight fails before lease acquisition or file/service mutation.
    #[must_use]
    pub const fn with_coreaudio_reload_approval(mut self, approved: bool) -> Self {
        self.coreaudio_reload_approved = approved;
        self
    }

    /// Selects optional Driver settings cleanup alongside automatic management UI cleanup.
    /// Graph App data and authentication are preserved.
    ///
    /// A cleanup failure uses the existing transaction rollback so the installed
    /// account plist remains available for a subsequent removal attempt.
    #[must_use]
    pub const fn with_removal_data_cleanup(mut self, selected: bool) -> Self {
        self.delete_user_data_on_remove = selected;
        self
    }

    fn verify_selected_account(&self) -> Result<(), NativeError> {
        let current = super::account::resolve_service_account(self.account.account())?;
        if current != self.account {
            return Err(NativeError::Account(
                "selected passwd/group/home identity changed during installer transaction"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn require_reload_approval(&self) -> Result<(), NativeError> {
        require_reload_approval(self.coreaudio_reload_approved)
    }

    fn reload_coreaudio(&mut self) -> Result<(), NativeError> {
        // Even a failed reload command may have killed the old server. Recovery must
        // reload the restored HAL in that case, not only after command success.
        self.coreaudio_reload_attempted = true;
        launchctl::reload_coreaudio()
    }

    fn inspect_owned_payload(
        &self,
        layout: &FilesystemLayout,
    ) -> Result<Option<LaunchdConfiguration>, NativeError> {
        let paths = OwnedTarget::all()
            .map(|target| layout.root().join(target.path().trim_start_matches('/')));
        let mut present = Vec::with_capacity(paths.len());
        for path in &paths {
            match fs::symlink_metadata(path) {
                Ok(_) => present.push(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => present.push(false),
                Err(source) => {
                    return Err(NativeError::Io {
                        operation: "inspect owned payload",
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        let present_count = present.iter().filter(|is_present| **is_present).count();
        if present_count == 0 {
            return Ok(None);
        }
        if present_count != paths.len() {
            return Err(NativeError::Trust {
                path: layout.root().to_path_buf(),
                reason: "an existing MixRelay installation is partial; refusing replacement/removal",
            });
        }
        for (path, expected_mode) in [
            (&paths[0], 0o755),
            (&paths[2], 0o755),
            (&paths[1], 0o755),
            (&paths[3], 0o644),
        ] {
            validate_owned_target(path, expected_mode)?;
        }
        self.verifier
            .verify_installed(
                paths[0].clone(),
                paths[2].clone(),
                paths[1].clone(),
                paths[3].clone(),
            )
            .map(Some)
            .map_err(NativeError::from)
    }

    fn write_bound_plist(
        staged: &FilesystemStaged,
        definition: &LaunchdConfiguration,
    ) -> Result<(), NativeError> {
        let path = staged.payload_path(OwnedTarget::Launchd);
        let bytes = definition.to_xml()?;
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|source| NativeError::Io {
                operation: "open private launchd payload",
                path: path.clone(),
                source,
            })?;
        let metadata = file.metadata().map_err(|source| NativeError::Io {
            operation: "inspect private launchd payload",
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.uid() != 0 {
            return Err(NativeError::Trust {
                path,
                reason: "private launchd payload is not a private root-owned regular inode",
            });
        }
        file.set_len(0).map_err(|source| NativeError::Io {
            operation: "truncate private launchd payload",
            path: path.clone(),
            source,
        })?;
        file.write_all(&bytes).map_err(|source| NativeError::Io {
            operation: "write account-bound launchd payload",
            path: path.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| NativeError::Io {
            operation: "sync account-bound launchd payload",
            path: path.clone(),
            source,
        })?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).map_err(|source| {
            NativeError::Io {
                operation: "set account-bound launchd payload mode",
                path,
                source,
            }
        })
    }

    fn current_registration() -> Result<RegistrationState, NativeError> {
        launchctl::inspect_registration()
    }

    fn ensure_idle(state: &RegistrationState) -> Result<(), NativeError> {
        if matches!(state, RegistrationState::Loaded { pid: Some(pid) } if *pid != 0) {
            return Err(NativeError::LaunchdState(
                "the exact MixRelay launchd job is still running",
            ));
        }
        Ok(())
    }

    fn bootstrap_restored_job(
        &mut self,
        expected: &LaunchdConfiguration,
    ) -> Result<(), NativeError> {
        let layout = FilesystemLayout::new("/")?;
        let actual = self
            .inspect_owned_payload(&layout)?
            .ok_or(NativeError::LaunchdState(
                "preflight registration snapshot has no complete restored payload",
            ))?;
        if &actual != expected {
            return Err(NativeError::Trust {
                path: PathBuf::from(LAUNCHD_PATH),
                reason: "restored payload differs from its preflight snapshot",
            });
        }
        let installed = installed_launchd_path();
        let bytes = read_bounded_file(&installed)?;
        let actual = LaunchdConfiguration::validate_installed(&bytes, self.account.account())?;
        if &actual != expected {
            return Err(NativeError::Trust {
                path: installed,
                reason: "restored launchd plist differs from its preflight snapshot",
            });
        }
        let version = installed_hal_version()?;
        if self.coreaudio_reload_attempted {
            activate_driver(
                &mut self.coreaudio_reload_attempted,
                launchctl::bootstrap,
                launchctl::reload_coreaudio,
                || wait_for_driver_ready(&version),
            )
        } else {
            launchctl::bootstrap()?;
            wait_for_driver_ready(&version)
        }
    }
}

impl ServiceLifecycle for NativeServiceLifecycle {
    type Snapshot = NativeServiceSnapshot;
    type Registration = NativeServiceRegistration;
    type Error = NativeError;

    fn inspect(
        &mut self,
        request: &TransactionRequest,
        layout: &FilesystemLayout,
    ) -> Result<Self::Snapshot, Self::Error> {
        self.require_reload_approval()?;
        require_root()?;
        self.verify_selected_account()?;
        validate_destination_ancestors(layout.root())?;
        require_lifecycle_storage(&self.account)?;
        let installed = self.inspect_owned_payload(layout)?;
        let registration = Self::current_registration()?;
        if installed.is_none() && matches!(registration, RegistrationState::Loaded { .. }) {
            return Err(NativeError::LaunchdState(
                "a service is registered but its complete signed payload set is absent",
            ));
        }
        if matches!(request, TransactionRequest::Remove)
            && installed.is_none()
            && registration != RegistrationState::Absent
        {
            return Err(NativeError::LaunchdState(
                "cannot remove an unverified registered service without its owned payload",
            ));
        }
        if let TransactionRequest::Install(sources) = request {
            self.verifier.verify_sources(sources)?;
        }
        self.expected_registration = None;
        self.coreaudio_reload_attempted = false;
        Ok(NativeServiceSnapshot {
            registration,
            installed,
            account: self.account.clone(),
            team: self.team.clone(),
        })
    }

    fn validate_stage(
        &mut self,
        request: &TransactionRequest,
        staged: &FilesystemStaged,
    ) -> Result<(), Self::Error> {
        self.require_reload_approval()?;
        require_root()?;
        if matches!(request, TransactionRequest::Remove) {
            self.verify_selected_account()?;
            // The embedded executable is still at its verified original path here;
            // commit moves the Manager bundle into private rollback storage.
            self.pending_cleanup = Some(
                super::user_data::prepare_for_account(
                    &self.account,
                    self.delete_user_data_on_remove,
                )
                .map_err(NativeError::UserData)?,
            );
            return Ok(());
        }
        let definition = self.verifier.verify_staged(staged)?;
        Self::write_bound_plist(staged, &definition)
    }

    fn drain(&mut self, snapshot: &Self::Snapshot) -> Result<(), Self::Error> {
        self.require_reload_approval()?;
        require_root()?;
        self.verify_selected_account()?;
        validate_destination_ancestors(&PathBuf::from("/"))?;
        require_lifecycle_storage(&self.account)?;
        let current = Self::current_registration()?;
        if current != snapshot.registration {
            return Err(NativeError::LaunchdState(
                "registration changed after serialized preflight",
            ));
        }
        if matches!(current, RegistrationState::Loaded { .. }) {
            launchctl::bootout()?;
            wait_for_service_exit(&current)?;
            if Self::current_registration()? != RegistrationState::Absent {
                return Err(NativeError::LaunchdState(
                    "Driver bootout returned but the verified service remains registered",
                ));
            }
        }
        self.expected_registration = Some(RegistrationState::Absent);
        Ok(())
    }

    fn register(
        &mut self,
        request: &TransactionRequest,
    ) -> Result<Self::Registration, Self::Error> {
        self.require_reload_approval()?;
        require_root()?;
        self.verify_selected_account()?;
        let layout = FilesystemLayout::new("/")?;
        validate_destination_ancestors(layout.root())?;
        require_lifecycle_storage(&self.account)?;
        if matches!(request, TransactionRequest::Install(_)) {
            super::trust::validate_engine_directory(layout.root(), true)?;
        }
        let installed = self.inspect_owned_payload(&layout)?;
        match request {
            TransactionRequest::Install(_) if installed.is_none() => {
                return Err(NativeError::Trust {
                    path: PathBuf::from("/"),
                    reason: "committed install payload is absent or incomplete",
                });
            }
            TransactionRequest::Remove if installed.is_some() => {
                return Err(NativeError::Trust {
                    path: PathBuf::from("/"),
                    reason: "remove commit left an owned payload behind",
                });
            }
            _ => {}
        }
        let expected = self
            .expected_registration
            .take()
            .ok_or(NativeError::LaunchdState(
                "registration was attempted without a successful drain",
            ))?;
        let current = Self::current_registration()?;
        if current != expected {
            return Err(NativeError::LaunchdState(
                "registration changed between drain and commit",
            ));
        }
        Self::ensure_idle(&current)?;
        match request {
            TransactionRequest::Install(_) => {
                let version = installed_hal_version()?;
                activate_driver(
                    &mut self.coreaudio_reload_attempted,
                    launchctl::bootstrap,
                    launchctl::reload_coreaudio,
                    || wait_for_driver_ready(&version),
                )?;
                Ok(NativeServiceRegistration)
            }
            TransactionRequest::Remove => {
                self.reload_coreaudio()?;
                wait_for_hal_absent()?;
                self.pending_cleanup
                    .take()
                    .ok_or_else(|| {
                        NativeError::UserData(anyhow::anyhow!("removal cleanup was not prepared"))
                    })?
                    .commit()
                    .map_err(NativeError::UserData)?;
                Ok(NativeServiceRegistration)
            }
        }
    }

    fn prepare_restore(&mut self, snapshot: &Self::Snapshot) -> Result<(), Self::Error> {
        drop(self.pending_cleanup.take());
        self.require_reload_approval()?;
        require_root()?;
        if snapshot.account != self.account || snapshot.team != self.team {
            return Err(NativeError::Trust {
                path: PathBuf::from("/"),
                reason: "service rollback snapshot belongs to another account or publisher",
            });
        }
        self.verify_selected_account()?;
        let layout = FilesystemLayout::new("/")?;
        validate_destination_ancestors(layout.root())?;
        require_lifecycle_storage(&self.account)?;
        let current = Self::current_registration()?;
        if matches!(current, RegistrationState::Loaded { .. }) {
            // A new Driver may have started before readiness failed. Revalidate
            // its complete signed on-disk identity before stopping the exact job.
            self.inspect_owned_payload(&layout)?
                .ok_or(NativeError::LaunchdState(
                    "cannot drain a registered Driver without its verified payload",
                ))?;
            launchctl::bootout()?;
            wait_for_service_exit(&current)?;
        }
        if Self::current_registration()? != RegistrationState::Absent {
            return Err(NativeError::LaunchdState(
                "rollback cannot restore files while the Driver remains registered",
            ));
        }
        Ok(())
    }

    fn restore(&mut self, snapshot: &Self::Snapshot) -> Result<(), Self::Error> {
        self.require_reload_approval()?;
        require_root()?;
        if snapshot.account != self.account || snapshot.team != self.team {
            return Err(NativeError::Trust {
                path: PathBuf::from("/"),
                reason: "service rollback snapshot belongs to another account or publisher",
            });
        }
        self.verify_selected_account()?;
        validate_destination_ancestors(&PathBuf::from("/"))?;
        require_lifecycle_storage(&self.account)?;
        let layout = FilesystemLayout::new("/")?;
        let restored = self.inspect_owned_payload(&layout)?;
        if restored != snapshot.installed {
            return Err(NativeError::Trust {
                path: PathBuf::from(LAUNCHD_PATH),
                reason: "restored payload differs from its preflight snapshot",
            });
        }
        if Self::current_registration()? != RegistrationState::Absent {
            return Err(NativeError::LaunchdState(
                "registration changed after rollback drain",
            ));
        }
        match &snapshot.registration {
            RegistrationState::Absent => {
                if self.coreaudio_reload_attempted {
                    launchctl::reload_coreaudio()?;
                    if snapshot.installed.is_none() {
                        wait_for_hal_absent()?;
                    }
                }
                Ok(())
            }
            RegistrationState::Loaded { .. } => {
                let expected = snapshot
                    .installed
                    .as_ref()
                    .ok_or(NativeError::LaunchdState(
                        "preflight registration snapshot has no validated installed plist",
                    ))?;
                self.bootstrap_restored_job(expected)
            }
        }
    }
}

/// Immutable platform snapshot used to restore launchd registration after file rollback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeServiceSnapshot {
    registration: RegistrationState,
    installed: Option<LaunchdConfiguration>,
    account: ResolvedServiceAccount,
    team: SigningTeam,
}

/// Receipt of a completed native launchd registration change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeServiceRegistration;

fn installed_launchd_path() -> PathBuf {
    PathBuf::from("/").join(LAUNCHD_PATH.trim_start_matches('/'))
}

fn require_reload_approval(approved: bool) -> Result<(), NativeError> {
    if !approved {
        return Err(NativeError::CoreAudioReloadApprovalRequired);
    }
    Ok(())
}

fn activate_driver(
    reload_attempted: &mut bool,
    bootstrap: impl FnOnce() -> Result<(), NativeError>,
    reload: impl FnOnce() -> Result<(), NativeError>,
    ready: impl FnOnce() -> Result<(), NativeError>,
) -> Result<(), NativeError> {
    bootstrap()?;
    *reload_attempted = true;
    reload()?;
    ready()
}

fn installed_hal_version() -> Result<String, NativeError> {
    let path = PathBuf::from(HAL_PATH).join("Contents/Info.plist");
    let bytes = read_bounded_file(&path)?;
    hal_release_version(&bytes)
}

fn hal_release_version(bytes: &[u8]) -> Result<String, NativeError> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|error| NativeError::Activation(format!("invalid HAL version plist: {error}")))?;
    value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleShortVersionString"))
        .and_then(plist::Value::as_string)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| NativeError::Activation("signed HAL release version is absent".to_owned()))
}

fn validate_driver_ready(
    snapshot: &DriverSnapshot,
    registration: &RegistrationState,
    version: &str,
) -> Result<(), NativeError> {
    if snapshot.state != DriverState::Ready {
        return Err(NativeError::Activation(format!(
            "Driver is not Ready ({:?})",
            snapshot.state
        )));
    }
    if snapshot.installed_version.as_deref() != Some(version) {
        return Err(NativeError::Activation(
            "Driver release version differs from the committed HAL".to_owned(),
        ));
    }
    let RegistrationState::Loaded { pid: Some(pid) } = registration else {
        return Err(NativeError::Activation(
            "registered Driver has no live PID".to_owned(),
        ));
    };
    if *pid == 0 || snapshot.owner_process_id != Some(*pid) {
        return Err(NativeError::Activation(
            "Driver snapshot owner differs from the current launchd PID".to_owned(),
        ));
    }
    Ok(())
}

fn wait_for_driver_ready(version: &str) -> Result<(), NativeError> {
    let client = DriverClient::new()
        .map_err(|error| NativeError::Activation(format!("Driver client: {error}")))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let result = client
            .snapshot_blocking()
            .map_err(|error| NativeError::Activation(format!("Driver snapshot: {error}")))
            .and_then(|snapshot| {
                // Inspect after the response so a stale owner cannot satisfy readiness.
                let registration = launchctl::inspect_registration()?;
                validate_driver_ready(&snapshot, &registration, version)
            });
        match result {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const std::ffi::c_void,
        data_size: *mut u32,
        data: *mut std::ffi::c_void,
    ) -> i32;
}

fn hal_is_loaded() -> Result<bool, NativeError> {
    let bundle = CFString::new("io.mixrelay.audio.driver");
    let qualifier = bundle.as_concrete_TypeRef();
    let address = AudioObjectPropertyAddress {
        selector: u32::from_be_bytes(*b"bidp"),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    let mut plugin = 0_u32;
    let mut size = 4_u32;
    let qualifier_size = u32::try_from(std::mem::size_of_val(&qualifier))
        .map_err(|_| NativeError::Activation("invalid CFString qualifier size".to_owned()))?;
    // SAFETY: the qualifier points to a live, owned CFStringRef until this
    // synchronous call returns. The aligned, disjoint u32 output has exactly
    // the declared four-byte capacity; the address has the official C layout.
    let status = unsafe {
        AudioObjectGetPropertyData(
            1,
            &raw const address,
            qualifier_size,
            (&raw const qualifier).cast(),
            &raw mut size,
            (&raw mut plugin).cast(),
        )
    };
    classify_hal_lookup(status, size, plugin)
}

fn classify_hal_lookup(status: i32, size: u32, plugin: u32) -> Result<bool, NativeError> {
    if status != 0 || size != 4 {
        return Err(NativeError::Activation(format!(
            "CoreAudio bundle lookup failed (status {status}, size {size})"
        )));
    }
    Ok(plugin != 0)
}

fn wait_for_hal_absent() -> Result<(), NativeError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match hal_is_loaded() {
            Ok(false) => return Ok(()),
            result if Instant::now() >= deadline => {
                return match result {
                    Err(error) => Err(error),
                    Ok(_) => Err(NativeError::Activation(
                        "removed HAL is still loaded by CoreAudio".to_owned(),
                    )),
                };
            }
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn wait_for_service_exit(state: &RegistrationState) -> Result<(), NativeError> {
    let RegistrationState::Loaded { pid: Some(pid) } = state else {
        return Ok(());
    };
    let pid = i32::try_from(*pid).map_err(|_| NativeError::LaunchdState("invalid Driver PID"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        // SAFETY: signal zero only queries the exact launchd-observed PID and
        // does not deliver a signal or accept any pointer arguments.
        if unsafe { libc::kill(pid, 0) } == -1
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(NativeError::LaunchdState(
                "Driver process did not drain after bootout",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn read_bounded_file(path: &std::path::Path) -> Result<Vec<u8>, NativeError> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|source| NativeError::Io {
        operation: "read bounded installed metadata",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| NativeError::Io {
        operation: "inspect bounded installed metadata",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(NativeError::Trust {
            path: path.to_path_buf(),
            reason: "installed metadata is not a bounded regular file",
        });
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| NativeError::Io {
            operation: "read bounded installed metadata",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > 16 * 1024 {
        return Err(NativeError::Trust {
            path: path.to_path_buf(),
            reason: "installed metadata exceeds its size limit",
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ready_snapshot() -> DriverSnapshot {
        serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "revision": 1,
            "state": "ready",
            "installedVersion": "0.1.0",
            "ownerProcessId": 42,
            "settings": {
                "canCreate": true, "canRename": true,
                "renderChannels": [1, 2], "captureChannels": [1, 2],
                "creationTemplate": null, "devices": [],
            },
        }))
        .unwrap()
    }

    #[test]
    fn approval_is_required_even_for_an_otherwise_valid_transaction() {
        assert!(matches!(
            require_reload_approval(false),
            Err(NativeError::CoreAudioReloadApprovalRequired)
        ));
        assert!(require_reload_approval(true).is_ok());
    }

    #[test]
    fn activation_bootstraps_then_reloads_once_then_checks_readiness() {
        let events = RefCell::new(Vec::new());
        let mut attempted = false;
        activate_driver(
            &mut attempted,
            || {
                events.borrow_mut().push("bootstrap");
                Ok(())
            },
            || {
                events.borrow_mut().push("reload");
                Ok(())
            },
            || {
                events.borrow_mut().push("ready");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["bootstrap", "reload", "ready"]);
        assert!(attempted);
    }

    #[test]
    fn failed_bootstrap_never_reloads_and_failed_reload_still_requires_runtime_recovery() {
        for failed_operation in ["bootstrap", "reload", "ready"] {
            let events = RefCell::new(Vec::new());
            let mut attempted = false;
            let action = |operation| {
                events.borrow_mut().push(operation);
                if failed_operation == operation {
                    Err(NativeError::LaunchdState("injected activation failure"))
                } else {
                    Ok(())
                }
            };
            assert!(
                activate_driver(
                    &mut attempted,
                    || action("bootstrap"),
                    || action("reload"),
                    || action("ready")
                )
                .is_err()
            );
            let expected = match failed_operation {
                "bootstrap" => vec!["bootstrap"],
                "reload" => vec!["bootstrap", "reload"],
                "ready" => vec!["bootstrap", "reload", "ready"],
                _ => unreachable!(),
            };
            assert_eq!(*events.borrow(), expected);
            assert_eq!(attempted, failed_operation != "bootstrap");
        }
    }

    #[test]
    fn hal_absence_requires_successful_coreaudio_lookup_not_an_error() {
        assert!(!classify_hal_lookup(0, 4, 0).unwrap());
        assert!(classify_hal_lookup(0, 4, 100).unwrap());
        assert!(classify_hal_lookup(-1, 4, 0).is_err());
        assert!(classify_hal_lookup(0, 0, 0).is_err());
        assert!(classify_hal_lookup(0, 8, 0).is_err());
    }

    #[test]
    fn readiness_accepts_empty_desired_inventory_but_requires_current_owner_and_version() {
        let mut snapshot = ready_snapshot();
        let current = RegistrationState::Loaded { pid: Some(42) };
        assert!(snapshot.settings.devices.is_empty());
        assert!(validate_driver_ready(&snapshot, &current, "0.1.0").is_ok());
        for state in [
            DriverState::InstalledNotLoaded,
            DriverState::NotInstalled,
            DriverState::Unavailable,
        ] {
            snapshot.state = state;
            assert!(validate_driver_ready(&snapshot, &current, "0.1.0").is_err());
        }
        snapshot.state = DriverState::Ready;
        assert!(validate_driver_ready(&snapshot, &current, "0.2.0").is_err());
        snapshot.installed_version = None;
        assert!(validate_driver_ready(&snapshot, &current, "0.1.0").is_err());
        snapshot.installed_version = Some("0.1.0".to_owned());
        for owner in [None, Some(0), Some(41)] {
            snapshot.owner_process_id = owner;
            assert!(validate_driver_ready(&snapshot, &current, "0.1.0").is_err());
        }
        snapshot.owner_process_id = Some(42);
        for registration in [
            RegistrationState::Absent,
            RegistrationState::Loaded { pid: None },
            RegistrationState::Loaded { pid: Some(43) },
        ] {
            assert!(validate_driver_ready(&snapshot, &registration, "0.1.0").is_err());
        }
    }

    #[test]
    fn hal_release_version_requires_the_committed_bundle_version() {
        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleShortVersionString".to_owned(),
            plist::Value::String("0.1.0".to_owned()),
        );
        let mut bytes = Vec::new();
        plist::Value::Dictionary(dictionary)
            .to_writer_xml(&mut bytes)
            .unwrap();
        assert_eq!(hal_release_version(&bytes).unwrap(), "0.1.0");
        assert!(hal_release_version(b"<plist version=\"1.0\"><dict/></plist>").is_err());
        assert!(hal_release_version(b"invalid").is_err());
    }
}
