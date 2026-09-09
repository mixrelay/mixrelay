//! Control-thread-only preferences. Stage beside the destination, sync, then replace;
//! a failed write must never truncate the last accepted session configuration.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tonenari_audio_io::session::{AudioBackendKind, SessionFormat};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
const MAX_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    version: u32,
    backend: AudioBackendKind,
    format: SessionFormat,
}

pub(crate) struct SessionPreferences {
    path: PathBuf,
    backend: AudioBackendKind,
}

impl SessionPreferences {
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf, backend: AudioBackendKind) -> Self {
        Self { path, backend }
    }

    pub(crate) fn for_backend(backend: AudioBackendKind) -> io::Result<Self> {
        Ok(Self {
            path: config_directory()?.join("audio-session.json"),
            backend,
        })
    }

    pub(crate) fn load(&self) -> io::Result<Option<SessionFormat>> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session preferences exceed size limit",
            ));
        }
        let value: Preferences = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if value.version != 1 || value.backend != self.backend {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported session preferences version or backend",
            ));
        }
        Ok(Some(value.format))
    }

    pub(crate) fn stage(&self, format: SessionFormat) -> io::Result<StagedPreferences> {
        stage_json(
            &self.path,
            &Preferences {
                version: 1,
                backend: self.backend,
                format,
            },
        )
    }
}

/// Shared atomic replacement for independent control-plane preference documents.
pub(crate) fn stage_json(path: &Path, value: &impl Serialize) -> io::Result<StagedPreferences> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("preferences need a directory"))?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(parent)?;
    let (temporary, mut file) = create_temporary(parent)?;
    let staged = StagedPreferences {
        temporary,
        destination: path.to_path_buf(),
    };
    // Keep the cleanup guard before fallible serialization. Close the file before the
    // guard unlinks it on error (Windows does not allow unlinking this open handle).
    let result = (|| {
        serde_json::to_writer(&mut file, value).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    drop(file);
    result?;
    Ok(staged)
}

pub(crate) struct StagedPreferences {
    temporary: PathBuf,
    destination: PathBuf,
}

impl StagedPreferences {
    pub(crate) fn commit(self) -> io::Result<()> {
        // Both paths are in one directory. Rust uses the platform replacement primitive;
        // never remove the destination first, including on Windows.
        fs::rename(&self.temporary, &self.destination)?;
        #[cfg(unix)]
        if let Some(parent) = self.destination.parent()
            && let Err(error) = File::open(parent).and_then(|directory| directory.sync_all())
        {
            // Replacement already committed. Do not report rollback after that point.
            // A directory-sync failure limits power-loss durability, not process-restart use.
            eprintln!("ToneNari session directory sync failed: {error}");
        }
        Ok(())
    }
}

impl Drop for StagedPreferences {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("ToneNari session temporary-file cleanup failed: {error}");
        }
    }
}

fn create_temporary(parent: &Path) -> io::Result<(PathBuf, File)> {
    for _ in 0..16 {
        let ticket = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".audio-session-{}-{ticket}.tmp",
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
        "session temporary-file names occupied",
    ))
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub(crate) fn config_directory() -> io::Result<PathBuf> {
    // Explicit override is useful for packaged services and non-interactive launches.
    if env::var_os("TONENARI_CONFIG_DIR").is_some() {
        return absolute_env("TONENARI_CONFIG_DIR").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "TONENARI_CONFIG_DIR must be absolute",
            )
        });
    }
    #[cfg(target_os = "macos")]
    let base = absolute_env("HOME").map(|home| home.join("Library/Application Support/ToneNari"));
    #[cfg(target_os = "windows")]
    let base = absolute_env("LOCALAPPDATA").map(|directory| directory.join("ToneNari"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = absolute_env("XDG_CONFIG_HOME")
        .or_else(|| absolute_env("HOME").map(|home| home.join(".config")))
        .map(|directory| directory.join("tonenari"));
    base.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "user configuration directory unavailable",
        )
    })
}
