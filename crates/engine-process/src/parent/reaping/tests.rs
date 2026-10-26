//! Prepared reaper ownership when no process has been spawned.
use super::*;

#[test]
fn unused_worker_exits_when_its_handoff_closes() {
    let (reaper, observed) = Reaper::observe_exit().unwrap();
    drop(reaper);
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
}
