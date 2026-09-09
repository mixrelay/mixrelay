use std::{
    io::{self, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use super::fixture::InteractiveFixture;
use super::{memory_child, memory_evidence, process};

const CHILD_NAME: &str = "tests::owned_child_fixture";
const CHILD_MODE: &str = "TONENARI_RUNNER_FIXTURE_MODE";
const TIMEOUT: Duration = Duration::from_millis(250);

fn child(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", CHILD_NAME, "--nocapture", "--test-threads=1"]);
    command.env(CHILD_MODE, mode);
    command
}

/// This Cargo test is also the owned subprocess fixture. Normal execution does
/// nothing; subprocess modes are selected only by this module's explicit env.
#[test]
fn owned_child_fixture() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    println!("fixture_pid={}", std::process::id());
    io::stdout().flush().expect("flush fixture identity");
    match mode.as_str() {
        "success" => {
            println!("fixture stdout");
            eprintln!("fixture stderr");
        }
        "failure" => std::process::exit(9),
        "hang" => loop {
            thread::sleep(Duration::from_secs(1));
        },
        "descendant" => {
            let mut descendant = child("hang").spawn().expect("owned descendant");
            println!("descendant_pid={}", descendant.id());
            io::stdout().flush().expect("flush descendant identity");
            let _ = descendant.wait();
        }
        "noisy" => loop {
            println!("{}", "x".repeat(8192));
        },
        other => panic!("unknown fixture mode: {other}"),
    }
}

#[test]
fn runner_captures_success_and_failure() {
    let success =
        process::run(&mut child("success"), Duration::from_secs(5)).expect("success runner");
    assert!(success.status.success());
    assert!(String::from_utf8_lossy(&success.stdout).contains("fixture stdout"));
    assert!(String::from_utf8_lossy(&success.stderr).contains("fixture stderr"));
    let failure =
        process::run(&mut child("failure"), Duration::from_secs(5)).expect("failure runner");
    assert_eq!(failure.status.code(), Some(9));
    assert_reaped(pid(
        &String::from_utf8_lossy(&failure.stdout),
        "fixture_pid=",
    ));
}

#[test]
fn runner_times_out_and_reaps_the_child() {
    let started = Instant::now();
    let error = process::run(&mut child("hang"), TIMEOUT).expect_err("hanging child must fail");
    assert!(error.contains("deadline exceeded"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_reaped(pid(&error, "fixture_pid="));
}

#[test]
fn runner_timeout_terminates_only_its_owned_process_group() {
    let error =
        process::run(&mut child("descendant"), TIMEOUT).expect_err("hanging descendant must fail");
    assert!(error.contains("deadline exceeded"), "{error}");
    assert_reaped(pid(&error, "fixture_pid="));
    assert_reaped(pid(&error, "descendant_pid="));
}

#[test]
fn runner_limits_diagnostics_without_hanging() {
    let error = process::run(&mut child("noisy"), Duration::from_secs(5))
        .expect_err("unbounded output must fail");
    assert!(
        error.contains("output exceeded 1 MiB"),
        "missing output limit failure"
    );
    assert_reaped(pid(&error, "fixture_pid="));
}

#[test]
fn runner_reports_launch_failures() {
    let error = process::run(
        &mut Command::new("/nonexistent-tonenari-owned-test-fixture"),
        TIMEOUT,
    )
    .expect_err("missing binary must fail");
    assert!(error.contains("cannot launch"));
}

#[test]
fn allocator_child_executes_real_assertions_and_reports_clean() {
    if memory_child("tests::allocator_child_executes_real_assertions_and_reports_clean") {
        let values: Vec<u32> = (0..256).collect();
        assert_eq!(values.iter().sum::<u32>(), 32640);
    }
}

#[test]
fn allocator_evidence_rejects_a_successful_but_filtered_child_process() {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", "tests::does_not_exist", "--nocapture"]);
    let output = process::run(&mut command, Duration::from_secs(5)).expect("filtered child");
    assert!(
        output.status.success(),
        "libtest itself accepts an unmatched filter"
    );
    assert!(!memory_evidence(
        "tests::does_not_exist",
        &String::from_utf8_lossy(&output.stdout),
        "0 leaks for 0 total leaked bytes"
    ));
}

fn pid(output: &str, prefix: &str) -> i32 {
    output
        .split(prefix)
        .nth(1)
        .and_then(|suffix| suffix.lines().next())
        .and_then(|value| value.parse().ok())
        .expect("fixture process ID")
}

fn assert_reaped(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal zero probes existence without changing process state.
        if unsafe { libc::kill(pid, 0) } == -1 {
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owned process {pid} survived cleanup"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn allocator_evidence_requires_one_executed_matching_child() {
    let report = "0 leaks for 0 total leaked bytes";
    let entered = "test tests::memory ... TONENARI_MEMORY_TEST_ENTERED tests::memory\n";
    let passed = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 53 filtered out; finished in 0.1s\n";
    assert!(memory_evidence(
        "tests::memory",
        &format!("{entered}{passed}"),
        report
    ));
    assert!(!memory_evidence(
        "tests::different",
        &format!("{entered}{passed}"),
        report
    ));
    assert!(!memory_evidence("tests::memory", passed, report));
    for summary in [
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 54 filtered out;",
        "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 53 filtered out;",
        "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 53 filtered out;",
        "test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out;",
    ] {
        assert!(!memory_evidence(
            "tests::memory",
            &format!("{entered}{summary}"),
            report
        ));
    }
    assert!(!memory_evidence(
        "tests::memory",
        &format!("{entered}{passed}"),
        "1 leaks for 4 total leaked bytes"
    ));
}

#[test]
fn interactive_handshake_timeout_still_closes_stdin_and_reaps() {
    // cat owns no external resource, waits for stdin, and exits normally on EOF.
    let mut session = InteractiveFixture::spawn(&mut Command::new("/bin/cat"), TIMEOUT, TIMEOUT)
        .expect("owned interactive fixture");
    let child_pid = i32::try_from(session.pid()).expect("process ID");
    let started = Instant::now();
    let error = session
        .handshake("fixture-ready")
        .expect_err("missing handshake must fail");
    assert!(error.contains("reply deadline exceeded"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    session
        .finish()
        .expect("quit and EOF must gracefully stop cat");
    assert_reaped(child_pid);
}

#[test]
fn interactive_malformed_reply_does_not_pass_or_leak_child() {
    let mut session = InteractiveFixture::spawn(&mut Command::new("/bin/cat"), TIMEOUT, TIMEOUT)
        .expect("owned interactive fixture");
    let child_pid = i32::try_from(session.pid()).expect("process ID");
    let error = session
        .command("disconnect")
        .expect_err("echo is not an ok response");
    assert!(error.contains("malformed fixture reply"), "{error}");
    session
        .finish()
        .expect("malformed protocol still receives quit and EOF");
    assert_reaped(child_pid);
}

#[test]
fn interactive_unresponsive_cleanup_is_bounded_and_reaped() {
    let mut session =
        InteractiveFixture::spawn(Command::new("/bin/sleep").arg("60"), TIMEOUT, TIMEOUT)
            .expect("owned unresponsive fixture");
    let child_pid = i32::try_from(session.pid()).expect("process ID");
    let started = Instant::now();
    let error = session
        .finish()
        .expect_err("child ignoring quit must fail cleanup");
    assert!(error.contains("cleanup deadline exceeded"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_reaped(child_pid);
}

#[test]
fn interactive_drop_reaps_an_unresponsive_child_during_unwind() {
    let session = InteractiveFixture::spawn(Command::new("/bin/sleep").arg("60"), TIMEOUT, TIMEOUT)
        .expect("owned unresponsive fixture");
    let child_pid = i32::try_from(session.pid()).expect("process ID");
    let failure = std::panic::catch_unwind(move || {
        let _owned = session;
        panic!("original fixture assertion");
    });
    assert!(failure.is_err());
    assert_reaped(child_pid);
}
