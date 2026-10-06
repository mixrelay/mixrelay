//! Process-owned leases for the desktop and its audio server. Lock files are
//! installed once in a root-owned directory; their existence is not an on/off flag.

use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    path::Path,
};

mod wake;
pub use wake::EngineWake;

/// Fixed, administrator-installed desktop lease. Only the configured user can write it.
pub const APP_LEASE: &str = "/Library/Application Support/MixRelay/lifecycle/app.owner";
/// Server lease, held until all native/audio owners have been destroyed.
pub const ENGINE_LEASE: &str = "/Library/Application Support/MixRelay/lifecycle/engine.owner";

/// An exclusive advisory lock released by the OS on normal exit or process death.
#[derive(Debug)]
pub struct ProcessLease(File);

impl ProcessLease {
    /// Claims an existing installer-owned inode without replacing or truncating it.
    ///
    /// # Errors
    /// Returns an I/O error for missing/inaccessible files or an existing owner.
    pub fn acquire(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::options().read(true).write(true).open(path)?;
        file.try_lock().map_err(io::Error::from)?;
        Ok(Self(file))
    }

    /// Claims server ownership before testing the desktop's liveness, closing
    /// the gap where Quit could observe no server while one was being admitted.
    ///
    /// # Errors
    /// Returns lease-file or lock errors. An absent desktop returns `Ok(None)`.
    pub fn acquire_for_app(
        engine_path: impl AsRef<Path>,
        app_path: impl AsRef<Path>,
    ) -> io::Result<Option<Self>> {
        let lease = Self::acquire(engine_path)?;
        Ok(is_owned(app_path)?.then_some(lease))
    }

    /// Records bounded lifecycle status while this process still owns the inode.
    /// Readers inspect it only after the lock is released.
    ///
    /// # Errors
    /// Returns truncate/write/flush errors. A stale `running` record is not success.
    pub fn record(&self, status: &str) -> io::Result<()> {
        self.0.set_len(0)?;
        let mut file = &self.0;
        file.rewind()?;
        file.write_all(&status.as_bytes()[..status.floor_char_boundary(4096)])?;
        file.sync_data()
    }
}

/// Reads the bounded finalization record after server ownership has ended.
///
/// # Errors
/// Returns file access or non-UTF-8 status errors.
pub fn engine_exit_record() -> io::Result<String> {
    let mut status = String::new();
    File::open(ENGINE_LEASE)?
        .take(4096)
        .read_to_string(&mut status)?;
    Ok(status)
}

impl Drop for ProcessLease {
    fn drop(&mut self) {
        // File closure also releases the lock. Explicit unlock makes ownership
        // visible before the final descriptor cleanup on a normal control exit.
        let _ = self.0.unlock();
    }
}

/// Checks a lease without requiring write access (the HAL runs as another user).
/// Never call from the audio callback: this is a control-plane filesystem probe.
///
/// # Errors
/// Returns missing-file, permission, or lock-system errors; callers must fail closed.
pub fn is_owned(path: impl AsRef<Path>) -> io::Result<bool> {
    let file = File::open(path)?;
    match file.try_lock_shared() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_admission_rechecks_desktop_under_engine_ownership() {
        let directory = mixrelay_test_support::FixtureDir::new("app-lifecycle").unwrap();
        let app_path = directory.join("app");
        let engine_path = directory.join("engine");
        File::create(&app_path).expect("app inode");
        File::create(&engine_path).expect("engine inode");
        assert!(
            ProcessLease::acquire_for_app(&engine_path, &app_path)
                .expect("absent app")
                .is_none()
        );
        assert!(!is_owned(&engine_path).expect("rejected owner released"));
        let app = ProcessLease::acquire(&app_path).expect("app owner");
        let engine = ProcessLease::acquire_for_app(&engine_path, &app_path)
            .expect("live app")
            .expect("admitted");
        drop(app);
        assert!(is_owned(&engine_path).expect("Quit must wait for engine teardown"));
        drop(engine);
        assert!(
            ProcessLease::acquire_for_app(&engine_path, &app_path)
                .expect("late demand")
                .is_none()
        );
    }

    #[test]
    fn ownership_follows_descriptor_lifetime_not_file_existence() {
        let directory = mixrelay_test_support::FixtureDir::new("app-lifecycle").unwrap();
        let path = directory.join("lease");
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("test inode");
        drop(file);
        assert!(!is_owned(&path).expect("unowned probe"));
        let lease = ProcessLease::acquire(&path).expect("claim");
        lease.record("running").expect("start record");
        lease.record("stopped").expect("exit record");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read record"),
            "stopped"
        );
        assert!(is_owned(&path).expect("owned probe"));
        assert!(ProcessLease::acquire(&path).is_err());
        drop(lease);
        assert!(!is_owned(&path).expect("released probe"));
    }

    #[test]
    fn missing_lease_does_not_create_an_owner() {
        let directory = mixrelay_test_support::FixtureDir::new("app-lifecycle").unwrap();
        let path = directory.join("missing-parent/owner");
        assert!(is_owned(&path).is_err());
        assert!(ProcessLease::acquire(&path).is_err());
    }
}
