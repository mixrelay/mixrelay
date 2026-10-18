//! Product-folder identity and empty-only cleanup; app bundles remain separate targets.

use std::{fs, io, os::unix::fs::MetadataExt, path::PathBuf};

use super::{
    FilesystemAdapter, FilesystemError, FilesystemIdentity, FilesystemSnapshot, MutationProgress,
    PRODUCT_DIRECTORY, identity, io_failure,
};

impl FilesystemAdapter {
    pub(super) fn product_directory_path(&self) -> PathBuf {
        self.layout
            .root()
            .join(PRODUCT_DIRECTORY.trim_start_matches('/'))
    }

    pub(super) fn observe_product_directory(
        &self,
    ) -> Result<Option<FilesystemIdentity>, FilesystemError> {
        let path = self.product_directory_path();
        super::ensure_no_symlink_components(&path, true)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_failure("inspect product folder", &path, error)),
        };
        // Fixtures use their normal owner; production requires root.
        let owner = if self.layout.root() == std::path::Path::new("/") {
            0
        } else {
            rustix::process::geteuid().as_raw()
        };
        if !metadata.is_dir() || !product_directory_policy(metadata.uid(), metadata.mode(), owner) {
            return Err(FilesystemError::UnsafePath {
                path,
                reason: "product folder must be a protected owned directory",
            });
        }
        Ok(Some(identity(&metadata)))
    }

    pub(super) fn ensure_product_directory(
        &self,
        snapshot: &FilesystemSnapshot,
        progress: &MutationProgress,
    ) -> Result<(), FilesystemError> {
        let path = self.product_directory_path();
        let expected = snapshot.product_directory.or_else(|| {
            progress
                .created_directories
                .iter()
                .find(|directory| directory.path == path)
                .map(|directory| directory.identity)
        });
        if self.observe_product_directory()? != expected {
            return Err(FilesystemError::CreatedDirectoryChanged { path });
        }
        Ok(())
    }

    pub(super) fn remove_empty_product_directory(
        &self,
        expected: Option<FilesystemIdentity>,
    ) -> Result<(), FilesystemError> {
        let observed = self.observe_product_directory()?;
        if observed.is_none() {
            return Ok(());
        }
        if observed != expected {
            return Err(FilesystemError::CreatedDirectoryChanged {
                path: self.product_directory_path(),
            });
        }
        let path = self.product_directory_path();
        match fs::remove_dir(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(()),
            Err(error) => Err(io_failure("remove empty product folder", &path, error)),
        }
    }
}

fn product_directory_policy(uid: u32, mode: u32, expected_uid: u32) -> bool {
    uid == expected_uid && mode & 0o022 == 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn production_product_folder_refuses_non_root_and_writable_owners() {
        assert!(super::product_directory_policy(0, 0o755, 0));
        assert!(!super::product_directory_policy(501, 0o755, 0));
        assert!(!super::product_directory_policy(0, 0o775, 0));
        assert!(!super::product_directory_policy(0, 0o757, 0));
    }
}
