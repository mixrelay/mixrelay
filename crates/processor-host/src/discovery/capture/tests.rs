use super::*;
use std::io::Write;

fn fixture(unix: &str, windows: &str) -> Command {
    #[cfg(windows)]
    {
        let _ = unix;
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", windows]);
        command
    }
    #[cfg(not(windows))]
    {
        let _ = windows;
        let mut command = Command::new("/bin/sh");
        command.args(["-c", unix]);
        command
    }
}

#[test]
fn empty_json_succeeds_but_invalid_json_and_child_exit_are_errors() {
    let success = fixture("printf '[]'", "[Console]::Out.Write('[]')");
    assert_eq!(
        capture_json_child::<Vec<String>>(success, Duration::from_secs(5)).unwrap(),
        Vec::<String>::new()
    );
    let invalid = fixture("printf 'broken'", "[Console]::Out.Write('broken')");
    assert!(
        capture_json_child::<Vec<String>>(invalid, Duration::from_secs(5))
            .unwrap_err()
            .starts_with("invalid child JSON:")
    );
    let exit = fixture(
        "printf 'probe rejected' >&2; exit 7",
        "[Console]::Error.Write('probe rejected'); exit 7",
    );
    let error = capture_json_child::<Vec<String>>(exit, Duration::from_secs(5)).unwrap_err();
    assert!(error.contains("child exited with"), "{error}");
    assert!(error.contains('7'), "{error}");
    assert!(error.contains("probe rejected"), "{error}");
}

#[test]
fn timeout_kills_and_reaps_direct_child_without_waiting_for_stdout_eof() {
    let command = fixture("exec sleep 5", "Start-Sleep -Seconds 5");
    let start = Instant::now();
    let error = capture_json_child::<Vec<String>>(command, Duration::from_millis(50)).unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "direct-child timeout must stay bounded"
    );
}

#[cfg(unix)]
#[test]
fn invalid_utf8_inside_json_is_rejected_instead_of_repaired() {
    let command = fixture("printf '[\"\\377\"]'", "");
    let error = capture_json_child::<Vec<String>>(command, Duration::from_secs(5)).unwrap_err();
    assert!(error.starts_with("invalid child JSON:"), "{error}");
}

#[cfg(unix)]
#[test]
fn inherited_stdout_does_not_hold_discovery_open() {
    // The finite child intentionally inherits capture handles after the scanner exits. This
    // used to hang reader.join() beyond the scanner timeout; no reader thread exists now.
    let command = fixture("sleep 1 & printf '[]'", "");
    let start = Instant::now();
    assert!(
        capture_json_child::<Vec<String>>(command, Duration::from_millis(100))
            .unwrap()
            .is_empty()
    );
    assert!(start.elapsed() < Duration::from_millis(700));
}

#[test]
fn output_limits_and_capture_file_cleanup_are_explicit() {
    let mut capture = CaptureFile::new().unwrap();
    let path = capture.path.clone();
    capture
        .file
        .as_mut()
        .unwrap()
        .write_all(b"0123456789")
        .unwrap();
    assert!(
        capture
            .check_size(9)
            .unwrap_err()
            .contains("exceeds 9 bytes")
    );
    assert_eq!(capture.read(4).unwrap(), b"0123");
    drop(capture);
    assert!(
        !path.exists(),
        "owned capture files are removed on scope exit"
    );

    let missing_directory = tonenari_test_support::FixtureDir::new("missing-scanner").unwrap();
    let missing = Command::new(missing_directory.join("no-such-scanner"));
    assert!(
        capture_json_child::<Vec<String>>(missing, Duration::from_millis(50))
            .unwrap_err()
            .starts_with("child spawn failed:")
    );
}

#[test]
fn excessive_child_output_is_rejected_but_failure_exit_keeps_its_reason() {
    let oversized = fixture(
        "printf '%016385d' 0 >&2; printf '[]'",
        "[Console]::Error.Write(('x' * 16385)); [Console]::Out.Write('[]')",
    );
    let error = capture_json_child::<Vec<String>>(oversized, Duration::from_secs(5)).unwrap_err();
    assert!(error.contains("exceeds 16384 bytes"), "{error}");
    let crashed = fixture(
        "printf 'rejected ' >&2; printf '%016385d' 0 >&2; exit 7",
        "[Console]::Error.Write('rejected ' + ('x' * 16385)); exit 7",
    );
    let error = capture_json_child::<Vec<String>>(crashed, Duration::from_secs(5)).unwrap_err();
    // If a running process crosses the size cap first it is killed; if already exited, its
    // nonzero status remains authoritative rather than being relabeled as invalid JSON.
    assert!(
        error.contains("exceeds 16384 bytes")
            || (error.contains("child exited with") && error.contains("rejected")),
        "{error}"
    );
    assert!(!error.contains("invalid child JSON"));
}
