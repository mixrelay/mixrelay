//! Release payload validation shared by preflight and private staged copies.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use rustix::fs::{Mode, OFlags};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    FilesystemStaged, InstallSources, LaunchdConfiguration, LaunchdError, OwnedTarget,
    ServiceAccount, UniversalError, validate_universal_binary,
};

mod policy;
#[cfg(target_os = "macos")]
mod security_metadata;
mod signature;
use policy::ArtifactRole;
#[cfg(target_os = "macos")]
use policy::{Architecture, PolicyViolation};
use signature::verify_signature;

/// The publisher identity required for every executable in a release payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigningTeam(String);

impl SigningTeam {
    /// Validates a ten-character Apple Developer Team identifier.
    /// # Errors
    /// Returns invalid syntax. This does not assert membership or certificate availability.
    pub fn new(value: String) -> Result<Self, ReleaseError> {
        if value.len() != 10
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(ReleaseError::SigningTeam);
        }
        Ok(Self(value))
    }

    fn requirement(&self, identifier: &str) -> String {
        // Identifiers come only from the fixed artifact policy below. Team syntax
        // excludes requirement-language metacharacters; no shell is invoked.
        format!(
            "anchor apple generic and identifier \"{identifier}\" and certificate leaf[subject.OU] = \"{}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists",
            self.0
        )
    }
}

