//! Cargo-owned fixtures, used only as a macOS dev-dependency. Native C tests
//! exercise the actual FFI modules; Rust assertions exercise production Rust.
#![cfg(target_os = "macos")]
use std::{process::Command, time::Duration};

mod fixture;
#[cfg(test)]
mod interactive_fixture;
mod private_device;
mod process;
pub use fixture::DeviceFixture;
#[cfg(test)]
mod tests;

const NATIVE_TIMEOUT: Duration = Duration::from_mins(1);
const MEMORY_TIMEOUT: Duration = Duration::from_mins(2);

#[must_use]
pub fn device_fixture() -> &'static str {
    env!("TN_NATIVE_FIXTURE")
}

/// Runs native callback and HAL assertions with `ASan` and `UBSan` enabled.
/// # Panics
/// A native assertion, sanitizer diagnostic or launch failure fails the test.
pub fn assert_native_contracts() {
    for path in [
        env!("TN_NATIVE_CLOCK"),
        env!("TN_NATIVE_HAL"),
        env!("TN_NATIVE_APPLICATION_CLOCK"),
        env!("TN_NATIVE_AUDIO_RING"),
        env!("TN_NATIVE_PCM_RING"),
    ] {
        // UBSan normally recovers after diagnostics; tests must instead fail on
        // the first violation, just as ASan and native assertions do.
        let result = process::run(
            Command::new(path)
                .env("UBSAN_OPTIONS", "halt_on_error=1:print_stacktrace=1")
                .env("ASAN_OPTIONS", "halt_on_error=1"),
            NATIVE_TIMEOUT,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        eprintln!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.status.success(),
            "native test failed: {path}: {}",
            result.status
        );
    }
}

/// Isolates allocator evidence and global owner counters from concurrent tests.
/// Returns true only in the child where the actual Rust assertions run.
/// # Panics
/// Missing diagnostics, failed child assertions or a non-clean report fail.
#[must_use]
pub fn memory_child(test_name: &str) -> bool {
    assert!(
        !test_name.is_empty(),
        "allocator test name must not be empty"
    );
    if std::env::var("TONENARI_MEMORY_TEST_CHILD").as_deref() == Ok(test_name) {
        println!("TONENARI_MEMORY_TEST_ENTERED {test_name}");
        return true;
    }
    let result = process::run(
        Command::new("/usr/bin/leaks")
            .args(["--atExit", "--"])
            .arg(std::env::current_exe().expect("test executable"))
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env("TONENARI_MEMORY_TEST_CHILD", test_name),
        MEMORY_TIMEOUT,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    eprintln!("{stdout}{stderr}");
    assert!(
        result.status.success() && memory_evidence(test_name, &stdout, &stderr),
        "allocator diagnostic failed: {test_name}"
    );
    false
}

fn memory_evidence(test_name: &str, stdout: &str, stderr: &str) -> bool {
    let entered = format!("TONENARI_MEMORY_TEST_ENTERED {test_name}");
    stdout.lines().any(|line| line.ends_with(&entered))
        && stdout.lines().any(|line| {
            line.strip_prefix("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ")
                .and_then(|rest| rest.split_once(" filtered out;"))
                .is_some_and(|(count, _)| count.parse::<usize>().is_ok())
        })
        && (stdout.contains("0 leaks for 0 total leaked bytes")
            || stderr.contains("0 leaks for 0 total leaked bytes"))
}
