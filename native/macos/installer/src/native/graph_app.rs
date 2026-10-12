//! Installs the editor independently of the Driver-owned audio runtime.

use std::{
    fs::{self, DirBuilder},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
#[cfg(test)]
use plist::Value;

use super::{ResolvedServiceAccount, launchctl, require_lifecycle_storage, require_root};
use crate::{LIFECYCLE_DIRECTORY, LifecycleDirectory, ReleasePayloadVerifier, SigningTeam};

const APP: &str = "/Applications/MixRelay.app";
const BASE: &str = "/Library/Application Support/MixRelay";

/// Replaces only the editor bundle. Driver owns and installs all audio executables.
/// # Errors
/// Returns on active owners, invalid code, unsafe paths, commit or cleanup failure.
pub fn install_graph_app(
    team: SigningTeam,
    account: &ResolvedServiceAccount,
    app_source: &Path,
) -> Result<()> {
    require_root()?;
    require_lifecycle_storage(account)?;
    let verifier = ReleasePayloadVerifier::new(team, account.account().clone());
    trusted_path(app_source)?;
    trusted_path(Path::new(APP))?;
    verifier.verify_graph_bundle(app_source)?;
    if Path::new(APP).exists() {
        verifier.verify_graph_bundle(Path::new(APP))?;
    }
    let _leases = LifecycleDirectory::open(LIFECYCLE_DIRECTORY)?.acquire_for_editor_update()?;
    let stage = PrivateStage::create()?;
    let result = (|| {
        let fresh = [stage.path.join("app")];
        checked(
            "/usr/bin/ditto",
            &[
                app_source.to_str().context("non-UTF-8 source")?,
                fresh[0].to_str().context("non-UTF-8 stage")?,
            ],
        )?;
        verifier.verify_graph_bundle(&fresh[0])?;
        ensure_parent(Path::new("/Applications"))?;
        commit_payload(
            Replacement {
                stage: &stage.path,
                fresh: &fresh,
                destinations: [Path::new(APP)],
                was_registered: false,
            },
            None,
            trusted_path,
            || {
                verifier
                    .verify_graph_bundle(Path::new(APP))
                    .map_err(Into::into)
            },
        )
    })();
    let cleanup = stage.cleanup();
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("temporary editor cleanup failed: {cleanup:#}")))
        }
    }
}

trait EngineRegistration {
    fn registered(&mut self) -> Result<bool>;
    fn bootout(&mut self) -> Result<()>;
    fn bootstrap(&mut self) -> Result<()>;
}

#[derive(Clone, Copy)]
struct Replacement<'a, const N: usize> {
    stage: &'a Path,
    fresh: &'a [PathBuf; N],
    destinations: [&'a Path; N],
    was_registered: bool,
}

