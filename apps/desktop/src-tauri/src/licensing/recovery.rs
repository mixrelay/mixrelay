//! Keyless receipts reconcile a confirmed remote change if credential persistence fails.

use std::{
    io::{Read as _, Write as _},
    path::Path,
};

use mixrelay_licensing::{Activation, LicenseError};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RemoteChange {
    ActivationPending {
        key_fingerprint: [u8; 32],
    },
    Activated {
        activation: Activation,
        key_fingerprint: [u8; 32],
    },
    Cleared,
}

pub(super) fn key_fingerprint(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

pub(super) fn read(path: &Path) -> Result<Option<RemoteChange>, LicenseError> {
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4_097)
                .read_to_end(&mut bytes)
                .map_err(|_| LicenseError::StorageUnavailable)?;
            if bytes.len() > 4_096 {
                return Err(LicenseError::StorageUnavailable);
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| LicenseError::StorageUnavailable)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LicenseError::StorageUnavailable),
    }
}

pub(super) fn preflight(path: &Path) -> Result<(), LicenseError> {
    let parent = path.parent().ok_or(LicenseError::StorageUnavailable)?;
    std::fs::create_dir_all(parent).map_err(|_| LicenseError::StorageUnavailable)?;
    let file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| LicenseError::StorageUnavailable)?;
    file.as_file()
        .sync_all()
        .map_err(|_| LicenseError::StorageUnavailable)
}

pub(super) fn write(path: &Path, change: &RemoteChange) -> Result<(), LicenseError> {
    let parent = path.parent().ok_or(LicenseError::StorageUnavailable)?;
    std::fs::create_dir_all(parent).map_err(|_| LicenseError::StorageUnavailable)?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| LicenseError::StorageUnavailable)?;
    serde_json::to_writer(&mut file, change).map_err(|_| LicenseError::StorageUnavailable)?;
    file.flush()
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| LicenseError::StorageUnavailable)?;
    file.persist(path)
        .map_err(|_| LicenseError::StorageUnavailable)?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LicenseError::StorageUnavailable)?;
    Ok(())
}

pub(super) fn remove(path: &Path) -> Result<(), LicenseError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(LicenseError::StorageUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_contains_only_public_seat_identity_and_survives_reopen() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("remote-change.json");
        let activation: Activation =
            serde_json::from_str(r#"{"licenseId":7,"instanceId":"seat-1","purchasedMajor":0}"#)
                .expect("seat");
        write(
            &path,
            &RemoteChange::Activated {
                activation,
                key_fingerprint: key_fingerprint("fixture-secret"),
            },
        )
        .expect("receipt");
        let value = std::fs::read_to_string(&path).expect("read");
        assert!(!value.contains("fixture-secret"));
        assert!(matches!(
            read(&path).expect("restore"),
            Some(RemoteChange::Activated { .. })
        ));
        write(&path, &RemoteChange::Cleared).expect("clear marker");
        assert!(matches!(
            read(&path).expect("restore"),
            Some(RemoteChange::Cleared)
        ));
        remove(&path).expect("remove");
        assert!(read(&path).expect("absent").is_none());
    }

    #[test]
    fn corrupt_receipt_is_not_treated_as_a_fresh_activation() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("remote-change.json");
        let mut file = std::fs::File::create(&path).expect("create");
        file.write_all(b"invalid").expect("corrupt fixture");
        assert!(matches!(read(&path), Err(LicenseError::StorageUnavailable)));
    }
}
