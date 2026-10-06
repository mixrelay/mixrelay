//! Actual filesystem tests for the unprivileged adapter.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

use super::*;
use crate::{InstallSources, OperationKind};

struct Fixture {
    _root: mixrelay_test_support::FixtureDir,
    root: PathBuf,
    app: PathBuf,
    hal: PathBuf,
    engine: PathBuf,
    launchd: PathBuf,
    user_data: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root_owner = mixrelay_test_support::FixtureDir::new("macos-filesystem")
            .expect("fixture root creates");
        let root = root_owner.path().to_path_buf();
        let lifecycle = root.join(LIFECYCLE_DIRECTORY.trim_start_matches('/'));
        fs::create_dir_all(&lifecycle).expect("lifecycle creates");
        let user_data = root.join("Users/fixture/Library/Application Support/MixRelay");
        fs::create_dir_all(&user_data).expect("user data creates");
        let fixture = Self {
            _root: root_owner,
            app: root.join("sources/MixRelay.app"),
            hal: root.join("sources/MixRelayHAL.driver"),
            engine: root.join("sources/mixrelay-driver-service"),
            launchd: root.join("sources/io.mixrelay.audio.driver.plist"),
            user_data: user_data.join("graphs.json"),
            root,
        };
        fixture.create_sources();
        fixture.create_user_data();
        fixture
    }

    fn create_sources(&self) {
        let app_executable = self.app.join("Contents/MacOS/mixrelay-desktop");
        let hal_executable = self.hal.join("Contents/MacOS/MixRelayHAL");
        write_executable(&app_executable, "new-app");
        write_executable(&hal_executable, "new-hal");
        write_file(&self.app.join("Contents/Info.plist"), "app-info");
        write_file(&self.hal.join("Contents/Info.plist"), "hal-info");
        fs::create_dir_all(self.app.join("Contents/Resources/empty"))
            .expect("empty app directory creates");
        fs::create_dir_all(self.hal.join("Contents/Resources/empty"))
            .expect("empty HAL directory creates");
        write_executable(&self.engine, "new-engine");
        write_file(&self.launchd, "new-launchd");
    }

    fn create_user_data(&self) {
        write_file(&self.user_data, "keep-this-data");
    }

    fn sources(&self) -> TransactionRequest {
        TransactionRequest::install(InstallSources::new(
            self.app.clone(),
            self.hal.clone(),
            self.engine.clone(),
            self.launchd.clone(),
        ))
    }

    fn adapter(&self) -> FilesystemAdapter {
        FilesystemAdapter::new(
            FilesystemLayout::new(self.root.clone()).expect("fixture layout validates"),
        )
    }

    fn destination(&self, target: OwnedTarget) -> PathBuf {
        self.root.join(target.path().trim_start_matches('/'))
    }

    fn create_old_targets(&self) {
        write_file(
            &self.destination(OwnedTarget::App).join("Contents/old.txt"),
            "old-app",
        );
        write_file(
            &self.destination(OwnedTarget::Hal).join("Contents/old.txt"),
            "old-hal",
        );
        write_file(&self.destination(OwnedTarget::Driver), "old-engine");
        write_file(&self.destination(OwnedTarget::Launchd), "old-launchd");
    }
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("fixture file has parent")).expect("parent creates");
    fs::write(path, contents).expect("fixture file writes");
}

fn write_executable(path: &Path, contents: &str) {
    write_file(path, contents);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("fixture executable mode sets");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("fixture file reads")
}

#[test]
fn preflight_and_stage_do_not_mutate_destinations_or_user_data() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();

    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");

    assert_eq!(snapshot.presence(OwnedTarget::App), TargetPresence::Present);
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "old-engine"
    );
    assert_eq!(
        fs::symlink_metadata(staged.staging_root())
            .expect("staging metadata reads")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        staged
            .staging_root()
            .join("payload/app/Contents/Resources/empty")
            .is_dir()
    );
    adapter.cleanup(&staged).expect("stage cleanup succeeds");
    assert!(!adapter.layout().staging_root().exists());
}

#[test]
fn staging_normalizes_permissions_without_inheriting_privileged_or_writable_modes() {
    let fixture = Fixture::new();
    fs::set_permissions(&fixture.app, fs::Permissions::from_mode(0o777)).unwrap();
    fs::set_permissions(&fixture.engine, fs::Permissions::from_mode(0o6755)).unwrap();
    fs::set_permissions(&fixture.launchd, fs::Permissions::from_mode(0o777)).unwrap();
    fs::set_permissions(
        fixture.app.join("Contents/Info.plist"),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).unwrap();
    let staged = adapter.stage(&request, &snapshot).unwrap();
    for (path, mode) in [
        (staged.payload_path(OwnedTarget::App), 0o755),
        (staged.payload_path(OwnedTarget::Driver), 0o755),
        (staged.payload_path(OwnedTarget::Launchd), 0o644),
        (
            staged
                .payload_path(OwnedTarget::App)
                .join("Contents/Info.plist"),
            0o644,
        ),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            mode
        );
    }
    assert_eq!(
        fs::metadata(&fixture.engine).unwrap().permissions().mode() & 0o7777,
        0o6755
    );
    adapter.cleanup(&staged).unwrap();
}

