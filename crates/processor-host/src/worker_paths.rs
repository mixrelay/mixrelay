//! Private, per-user processor IPC storage. Preparation and discovery use the
//! application cache; missing configuration is an error, never a global scratch
//! fallback. Unit-test files stay inside the checkout's Cargo target directory.

use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub(super) fn worker_paths() -> io::Result<(PathBuf, PathBuf)> {
    let directory = worker_directory()?;
    private_directory(&directory)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let stem = format!("processor-{}-{id}-{nonce}", std::process::id());
    Ok((
        directory.join(format!("{stem}.audio")),
        directory.join(format!("{stem}.json")),
    ))
}

#[cfg(not(test))]
fn worker_directory() -> io::Result<PathBuf> {
    cache_directory_for(std::env::consts::OS, |key| std::env::var_os(key))
        .map(|path| path.join("processor-workers"))
}

#[cfg(test)]
fn worker_directory() -> io::Result<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    Ok(workspace.join("target/test-artifacts/processor-host-runtime"))
}

fn cache_directory_for(
    os: &str,
    mut environment: impl FnMut(&str) -> Option<OsString>,
) -> io::Result<PathBuf> {
    let (key, suffix) = match os {
        "macos" => ("HOME", "Library/Caches/MixRelay"),
        "linux" => {
            if let Some(path) = environment("XDG_CACHE_HOME").filter(|path| !path.is_empty()) {
                return absolute(path, "XDG_CACHE_HOME").map(|path| path.join("MixRelay"));
            }
            ("HOME", ".cache/MixRelay")
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "processor cache is unsupported on this OS",
            ));
        }
    };
    let value = environment(key).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{key} is missing; processor cache unavailable"),
        )
    })?;
    absolute(value, key).map(|path| path.join(suffix))
}

fn absolute(value: OsString, key: &str) -> io::Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{key} must name an absolute per-user directory"),
        ));
    }
    Ok(path)
}

fn private_directory(directory: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(directory)?;
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "processor cache must be a real directory, not a symlink",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: geteuid has no arguments or pointer lifetimes. Do not chmod
        // another user's directory if environment/configuration points there.
        if metadata.uid() != unsafe { geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "processor cache is not owned by this user",
            ));
        }
        if metadata.permissions().mode() & 0o777 != 0o700 {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_test_support::FixtureDir;

    #[test]
    fn each_os_uses_its_per_user_cache_without_fallback() {
        let base = FixtureDir::new("processor-cache-layout").unwrap();
        for (os, key, suffix) in [
            ("macos", "HOME", "Library/Caches/MixRelay"),
            ("linux", "HOME", ".cache/MixRelay"),
        ] {
            assert_eq!(
                cache_directory_for(os, |name| (name == key)
                    .then(|| base.as_os_str().to_owned()))
                .unwrap(),
                base.join(suffix)
            );
            assert_eq!(
                cache_directory_for(os, |_| None).unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            assert_eq!(
                cache_directory_for(os, |name| (name == key).then(|| OsString::from("relative")))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(
            cache_directory_for("linux", |key| (key == "XDG_CACHE_HOME")
                .then(|| base.as_os_str().to_owned()))
            .unwrap(),
            base.join("MixRelay")
        );
        assert_eq!(
            cache_directory_for("linux", |key| Some(if key == "XDG_CACHE_HOME" {
                OsString::new()
            } else {
                base.as_os_str().to_owned()
            }))
            .unwrap(),
            base.join(".cache/MixRelay")
        );
        assert!(cache_directory_for("linux", |_| Some(OsString::from("relative"))).is_err());
        assert_eq!(
            cache_directory_for("unknown", |_| None).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn private_directory_and_unique_paths_remain_in_project_target() {
        let (first, first_config) = worker_paths().unwrap();
        let (second, _) = worker_paths().unwrap();
        assert_ne!(first, second);
        assert_eq!(first.with_extension("json"), first_config);
        assert!(
            first
                .parent()
                .unwrap()
                .ends_with("target/test-artifacts/processor-host-runtime")
        );
        let owned = FixtureDir::new("processor-private-cache").unwrap();
        let directory = owned.join("workers");
        private_directory(&directory).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
            private_directory(&directory).unwrap();
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let link = owned.join("link");
            std::os::unix::fs::symlink(&directory, &link).unwrap();
            assert_eq!(
                private_directory(&link).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        let file = owned.join("file");
        fs::write(&file, b"existing").unwrap();
        assert!(private_directory(&file).is_err());
    }
}
