use super::*;
use std::{sync::Arc, thread, time::Instant};

const PERIOD: Duration = Duration::from_micros(2_667);

#[test]
fn timebase_conversions_round_up_and_invert() {
    let timebase = Timebase::current();
    let ticks = timebase.ticks(Duration::from_millis(1));
    let back = timebase.nanoseconds(ticks);
    assert!((1_000_000..1_000_000 + 100).contains(&back), "{back}");
    assert_eq!(timebase.ticks(Duration::ZERO), 0);
    let start = host_time();
    wait_until(start + timebase.ticks(Duration::from_millis(2)));
    assert!(timebase.nanoseconds(host_time() - start) >= 2_000_000);
}

#[test]
fn realtime_budgets_are_validated_before_the_kernel_sees_them() {
    for (period, computation, constraint) in [
        (
            Duration::ZERO,
            Duration::from_micros(1),
            Duration::from_micros(1),
        ),
        (PERIOD, Duration::ZERO, PERIOD),
        (PERIOD, PERIOD, PERIOD / 2),
        (PERIOD, PERIOD / 4, PERIOD * 2),
        (
            Duration::from_mins(10),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ),
    ] {
        let error = promote_current_thread(period, computation, constraint).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

#[test]
fn a_promoted_thread_reports_its_applied_policy_and_runs_intervals() {
    thread::spawn(|| {
        let policy =
            promote_current_thread(PERIOD, PERIOD * 2 / 5, PERIOD * 4 / 5).expect("promote");
        assert_eq!(policy.period_ns.div_ceil(1_000), 2_667);
        // The kernel may raise the computation budget, never past the constraint.
        assert!(
            policy.computation_ns >= 1_066_000 && policy.computation_ns <= policy.constraint_ns
        );
        let interval = WorkInterval::create(c"mixrelay.test").expect("interval");
        let joined = interval.join().expect("join");
        let timebase = Timebase::current();
        let start = host_time();
        let deadline = start + timebase.ticks(PERIOD);
        joined
            .start(start, deadline)
            .expect("start")
            .finish()
            .expect("finish");
        // A cycle abandoned by an early return still finishes, so the next one starts.
        drop(joined.start(start, deadline).expect("start"));
        let cycle = joined.start(start, deadline).expect("restart after drop");
        // Only one interval runs at a time.
        assert!(joined.start(start, deadline).is_err());
        cycle.finish().expect("finish");
    })
    .join()
    .unwrap();
}

#[test]
fn notifications_coalesce_and_wake_a_waiting_consumer() {
    let notifier = Arc::new(Notifier::new().unwrap());
    // Publications before the wait leave one token: one wake, then a timeout.
    for _ in 0..3 {
        notifier.notify();
    }
    assert_eq!(
        notifier.wait(Duration::from_millis(50)).unwrap(),
        Wake::Notified
    );
    assert_eq!(
        notifier.wait(Duration::from_millis(5)).unwrap(),
        Wake::TimedOut
    );
    let producer = Arc::clone(&notifier);
    let started = Instant::now();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        producer.notify();
    });
    assert_eq!(
        notifier.wait(Duration::from_secs(5)).unwrap(),
        Wake::Notified
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    handle.join().unwrap();
}

#[test]
fn kernel_and_os_failures_are_reported_not_ignored() {
    let timebase = Timebase::current();
    let applied = TimeConstraintPolicy {
        period: 1,
        computation: 1,
        constraint: 1,
        preemptible: 1,
    };
    for (codes, defaulted) in [
        ((5, KERN_SUCCESS), false),
        ((KERN_SUCCESS, 5), false),
        ((KERN_SUCCESS, KERN_SUCCESS), true),
    ] {
        assert!(applied_policy(timebase, codes, defaulted, applied).is_err());
    }
    assert!(WorkInterval::adopt(std::ptr::null_mut()).is_err());
    assert_eq!(os(22).unwrap_err().raw_os_error(), Some(22));
    assert!(wake_result(5).is_err());
    assert_eq!(wake_result(KERN_ABORTED).unwrap(), Wake::TimedOut);
    assert!(kern(5).unwrap_err().to_string().contains('5'));
    // A failed signal reopens notification for the next producer and is counted.
    let notifier = Notifier::new().unwrap();
    notifier.pending.store(1, Ordering::Relaxed);
    notifier.signaled(5);
    assert_eq!(notifier.failed_signals(), 1);
    notifier.notify();
    assert_eq!(
        notifier.wait(Duration::from_millis(50)).unwrap(),
        Wake::Notified
    );
}