#[test]
fn direct_copy_rejects_special_files_and_does_not_overwrite_an_existing_inode() {
    let fixture = Fixture::new();
    let fifo = fixture.root.join("fifo");
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let destination = fixture.root.join("copied");
    assert!(copy_tree(&fifo, &destination).is_err());
    assert!(super::payload::copy_regular_file(&fifo, &destination).is_err());
    assert!(!destination.exists());
    let alias = fixture.root.join("alias");
    symlink(&fixture.engine, &alias).unwrap();
    assert!(super::payload::copy_regular_file(&alias, &destination).is_err());
    assert!(!destination.exists());
    fs::write(&destination, b"keep-existing").unwrap();
    assert!(copy_tree(&fixture.engine, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"keep-existing");
}

#[test]
fn receipts_are_bound_to_layout_request_and_snapshot() {
    let first = Fixture::new();
    let second = Fixture::new();
    let first_adapter = first.adapter();
    let second_adapter = second.adapter();
    let first_request = first.sources();
    let second_request = second.sources();
    let first_snapshot = first_adapter
        .preflight(&first_request)
        .expect("first preflight succeeds");
    let second_snapshot = second_adapter
        .preflight(&second_request)
        .expect("second preflight succeeds");
    let staged = first_adapter
        .stage(&first_request, &first_snapshot)
        .expect("first stage succeeds");

    let error = second_adapter
        .commit(&second_request, &second_snapshot, &staged)
        .expect_err("cross-layout receipt must reject before mutation");
    assert!(matches!(error, FilesystemError::ReceiptMismatch));
    assert!(!second.destination(OwnedTarget::App).exists());
    first_adapter
        .cleanup(&staged)
        .expect("first cleanup succeeds");
}

#[test]
fn install_commit_and_rollback_restore_exact_targets() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    let committed = adapter
        .commit(&request, &snapshot, &staged)
        .expect("commit succeeds");

    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "new-engine"
    );
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Launchd)),
        "new-launchd"
    );
    assert!(committed.backed_up_targets().contains(&OwnedTarget::App));

    adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect("rollback succeeds");
    assert_eq!(
        read(
            &fixture
                .destination(OwnedTarget::App)
                .join("Contents/old.txt")
        ),
        "old-app"
    );
    assert_eq!(
        read(
            &fixture
                .destination(OwnedTarget::Hal)
                .join("Contents/old.txt")
        ),
        "old-hal"
    );
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "old-engine"
    );
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Launchd)),
        "old-launchd"
    );
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    adapter.cleanup(&staged).expect("cleanup succeeds");
}

#[test]
fn successful_commit_cleanup_is_allowed_after_registration_boundary() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    adapter
        .commit(&request, &snapshot, &staged)
        .expect("commit succeeds");

    // The native caller invokes cleanup only after service registration succeeds.
    adapter
        .cleanup(&staged)
        .expect("committed staging cleanup succeeds");
    assert!(!adapter.layout().staging_root().exists());
}

#[test]
fn remove_commit_and_rollback_never_touch_user_data() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = TransactionRequest::remove();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    let committed = adapter
        .commit(&request, &snapshot, &staged)
        .expect("remove commit succeeds");

    for target in OwnedTarget::all() {
        assert!(!fixture.destination(target).exists());
    }
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect("remove rollback succeeds");
    assert!(fixture.destination(OwnedTarget::App).is_dir());
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "old-engine"
    );
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    adapter.cleanup(&staged).expect("cleanup succeeds");
}

#[test]
fn target_symlink_is_rejected_before_any_mutation() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let hal = fixture.destination(OwnedTarget::Hal);
    let old_hal = fixture.root.join("old-hal-outside-target");
    fs::rename(&hal, &old_hal).expect("old HAL moves");
    symlink(&old_hal, &hal).expect("HAL symlink creates");
    let adapter = fixture.adapter();

    let error = adapter
        .preflight(&fixture.sources())
        .expect_err("symlink target must reject preflight");
    assert!(matches!(error, FilesystemError::UnsafePath { .. }));
    assert!(hal.is_symlink());
    assert_eq!(read(&fixture.user_data), "keep-this-data");
}

#[test]
fn partial_commit_failure_rolls_back_real_fixture_files() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    fs::remove_dir_all(staged.staging_root().join("payload/hal"))
        .expect("HAL payload removal creates a commit failure");

    let error = adapter
        .commit(&request, &snapshot, &staged)
        .expect_err("missing payload must fail after earlier renames");
    assert!(matches!(error, FilesystemError::Io { .. }));
    let cleanup_error = adapter
        .cleanup(&staged)
        .expect_err("failed commit must retain rollback evidence");
    assert!(matches!(
        cleanup_error,
        FilesystemError::InvalidStageState {
            operation: "cleanup",
            state: FilesystemStageState::CommitFailed,
        }
    ));
    adapter
        .rollback(&request, &snapshot, &staged, None)
        .expect("partial commit rollback succeeds");
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "old-engine"
    );
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Launchd)),
        "old-launchd"
    );
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    adapter.cleanup(&staged).expect("cleanup succeeds");
}

