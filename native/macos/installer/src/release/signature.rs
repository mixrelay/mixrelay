#[cfg(target_os = "macos")]
use super::policy::verify_slices;
use super::{ReleaseError, policy::ArtifactRole};
use std::path::Path;

#[cfg(not(target_os = "macos"))]
pub(super) fn verify_signature(
    _path: &Path,
    _requirement: &str,
    _role: ArtifactRole,
) -> Result<(), ReleaseError> {
    Err(ReleaseError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
pub(super) fn verify_signature(
    path: &Path,
    requirement: &str,
    role: ArtifactRole,
) -> Result<(), ReleaseError> {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let literal_requirement = literal_requirement(requirement);
    let mut command = Command::new("/usr/bin/codesign");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .args([
            "--verify",
            "--deep",
            "--strict",
            "--all-architectures",
            "--test-requirement",
            &literal_requirement,
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    run_checked(&mut command, path, Duration::from_secs(30))?;
    verify_signature_metadata(path, role)
}

#[cfg(target_os = "macos")]
fn literal_requirement(requirement: &str) -> String {
    // Without '=' codesign interprets a requirement as a filename, not source.
    format!("={requirement}")
}

#[cfg(target_os = "macos")]
fn verify_signature_metadata(path: &Path, role: ArtifactRole) -> Result<(), ReleaseError> {
    let metadata_path = match role {
        ArtifactRole::Application => path.join("Contents/MacOS/mixrelay-desktop"),
        ArtifactRole::Hal => path.join("Contents/MacOS/MixRelayHAL"),
        ArtifactRole::Uninstaller => path.join("Contents/MacOS/mixrelay-driver-uninstaller"),
        ArtifactRole::Driver | ArtifactRole::Installer => path.to_path_buf(),
    };
    let slices = super::security_metadata::inspect_slices(&metadata_path, role)?;
    verify_slices(role, &slices)
        .map_err(|(architecture, violation)| super::policy_error(path, architecture, violation))
}

#[cfg(target_os = "macos")]
fn run_checked(
    command: &mut std::process::Command,
    path: &Path,
    timeout: std::time::Duration,
) -> Result<(), ReleaseError> {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let io_error = |source| ReleaseError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut owner = ChildOwner(command.spawn().map_err(io_error)?);
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = owner.0.try_wait().map_err(io_error)? {
            return if status.success() {
                Ok(())
            } else {
                Err(ReleaseError::Signature {
                    path: path.to_path_buf(),
                    status: status.to_string(),
                })
            };
        }
        if Instant::now() >= deadline {
            owner.0.kill().map_err(io_error)?;
            owner.0.wait().map_err(io_error)?;
            return Err(ReleaseError::Timeout(path.to_path_buf()));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "macos")]
struct ChildOwner(std::process::Child);

#[cfg(target_os = "macos")]
impl Drop for ChildOwner {
    fn drop(&mut self) {
        // Every early error/panic owns and reaps this exact child; no background
        // verifier or output-collection thread survives the transaction boundary.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{process::Command, time::Duration};

    #[test]
    fn requirement_is_compiled_as_source_not_interpreted_as_a_path() {
        assert_eq!(literal_requirement("anchor apple"), "=anchor apple");
    }

    #[test]
    fn native_child_success_failure_and_deadline_are_distinct() {
        run_checked(
            &mut Command::new("/usr/bin/true"),
            Path::new("success"),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(matches!(
            run_checked(
                &mut Command::new("/usr/bin/false"),
                Path::new("failure"),
                Duration::from_secs(1)
            ),
            Err(ReleaseError::Signature { .. })
        ));
        assert!(matches!(
            run_checked(
                Command::new("/bin/sleep").arg("60"),
                Path::new("timeout"),
                Duration::from_millis(30)
            ),
            Err(ReleaseError::Timeout(_))
        ));
    }
}
