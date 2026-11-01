//! Prepared reaper ownership when no process has been spawned.
use super::*;

#[test]
fn unused_worker_exits_when_its_handoff_closes() {
    let (reaper, observed) = Reaper::observe_exit().unwrap();
    drop(reaper);
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
}

/// A child that exits after `exits_after` probes, or never; errors are scripted.
#[derive(Default)]
struct FakeChild {
    exits_after: Option<usize>,
    probes: usize,
    probe_error: bool,
    kill_error: bool,
    killed: bool,
}

impl StartupChild for FakeChild {
    fn has_exited(&mut self) -> io::Result<bool> {
        self.probes += 1;
        if self.probe_error {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "probe"));
        }
        Ok(self.exits_after.is_some_and(|probes| self.probes > probes))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.killed = true;
        if self.kill_error {
            return Err(io::Error::other("kill"));
        }
        Ok(())
    }
}

fn clock() -> mixrelay_time::ManualClock {
    mixrelay_time::ManualClock::new(mixrelay_time::Instant::from_origin(Duration::ZERO))
}

#[test]
fn a_child_that_leaves_within_the_grace_is_not_killed() {
    let clock = clock();
    let mut child = FakeChild {
        exits_after: Some(3),
        ..FakeChild::default()
    };
    drain_or_kill(&mut child, &clock).unwrap();
    assert!(!child.killed);
    assert_eq!(
        clock
            .now()
            .duration_since(mixrelay_time::Instant::from_origin(Duration::ZERO)),
        READY_EXIT_POLL * 3
    );
}

#[test]
fn a_child_still_running_at_the_grace_deadline_is_killed_then() {
    let clock = clock();
    let mut child = FakeChild::default();
    drain_or_kill(&mut child, &clock).unwrap();
    assert!(child.killed);
    assert_eq!(
        clock.now(),
        mixrelay_time::Instant::from_origin(STARTUP_FAILURE_GRACE)
    );
}

#[test]
fn a_failed_exit_probe_kills_at_once_and_reports_both_failures() {
    let clock = clock();
    let mut child = FakeChild {
        probe_error: true,
        kill_error: true,
        ..FakeChild::default()
    };
    let error = drain_or_kill(&mut child, &clock).unwrap_err();
    assert!(child.killed);
    assert_eq!(child.probes, 1);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "probe; kill owned startup child: kill");
}

#[test]
fn a_failed_kill_is_reported() {
    let mut child = FakeChild {
        kill_error: true,
        ..FakeChild::default()
    };
    let error = drain_or_kill(&mut child, &clock()).unwrap_err();
    assert_eq!(error.to_string(), "kill owned startup child: kill");
}