#[test]
fn created_parent_replacement_is_retained_until_identity_is_restored() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    let committed = adapter
        .commit(&request, &snapshot, &staged)
        .expect("commit succeeds");

    let launchd = fixture.destination(OwnedTarget::Launchd);
    let launchd_parent = launchd.parent().expect("launchd has parent").to_path_buf();
    let moved_launchd = fixture.root.join("launchd-outside-parent");
    let moved_parent = fixture.root.join("launchd-parent-outside-target");
    fs::rename(&launchd, &moved_launchd).expect("launchd moves");
    fs::rename(&launchd_parent, &moved_parent).expect("created parent moves");
    fs::create_dir(&launchd_parent).expect("replacement parent creates");
    fs::rename(&moved_launchd, &launchd).expect("launchd returns to replacement parent");

    let failure = adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect_err("replacement parent identity must retain evidence");
    assert!(matches!(
        failure.error(),
        FilesystemError::CreatedDirectoryChanged { .. }
    ));
    assert!(failure.retained().staging_root().is_dir());
    assert!(!launchd.exists());

    fs::remove_dir(&launchd_parent).expect("replacement parent removes");
    fs::rename(&moved_parent, &launchd_parent).expect("original parent identity restores");
    adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect("rollback retry removes only original created parents");
    adapter
        .cleanup(&staged)
        .expect("cleanup succeeds after retry");
}

#[test]
fn staging_replacement_is_rejected_without_recursive_cleanup() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    let staging_root = staged.staging_root().to_path_buf();
    let moved_staging = fixture.root.join("staging-outside-target");
    fs::rename(&staging_root, &moved_staging).expect("staging moves");
    fs::create_dir(&staging_root).expect("replacement staging creates");

    let error = adapter
        .commit(&request, &snapshot, &staged)
        .expect_err("staging identity replacement must reject commit");
    assert!(matches!(
        error,
        FilesystemError::StagingIdentityChanged { .. }
    ));
    assert!(moved_staging.is_dir());
    assert!(staging_root.is_dir());
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    assert!(matches!(
        adapter.cleanup(&staged),
        Err(FilesystemError::StagingIdentityChanged { .. })
    ));
    fs::remove_dir(&staging_root).expect("remove empty replacement fixture directory");
    fs::rename(&moved_staging, &staging_root).expect("restore original staging inode");
    adapter
        .cleanup(&staged)
        .expect("same receipt can retry cleanup");
    assert!(!staging_root.exists());
}

#[test]
fn rollback_failure_retains_private_evidence_and_does_not_cleanup() {
    let fixture = Fixture::new();
    fixture.create_old_targets();
    let adapter = fixture.adapter();
    let request = fixture.sources();
    let snapshot = adapter.preflight(&request).expect("preflight succeeds");
    let staged = adapter.stage(&request, &snapshot).expect("stage succeeds");
    let committed = adapter
        .commit(&request, &snapshot, &staged)
        .expect("commit succeeds");
    let engine = fixture.destination(OwnedTarget::Driver);
    let moved_engine = fixture.root.join("engine-outside-target");
    fs::rename(&engine, &moved_engine).expect("new engine moves");
    symlink(&moved_engine, &engine).expect("engine symlink creates");

    let failure = adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect_err("rollback must retain evidence when target is replaced");
    assert!(failure.retained().staging_root().is_dir());
    assert!(staged.staging_root().is_dir());
    assert!(!fixture.destination(OwnedTarget::Launchd).exists());
    assert!(!fixture.destination(OwnedTarget::Hal).exists());
    assert_eq!(read(&fixture.user_data), "keep-this-data");
    let cleanup_error = adapter
        .cleanup(&staged)
        .expect_err("failed rollback must retain private evidence");
    assert!(matches!(
        cleanup_error,
        FilesystemError::InvalidStageState {
            operation: "cleanup",
            state: FilesystemStageState::RollbackFailed,
        }
    ));

    fs::remove_file(&engine).expect("engine symlink removes");
    fs::rename(&moved_engine, &engine).expect("new engine restores");
    adapter
        .rollback(&request, &snapshot, &staged, Some(&committed))
        .expect("rollback retry succeeds after the changed target is restored");
    assert_eq!(
        read(&fixture.destination(OwnedTarget::Driver)),
        "old-engine"
    );
    adapter
        .cleanup(&staged)
        .expect("cleanup succeeds after retry");
}

#[test]
fn install_request_shape_remains_explicit() {
    let fixture = Fixture::new();
    assert_eq!(fixture.sources().kind(), OperationKind::Install);
    assert_eq!(TransactionRequest::remove().kind(), OperationKind::Remove);
}
