use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{DriverDeviceConfig, DriverError, contract::JSON_LIMIT};

static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DriverStore {
    pub(crate) revision: u64,
    /// `false` keeps the retained devices unpublished until the user resumes the Driver.
    pub(crate) active: bool,
    pub(crate) devices: Vec<DriverDeviceConfig>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDevices<'a> {
    revision: u64,
    active: bool,
    devices: &'a [DriverDeviceConfig],
}

pub(crate) fn load(path: &Path) -> Result<Option<DriverStore>, DriverError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > JSON_LIMIT {
        return Err(DriverError::Invalid("Driver store exceeds 1 MiB".into()));
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub(crate) fn save(
    path: &Path,
    revision: u64,
    active: bool,
    devices: &[DriverDeviceConfig],
) -> Result<(), DriverError> {
    let bytes = serde_json::to_vec(&StoredDevices {
        revision,
        active,
        devices,
    })?;
    if bytes.len() > JSON_LIMIT {
        return Err(DriverError::Invalid("Driver store exceeds 1 MiB".into()));
    }
    write_atomic(path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DriverError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary_path = parent.join(format!(
        ".mixrelay-driver-{}-{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary_path)?;
    let temporary = PendingFile(temporary_path);
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary.0, path)?;
    Ok(())
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        // Only this unpublished sibling is owned here; the committed destination is untouched.
        let _ = fs::remove_file(&self.0);
    }
}