fn commit_payload<const N: usize>(
    replacement: Replacement<'_, N>,
    mut service: Option<&mut dyn EngineRegistration>,
    validate: impl Fn(&Path) -> Result<()>,
    verify_installed: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let Replacement {
        stage,
        fresh,
        destinations,
        was_registered,
    } = replacement;
    let mut backed_up = Vec::new();
    let mut installed = Vec::new();
    let result = (|| {
        if was_registered {
            service
                .as_deref_mut()
                .context("missing engine registration")?
                .bootout()?;
        }
        for (index, destination) in destinations.iter().enumerate() {
            validate(destination)?;
            if destination.exists() {
                fs::rename(destination, stage.join(format!("previous-{index}")))?;
                backed_up.push(index);
            }
            fs::rename(&fresh[index], destination)?;
            installed.push(index);
        }
        verify_installed()?;
        if let Some(service) = service.as_deref_mut() {
            service.bootstrap()?;
            ensure!(service.registered()?, "engine service was not registered");
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut recovery = Vec::new();
        if let Some(service) = service.as_deref_mut() {
            match service.registered() {
                Ok(true) => {
                    if let Err(error) = service.bootout() {
                        recovery.push(error.to_string());
                    }
                }
                Ok(false) => {}
                Err(error) => recovery.push(error.to_string()),
            }
        }
        for index in installed.into_iter().rev() {
            if let Err(error) =
                validate(destinations[index]).and_then(|()| remove_exact(destinations[index]))
            {
                recovery.push(error.to_string());
            }
        }
        for index in backed_up.into_iter().rev() {
            if let Err(error) = validate(destinations[index]).and_then(|()| {
                fs::rename(stage.join(format!("previous-{index}")), destinations[index])
                    .map_err(Into::into)
            }) {
                recovery.push(error.to_string());
            }
        }
        if was_registered
            && let Err(error) = service
                .context("missing engine registration")
                .and_then(EngineRegistration::bootstrap)
        {
            recovery.push(error.to_string());
        }
        return if recovery.is_empty() {
            Err(error)
        } else {
            Err(error.context(format!("app restoration failed: {}", recovery.join("; "))))
        };
    }
    Ok(())
}

/// Checks whether the restored app and its bound engine are ready for relaunch.
/// # Errors
/// Returns on invalid code, mismatched configuration or unknown registration.
pub fn graph_app_ready(team: SigningTeam, account: &ResolvedServiceAccount) -> Result<bool> {
    let verifier = ReleasePayloadVerifier::new(team, account.account().clone());
    verifier.verify_graph_bundle(Path::new(APP))?;
    Ok(true)
}

#[cfg(test)]
fn bound_definition(account: &crate::ServiceAccount) -> Result<Value> {
    crate::runtime_definition::for_account(account)
}

fn checked(program: &'static str, arguments: &[&str]) -> Result<()> {
    let output = launchctl::run_command(program, arguments)?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn trusted_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "app path must be absolute");
    let mut current = PathBuf::new();
    for component in path.components() {
        ensure!(
            matches!(component, Component::RootDir | Component::Normal(_)),
            "app path must be normalized"
        );
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let applications = current == Path::new("/Applications")
            && metadata.gid() == super::account::group_id_for_name("admin")?;
        ensure!(
            !metadata.file_type().is_symlink()
                && metadata.uid() == 0
                && metadata.mode() & 0o002 == 0
                && (applications || metadata.mode() & 0o020 == 0),
            "untrusted app installation path: {}",
            current.display()
        );
        if current != path {
            ensure!(metadata.is_dir(), "app path parent must be a directory");
        }
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    trusted_path(path)?;
    if !path.exists() {
        let parent = path.parent().context("missing app installation parent")?;
        ensure_parent(parent)?;
        DirBuilder::new().mode(0o755).create(path)?;
    }
    Ok(())
}

fn remove_exact(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

struct PrivateStage {
    path: PathBuf,
    identity: (u64, u64),
}
impl PrivateStage {
    fn create() -> Result<Self> {
        trusted_path(Path::new(BASE))?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = Path::new(BASE).join(format!(".app-install-{}-{nonce}", std::process::id()));
        DirBuilder::new().mode(0o700).create(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        Ok(Self {
            path,
            identity: (metadata.dev(), metadata.ino()),
        })
    }
    fn cleanup(&self) -> Result<()> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && (metadata.dev(), metadata.ino()) == self.identity,
            "app staging inode changed; refusing unrelated cleanup"
        );
        fs::remove_dir_all(&self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FixtureRegistration {
        loaded: bool,
        fail_next_bootstrap: bool,
        events: Vec<&'static str>,
    }
    impl EngineRegistration for FixtureRegistration {
        fn registered(&mut self) -> Result<bool> {
            Ok(self.loaded)
        }
        fn bootout(&mut self) -> Result<()> {
            self.events.push("bootout");
            self.loaded = false;
            Ok(())
        }
        fn bootstrap(&mut self) -> Result<()> {
            self.events.push("bootstrap");
            if self.fail_next_bootstrap {
                self.fail_next_bootstrap = false;
                anyhow::bail!("injected registration failure");
            }
            self.loaded = true;
            Ok(())
        }
    }

    fn replacement_fixture() -> (
        mixrelay_test_support::FixtureDir,
        PathBuf,
        [PathBuf; 3],
        [PathBuf; 3],
    ) {
        let root = mixrelay_test_support::FixtureDir::new("app-install-replacement").unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        let fresh = [stage.join("app"), stage.join("engine"), stage.join("plist")];
        let destinations = [
            root.path().join("installed-app"),
            root.path().join("installed-engine"),
            root.path().join("installed-plist"),
        ];
        fs::create_dir(&fresh[0]).unwrap();
        fs::write(fresh[0].join("code"), "new-app").unwrap();
        fs::create_dir(&destinations[0]).unwrap();
        fs::write(destinations[0].join("code"), "old-app").unwrap();
        for index in 1..3 {
            fs::write(&fresh[index], format!("new-{index}")).unwrap();
            fs::write(&destinations[index], format!("old-{index}")).unwrap();
        }
        (root, stage, fresh, destinations)
    }

    #[test]
    fn app_commit_and_failed_registration_restore_real_files_before_reloading_old_service() {
        for fail in [false, true] {
            let (_root, stage, fresh, destinations) = replacement_fixture();
            let mut service = FixtureRegistration {
                loaded: true,
                fail_next_bootstrap: fail,
                events: vec![],
            };
            let result = commit_payload(
                Replacement {
                    stage: &stage,
                    fresh: &fresh,
                    destinations: destinations.each_ref().map(PathBuf::as_path),
                    was_registered: true,
                },
                Some(&mut service),
                |_| Ok(()),
                || {
                    assert_eq!(
                        fs::read_to_string(destinations[0].join("code")).unwrap(),
                        "new-app"
                    );
                    Ok(())
                },
            );
            assert_eq!(result.is_err(), fail);
            assert_eq!(
                fs::read_to_string(destinations[0].join("code")).unwrap(),
                if fail { "old-app" } else { "new-app" }
            );
            assert_eq!(
                fs::read_to_string(&destinations[1]).unwrap(),
                if fail { "old-1" } else { "new-1" }
            );
            assert!(service.loaded);
            assert_eq!(
                service.events,
                if fail {
                    vec!["bootout", "bootstrap", "bootstrap"]
                } else {
                    vec!["bootout", "bootstrap"]
                }
            );
            let metadata = fs::symlink_metadata(&stage).unwrap();
            let private = PrivateStage {
                path: stage.clone(),
                identity: (metadata.dev(), metadata.ino()),
            };
            private.cleanup().unwrap();
            private.cleanup().unwrap();
            assert!(!stage.exists());
        }
    }

    #[test]
    fn editor_replacement_keeps_driver_runtime_and_engine_leases_owned() {
        let (root, stage, fresh, destinations) = replacement_fixture();
        let lifecycle = root.path().join("lifecycle");
        fs::create_dir(&lifecycle).unwrap();
        let mut owners = Vec::new();
        for name in ["app.owner", "engine.owner", "driver-manager.owner"] {
            let path = lifecycle.join(name);
            fs::write(&path, b"running").unwrap();
            let owner = fs::File::options()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            owner.try_lock().unwrap();
            owners.push(owner);
        }
        let directory = LifecycleDirectory::open(&lifecycle).unwrap();
        let _update = directory.acquire_for_editor_update().unwrap();
        let editor_fresh = [fresh[0].clone()];
        commit_payload(
            Replacement {
                stage: &stage,
                fresh: &editor_fresh,
                destinations: [destinations[0].as_path()],
                was_registered: false,
            },
            None,
            |_| Ok(()),
            || {
                assert_eq!(
                    fs::read_to_string(destinations[0].join("code")).unwrap(),
                    "new-app"
                );
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&destinations[1]).unwrap(), "old-1");
        for name in ["app.owner", "engine.owner", "driver-manager.owner"] {
            let lease = fs::File::open(lifecycle.join(name)).unwrap();
            assert!(matches!(
                lease.try_lock_shared(),
                Err(fs::TryLockError::WouldBlock)
            ));
        }
        drop(owners);
    }

    #[test]
    fn partial_file_commit_is_rolled_back_and_never_registers_incomplete_new_payload() {
        let (_root, stage, fresh, destinations) = replacement_fixture();
        fs::remove_file(&fresh[1]).unwrap();
        let mut service = FixtureRegistration {
            loaded: true,
            fail_next_bootstrap: false,
            events: vec![],
        };
        assert!(
            commit_payload(
                Replacement {
                    stage: &stage,
                    fresh: &fresh,
                    destinations: destinations.each_ref().map(PathBuf::as_path),
                    was_registered: true
                },
                Some(&mut service),
                |_| Ok(()),
                || panic!("incomplete payload must never be verified as installed")
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(destinations[0].join("code")).unwrap(),
            "old-app"
        );
        assert_eq!(fs::read_to_string(&destinations[1]).unwrap(), "old-1");
        assert_eq!(service.events, ["bootout", "bootstrap"]);
    }

    #[test]
    fn replaced_staging_inode_cannot_delete_unrelated_directory() {
        let (root, stage, _fresh, _destinations) = replacement_fixture();
        let metadata = fs::symlink_metadata(&stage).unwrap();
        let private = PrivateStage {
            path: stage.clone(),
            identity: (metadata.dev(), metadata.ino()),
        };
        fs::rename(&stage, root.path().join("original-stage")).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("keep"), "unrelated").unwrap();
        assert!(private.cleanup().is_err());
        assert_eq!(fs::read_to_string(stage.join("keep")).unwrap(), "unrelated");
    }

    #[test]
    fn driver_engine_is_on_demand_and_bound_to_the_user() {
        let account = crate::ServiceAccount::new(
            "artist".into(),
            "staff".into(),
            "/Users/artist & studio".into(),
        )
        .unwrap();
        let value = bound_definition(&account).unwrap();
        let dict = value.as_dictionary().unwrap();
        assert_eq!(dict["UserName"].as_string(), Some("artist"));
        assert_eq!(dict["KeepAlive"].as_boolean(), Some(false));
        assert_eq!(dict["RunAtLoad"].as_boolean(), Some(false));
        assert_eq!(
            dict["MachServices"].as_dictionary().unwrap()["io.mixrelay.audio.engine"].as_boolean(),
            Some(true)
        );
        let mut bytes = Vec::new();
        value.to_writer_xml(&mut bytes).unwrap();
        assert_eq!(Value::from_reader_xml(bytes.as_slice()).unwrap(), value);
        assert!(!String::from_utf8(bytes).unwrap().contains("__MIXRELAY"));
    }
}
