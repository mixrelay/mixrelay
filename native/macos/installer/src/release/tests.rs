use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

struct Fixture {
    _root: mixrelay_test_support::FixtureDir,
    root: PathBuf,
    sources: InstallSources,
}

impl Fixture {
    fn new() -> Self {
        let root_owner = mixrelay_test_support::FixtureDir::new("macos-release").unwrap();
        let root = root_owner.path().to_path_buf();
        let sources = InstallSources::new(
            root.join("app"),
            root.join("hal"),
            root.join("driver"),
            root.join("launchd"),
        );
        for (bundle, identifier, executable) in [
            (
                sources.app(),
                "io.mixrelay.driver.desktop",
                "mixrelay-desktop",
            ),
            (sources.hal(), "io.mixrelay.audio.driver", "MixRelayHAL"),
        ] {
            fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
            let audio_capture_description = if identifier == "io.mixrelay.driver.desktop" {
                "<key>NSAudioCaptureUsageDescription</key><string>MixRelay captures system audio when you enable it.</string><key>NSMicrophoneUsageDescription</key><string>MixRelay accesses selected audio inputs.</string>"
            } else {
                ""
            };
            fs::write(bundle.join("Contents/Info.plist"), format!("<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleExecutable</key><string>{executable}</string>{audio_capture_description}</dict></plist>")).unwrap();
            fs::write(
                bundle.join("Contents/MacOS").join(executable),
                crate::universal::tests::fixture(false),
            )
            .unwrap();
            fs::set_permissions(
                bundle.join("Contents/MacOS").join(executable),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        fs::write(sources.driver(), crate::universal::tests::fixture(true)).unwrap();
        fs::set_permissions(sources.driver(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            sources.launchd(),
            include_bytes!("../../../../../packaging/macos/io.mixrelay.audio.driver.plist.in"),
        )
        .unwrap();
        Self {
            _root: root_owner,
            root,
            sources,
        }
    }
}

fn verifier() -> ReleasePayloadVerifier {
    ReleasePayloadVerifier::new(
        SigningTeam::new("ABCD123456".into()).unwrap(),
        ServiceAccount::new("listener".into(), "staff".into(), "/Users/listener".into()).unwrap(),
    )
}

#[test]
fn binds_all_three_code_identities_and_team_without_shell_interpolation() {
    let fixture = Fixture::new();
    let mut checks = Vec::new();
    verifier()
        .verify_with(&fixture.sources, true, |path, requirement, role| {
            checks.push((path.to_path_buf(), requirement.to_owned(), role));
            Ok(())
        })
        .unwrap();
    assert_eq!(checks.len(), 3);
    for ((_, requirement, role), (identifier, expected_role)) in checks.iter().zip([
        ("io.mixrelay.driver.desktop", ArtifactRole::Application),
        ("io.mixrelay.audio.driver", ArtifactRole::Hal),
        ("mixrelay-driver-service", ArtifactRole::Driver),
    ]) {
        assert!(requirement.contains(&format!("identifier \"{identifier}\"")));
        assert!(requirement.contains("anchor apple generic"));
        assert!(requirement.contains("ABCD123456"));
        assert!(requirement.contains("1.2.840.113635.100.6.1.13"));
        assert_eq!(*role, expected_role);
    }
}

#[test]
fn invalid_team_and_template_fail_before_native_code_checks() {
    for team in ["", "short", "abcdefghij", "ABCD12345\"", "ABCD12345;"] {
        assert!(SigningTeam::new(team.into()).is_err());
    }
    let fixture = Fixture::new();
    fs::write(fixture.sources.launchd(), b"not a plist").unwrap();
    assert!(
        verifier()
            .verify_with(&fixture.sources, true, |_, _, _| panic!(
                "signature should not run"
            ))
            .is_err()
    );
}

#[test]
fn relative_paths_cannot_be_interpreted_as_codesign_options() {
    let sources = InstallSources::new("--help", "/hal", "/driver", "/launchd");
    assert!(matches!(
        verifier().verify_with(&sources, true, |_, _, _| panic!(
            "must reject before command"
        )),
        Err(ReleaseError::SourcePath(_))
    ));
}

#[test]
fn signature_failure_short_circuits_the_remaining_payload() {
    let fixture = Fixture::new();
    let mut calls = 0;
    let result = verifier().verify_with(&fixture.sources, true, |path, _, _| {
        calls += 1;
        Err(ReleaseError::Signature {
            path: path.to_path_buf(),
            status: "rejected".into(),
        })
    });
    assert!(matches!(result, Err(ReleaseError::Signature { .. })));
    assert_eq!(calls, 1);
}

#[test]
fn wrong_bundle_identity_and_thin_binary_are_rejected_after_signature() {
    let fixture = Fixture::new();
    let info = fixture.sources.hal().join("Contents/Info.plist");
    let original = fs::read_to_string(&info).unwrap();
    fs::write(
        &info,
        original.replace("io.mixrelay.audio.driver", "io.unrelated.driver"),
    )
    .unwrap();
    assert!(matches!(
        verifier().verify_with(&fixture.sources, true, |_, _, _| Ok(())),
        Err(ReleaseError::BundleIdentity(_))
    ));
    fs::write(info, original).unwrap();
    fs::write(fixture.sources.driver(), b"not universal").unwrap();
    assert!(matches!(
        verifier().verify_with(&fixture.sources, true, |_, _, _| Ok(())),
        Err(ReleaseError::Universal { .. })
    ));
}

#[test]
fn driver_manager_does_not_request_graph_capture_permissions() {
    let fixture = Fixture::new();
    let info = fixture.sources.app().join("Contents/Info.plist");
    let original = fs::read_to_string(&info).unwrap();
    let trimmed = original.replace(
        "<key>NSAudioCaptureUsageDescription</key><string>MixRelay captures system audio when you enable it.</string><key>NSMicrophoneUsageDescription</key><string>MixRelay accesses selected audio inputs.</string>",
        "",
    );
    fs::write(info, trimmed).unwrap();
    assert!(
        verifier()
            .verify_with(&fixture.sources, true, |_, _, _| Ok(()))
            .is_ok()
    );
}

#[test]
fn installer_helper_uses_fixed_team_identity_and_entitlement_free_role() {
    let fixture = Fixture::new();
    let mut observed = None;
    verifier()
        .verify_installer_executable_with(fixture.sources.driver(), |path, requirement, role| {
            observed = Some((path.to_path_buf(), requirement.to_owned(), role));
            Ok(())
        })
        .unwrap();
    let (path, requirement, role) = observed.expect("signature verifier should be called");
    assert_eq!(path, fixture.sources.driver());
    assert!(requirement.contains("identifier \"mixrelay-macos-installer\""));
    assert!(requirement.contains("ABCD123456"));
    assert_eq!(role, ArtifactRole::Installer);
}

#[test]
fn installer_helper_rejects_relative_paths_before_signature_verification() {
    assert!(matches!(
        verifier().verify_installer_executable_with(Path::new("helper"), |_, _, _| {
            panic!("relative path must be rejected first")
        }),
        Err(ReleaseError::SourcePath(_))
    ));
}

#[test]
fn configuration_reader_rejects_symlinks_directories_and_oversized_files() {
    let fixture = Fixture::new();
    assert!(read_configuration(&fixture.root, 128).is_err());
    let alias = fixture.root.join("alias");
    symlink(fixture.sources.launchd(), &alias).unwrap();
    assert!(read_configuration(&alias, 16 * 1024).is_err());
    assert!(read_configuration(fixture.sources.launchd(), 8).is_err());
    assert!(read_configuration(fixture.sources.launchd(), 16 * 1024).is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn native_verifier_rejects_unsigned_code_instead_of_accepting_local_builds() {
    let fixture = Fixture::new();
    assert!(matches!(
        verifier().verify_sources(&fixture.sources),
        Err(ReleaseError::Signature { .. })
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn native_stage_verification_checks_private_copies_even_when_sources_are_removed() {
    use crate::{FilesystemAdapter, FilesystemLayout, LIFECYCLE_DIRECTORY, TransactionRequest};
    let fixture = Fixture::new();
    fs::create_dir_all(
        fixture
            .root
            .join(LIFECYCLE_DIRECTORY.trim_start_matches('/')),
    )
    .unwrap();
    let adapter = FilesystemAdapter::new(FilesystemLayout::new(&fixture.root).unwrap());
    let request = TransactionRequest::install(fixture.sources.clone());
    let snapshot = adapter.preflight(&request).unwrap();
    let staged = adapter.stage(&request, &snapshot).unwrap();
    fs::remove_dir_all(fixture.sources.app()).unwrap();
    fs::remove_dir_all(fixture.sources.hal()).unwrap();
    fs::remove_file(fixture.sources.driver()).unwrap();
    fs::remove_file(fixture.sources.launchd()).unwrap();
    match verifier().verify_staged(&staged) {
        Err(ReleaseError::Signature { path, .. }) => {
            assert_eq!(path, staged.payload_path(OwnedTarget::App));
        }
        result => panic!("expected signature rejection of the private copy: {result:?}"),
    }
    adapter.cleanup(&staged).unwrap();
}