/// Native release payload failure; no installed artifact is changed by verification.
#[derive(Debug, Error)]
pub enum ReleaseError {
    /// Invalid publisher identifier.
    #[error("expected a ten-character uppercase Apple Developer Team identifier")]
    SigningTeam,
    /// A source argument could be interpreted as an option or ambiguous path.
    #[error("release source must be an absolute path: {0}")]
    SourcePath(PathBuf),
    /// File access, launch or child reaping failed.
    #[error("release verification I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Input is not a bounded regular configuration file.
    #[error("invalid or oversized release configuration: {0}")]
    ConfigurationFile(PathBuf),
    /// The application/HAL identity or executable differs from fixed policy.
    #[error("unexpected bundle identifier or executable: {0}")]
    BundleIdentity(PathBuf),
    /// Bundle metadata could not be decoded.
    #[error("invalid bundle metadata: {0}")]
    BundlePlist(#[from] plist::Error),
    /// A binary did not contain the required architecture slices.
    #[error("invalid Universal 2 binary {path}: {source}")]
    Universal {
        path: PathBuf,
        #[source]
        source: UniversalError,
    },
    /// The launchd payload does not match fixed policy.
    #[error(transparent)]
    Launchd(#[from] LaunchdError),
    /// Apple rejected integrity, architecture or publisher identity.
    #[error("Developer ID signature verification failed for {path}: {status}")]
    Signature { path: PathBuf, status: String },
    /// Structured signing metadata did not satisfy a release artifact policy.
    #[error("invalid {architecture} signing metadata for {path}: {reason}")]
    SignaturePolicy {
        path: PathBuf,
        architecture: &'static str,
        reason: &'static str,
    },
    /// Apple Security.framework could not read a specific architecture's signing metadata.
    #[error("could not inspect {architecture} signing metadata for {path}: {status}")]
    SignatureMetadata {
        path: PathBuf,
        architecture: &'static str,
        status: String,
    },
    /// A verification subprocess exceeded its deadline and was reaped.
    #[error("signature verification timed out for {0}")]
    Timeout(PathBuf),
    /// Native Apple signature checking is unavailable on this OS.
    #[error("Developer ID verification requires macOS")]
    UnsupportedPlatform,
}

/// Verifies actual copies using Apple signature checking and fixed product policy.
///
/// This never signs code, registers services, modifies installed files, or acquires
/// privileges. Native install callers still establish root ownership and validate
/// the selected account against the OS before using the returned launch definition.
#[derive(Debug)]
pub struct ReleasePayloadVerifier {
    team: SigningTeam,
    account: ServiceAccount,
}

impl ReleasePayloadVerifier {
    /// Binds one trusted publisher and OS-resolved service identity.
    #[must_use]
    pub const fn new(team: SigningTeam, account: ServiceAccount) -> Self {
        Self { team, account }
    }

    /// Verifies explicitly supplied artifacts, returning a bound launch definition.
    ///
    /// This preflight is not proof that later copies are identical. Always verify
    /// the private copies again through [`Self::verify_staged`] before committing.
    /// # Errors
    /// Returns invalid signatures, architectures, bundle identities or launch policy.
    pub fn verify_sources(
        &self,
        sources: &InstallSources,
    ) -> Result<LaunchdConfiguration, ReleaseError> {
        self.verify_with(sources, true, verify_signature)
    }

    /// Verifies the currently installed app, HAL, driver and account-bound plist.
    ///
    /// Removal must establish the same publisher identity and Universal 2 policy
    /// before treating an existing target as owned. The plist is already bound to
    /// the OS-resolved account, unlike an install source template.
    /// # Errors
    /// Returns invalid signatures, architectures, bundle identities or launch policy.
    pub fn verify_installed(
        &self,
        app: impl Into<PathBuf>,
        hal: impl Into<PathBuf>,
        driver: impl Into<PathBuf>,
        launchd: impl Into<PathBuf>,
    ) -> Result<LaunchdConfiguration, ReleaseError> {
        self.verify_with(
            &InstallSources::new(app, hal, driver, launchd),
            false,
            verify_signature,
        )
    }

    /// Verifies private install copies, not the original source paths.
    /// # Errors
    /// Returns absent/removal payloads or failed signature, format or policy checks.
    pub fn verify_staged(
        &self,
        staged: &FilesystemStaged,
    ) -> Result<LaunchdConfiguration, ReleaseError> {
        self.verify_sources(&InstallSources::new(
            staged.payload_path(OwnedTarget::App),
            staged.payload_path(OwnedTarget::Hal),
            staged.payload_path(OwnedTarget::Driver),
            staged.payload_path(OwnedTarget::Launchd),
        ))
    }

    /// Verifies the signed universal installer helper before it is embedded in a package.
    ///
    /// This requires the helper's fixed Developer ID identity and the same strict signing
    /// policy as the application and HAL: both slices are hardened and timestamped, and
    /// neither slice may carry entitlements.
    /// # Errors
    /// Returns an invalid path, architecture, signature, signing policy, or Security.framework
    /// metadata error.
    pub fn verify_installer_executable(&self, path: impl AsRef<Path>) -> Result<(), ReleaseError> {
        self.verify_installer_executable_with(path.as_ref(), verify_signature)
    }

    /// Verifies the fixed uninstaller bundle before trusting its publisher metadata.
    ///
    /// Both executable slices must meet the installer signing policy. The bundle's
    /// resource seal also binds its embedded helper and `MixRelaySigningTeam` plist entry.
    /// # Errors
    /// Returns an invalid path, bundle identity, architecture, or Developer ID signature.
    pub fn verify_uninstaller_bundle(&self, path: impl AsRef<Path>) -> Result<(), ReleaseError> {
        self.verify_uninstaller_bundle_with(path.as_ref(), verify_signature)
    }

    fn verify_uninstaller_bundle_with(
        &self,
        path: &Path,
        signature: impl FnOnce(&Path, &str, ArtifactRole) -> Result<(), ReleaseError>,
    ) -> Result<(), ReleaseError> {
        if !path.is_absolute() {
            return Err(ReleaseError::SourcePath(path.to_path_buf()));
        }
        verify_bundle(
            path,
            "io.mixrelay.driver.uninstaller",
            "mixrelay-driver-uninstaller",
        )?;
        signature(
            path,
            &self.team.requirement("io.mixrelay.driver.uninstaller"),
            ArtifactRole::Installer,
        )
    }

    fn verify_installer_executable_with(
        &self,
        path: &Path,
        signature: impl FnOnce(&Path, &str, ArtifactRole) -> Result<(), ReleaseError>,
    ) -> Result<(), ReleaseError> {
        if !path.is_absolute() {
            return Err(ReleaseError::SourcePath(path.to_path_buf()));
        }
        verify_architecture(path)?;
        signature(
            path,
            &self.team.requirement("mixrelay-macos-installer"),
            ArtifactRole::Installer,
        )
    }

    fn verify_with(
        &self,
        sources: &InstallSources,
        is_template: bool,
        mut signature: impl FnMut(&Path, &str, ArtifactRole) -> Result<(), ReleaseError>,
    ) -> Result<LaunchdConfiguration, ReleaseError> {
        for path in [
            sources.app(),
            sources.hal(),
            sources.driver(),
            sources.launchd(),
        ] {
            if !path.is_absolute() {
                return Err(ReleaseError::SourcePath(path.to_path_buf()));
            }
        }
        let launchd = read_configuration(sources.launchd(), 16 * 1024)?;
        let definition = if is_template {
            LaunchdConfiguration::from_template(&launchd, &self.account)?
        } else {
            LaunchdConfiguration::validate_installed(&launchd, &self.account)?
        };
        for (bundle, identifier, executable, role) in [
            (
                sources.app(),
                "io.mixrelay.driver.desktop",
                "mixrelay-desktop",
                ArtifactRole::Application,
            ),
            (
                sources.hal(),
                "io.mixrelay.audio.driver",
                "MixRelayHAL",
                ArtifactRole::Hal,
            ),
        ] {
            signature(bundle, &self.team.requirement(identifier), role)?;
            verify_bundle(bundle, identifier, executable)?;
        }
        signature(
            sources.driver(),
            &self.team.requirement("mixrelay-driver-service"),
            ArtifactRole::Driver,
        )?;
        verify_architecture(sources.driver())?;
        Ok(definition)
    }
}

#[derive(Deserialize)]
struct BundleInfo {
    #[serde(rename = "CFBundleIdentifier")]
    identifier: String,
    #[serde(rename = "CFBundleExecutable")]
    executable: String,
}

fn verify_bundle(bundle: &Path, identifier: &str, executable: &str) -> Result<(), ReleaseError> {
    let bytes = read_configuration(&bundle.join("Contents/Info.plist"), 1024 * 1024)?;
    let info: BundleInfo = plist::from_bytes(&bytes)?;
    if info.identifier != identifier || info.executable != executable {
        return Err(ReleaseError::BundleIdentity(bundle.to_path_buf()));
    }
    verify_architecture(&bundle.join("Contents/MacOS").join(executable))
}

fn verify_architecture(path: &Path) -> Result<(), ReleaseError> {
    validate_universal_binary(path).map_err(|source| ReleaseError::Universal {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(target_os = "macos")]
fn policy_error(
    path: &Path,
    architecture: Architecture,
    violation: PolicyViolation,
) -> ReleaseError {
    let reason = match violation {
        PolicyViolation::ArchitectureSet => "expected exactly arm64 and x86_64 signing slices",
        PolicyViolation::HardenedRuntime => "Hardened Runtime is not enabled",
        PolicyViolation::SecureTimestamp => "secure signing timestamp is missing",
        PolicyViolation::Entitlements => "embedded entitlements differ from the artifact allowlist",
    };
    ReleaseError::SignaturePolicy {
        path: path.to_path_buf(),
        architecture: architecture.name(),
        reason,
    }
}

fn read_configuration(path: &Path, limit: u64) -> Result<Vec<u8>, ReleaseError> {
    let read = || -> io::Result<Vec<u8>> {
        let descriptor = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let file = File::from(descriptor);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a bounded regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = read().map_err(|source| ReleaseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.len() as u64 > limit {
        return Err(ReleaseError::ConfigurationFile(path.to_path_buf()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod uninstaller_tests {
    use super::*;
    use std::fs;

    #[test]
    fn uninstaller_bundle_binds_fixed_identity_and_installer_signature_policy() {
        let fixture = mixrelay_test_support::FixtureDir::new("macos-uninstaller").unwrap();
        let bundle = fixture.path().join("Uninstall MixRelay Driver.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            "<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key>\
             <string>io.mixrelay.driver.uninstaller</string><key>CFBundleExecutable</key>\
             <string>mixrelay-driver-uninstaller</string></dict></plist>",
        )
        .unwrap();
        fs::write(
            bundle.join("Contents/MacOS/mixrelay-driver-uninstaller"),
            crate::universal::tests::fixture(true),
        )
        .unwrap();
        let verifier = ReleasePayloadVerifier::new(
            SigningTeam::new("ABCD123456".to_owned()).unwrap(),
            ServiceAccount::new(
                "tester".to_owned(),
                "staff".to_owned(),
                "/Users/tester".into(),
            )
            .unwrap(),
        );
        verifier
            .verify_uninstaller_bundle_with(&bundle, |path, requirement, role| {
                assert_eq!(path, bundle);
                assert!(requirement.contains("identifier \"io.mixrelay.driver.uninstaller\""));
                assert!(requirement.contains("ABCD123456"));
                assert_eq!(role, ArtifactRole::Installer);
                Ok(())
            })
            .unwrap();
        assert!(matches!(
            verifier.verify_uninstaller_bundle_with(Path::new("relative.app"), |_, _, _| {
                panic!("relative paths cannot reach signature verification")
            }),
            Err(ReleaseError::SourcePath(_))
        ));
        fs::write(
            bundle.join("Contents/Info.plist"),
            "<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key>\
             <string>io.mixrelay.desktop</string><key>CFBundleExecutable</key>\
             <string>mixrelay-driver-uninstaller</string></dict></plist>",
        )
        .unwrap();
        assert!(matches!(
            verifier.verify_uninstaller_bundle_with(&bundle, |_, _, _| {
                panic!("wrong bundle identities cannot reach signature verification")
            }),
            Err(ReleaseError::BundleIdentity(_))
        ));
    }
}
