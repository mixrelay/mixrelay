//! Control-side file transactions shared by graph catalogs and session settings.
//! Callers own validation and commit policy; this layer never negotiates audio.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Serialize;

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// Writes and syncs JSON beside its destination without replacing that destination.
/// The returned owner commits explicitly or removes only its staged file on drop.
/// Returns directory, creation, serialization, write or sync errors.
pub(crate) fn stage_json(path: &Path, value: &impl Serialize) -> io::Result<StagedWrite> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("document needs a directory"))?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(parent)?;
    let (temporary, mut file) = create_temporary(parent)?;
    let staged = StagedWrite {
        temporary,
        destination: path.to_path_buf(),
    };
    // Establish cleanup before serialization, and close the file before cleanup
    // on failure. Some platforms cannot unlink an open staged file.
    let result = (|| {
        serde_json::to_writer(&mut file, value).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    drop(file);
    result?;
    Ok(staged)
}

/// Sole owner of an unpublished file. Commit consumes the owner; cancellation
/// never removes or truncates the previously accepted destination.
#[must_use = "commit the staged document or drop it to cancel"]
#[derive(Debug)]
pub(crate) struct StagedWrite {
    temporary: PathBuf,
    destination: PathBuf,
}

impl StagedWrite {
    /// Atomically replaces the destination with the synced staged file.
    /// Returns a rename failure before commit. A later directory-sync failure is
    /// logged, not reported as rollback: the replacement has already happened.
    pub(crate) fn commit(self) -> io::Result<()> {
        fs::rename(&self.temporary, &self.destination)?;
        #[cfg(unix)]
        if let Some(parent) = self.destination.parent()
            && let Err(error) = File::open(parent).and_then(|directory| directory.sync_all())
        {
            tracing::warn!(%error, "MixRelay document directory sync failed after commit");
        }
        Ok(())
    }
}

impl Drop for StagedWrite {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "MixRelay staged document cleanup failed");
        }
    }
}

fn create_temporary(parent: &Path) -> io::Result<(PathBuf, File)> {
    for _ in 0..16 {
        let ticket = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".mixrelay-write-{}-{ticket}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "staged document names occupied",
    ))
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Resolves the application's configuration directory without creating it.
/// Returns an invalid override or unavailable user-directory error.
pub(crate) fn config_directory() -> io::Result<PathBuf> {
    if env::var_os("MIXRELAY_CONFIG_DIR").is_some() {
        return absolute_env("MIXRELAY_CONFIG_DIR").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "MIXRELAY_CONFIG_DIR must be absolute",
            )
        });
    }
    #[cfg(target_os = "macos")]
    let base = absolute_env("HOME").map(|home| home.join("Library/Application Support/MixRelay"));
    #[cfg(target_os = "windows")]
    let base = absolute_env("LOCALAPPDATA").map(|directory| directory.join("MixRelay"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = absolute_env("XDG_CONFIG_HOME")
        .or_else(|| absolute_env("HOME").map(|home| home.join(".config")))
        .map(|directory| directory.join("mixrelay"));
    base.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "user configuration directory unavailable",
        )
    })
}

#[cfg(test)]
mod tests;
