//! Process-fault reporting retains the worker boundary and reports only new events.

use std::io::Write;

#[cfg(unix)]
mod isolation;

use super::*;
use crate::{
    AudioLayout,
    process_fault::{FaultKind, FaultObserver},
    shared_transport,
};

#[derive(Clone)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn repeated_monitor_windows_report_only_new_faults_with_their_owner() {
    let directory = mixrelay_test_support::FixtureDir::new("fault-report").unwrap();
    let (_, mailbox) = shared_transport::create(
        &directory.join("audio"),
        AudioLayout {
            inputs: 2,
            outputs: 2,
            block_frames: 4,
        },
    )
    .unwrap();
    let control = HostedProcessorControl::new(mailbox);
    let instance = control.shared.header().instance_id;
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = LogWriter(Arc::clone(&bytes));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let mut faults = FaultObserver::default();
        control
            .shared
            .header()
            .process_faults
            .record(FaultKind::NonFinitePcm);
        report_process_faults(&control, &mut faults);
        for _ in 0..32 {
            report_process_faults(&control, &mut faults);
        }
        control
            .shared
            .header()
            .process_faults
            .record(FaultKind::NonFinitePcm);
        report_process_faults(&control, &mut faults);
    });
    let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    for line in &lines {
        for field in [
            format!("os=\"{}\"", std::env::consts::OS),
            "boundary=\"native_processor\"".to_owned(),
            "phase=\"process\"".to_owned(),
            "cause=\"nonfinite_pcm\"".to_owned(),
            format!("instance_id={instance}"),
            "new_failures=1".to_owned(),
        ] {
            assert!(line.contains(&field), "missing {field}: {line}");
        }
    }
    assert!(lines[0].contains("failures=1"));
    assert!(lines[1].contains("failures=2"));
}

#[test]
fn editor_interaction_waits_for_a_parent_request_but_never_masks_a_stuck_dsp_call() {
    use crate::native_operation::{Stage, Watchdog};
    let directory = mixrelay_test_support::FixtureDir::new("native-progress").unwrap();
    let (_, mailbox) = shared_transport::create(
        &directory.join("audio"),
        AudioLayout {
            inputs: 2,
            outputs: 2,
            block_frames: 4,
        },
    )
    .unwrap();
    let control = HostedProcessorControl::new(mailbox);
    let timeout = Duration::from_secs(5);
    let mut creator = Watchdog::new(timeout);
    let mut dsp = Watchdog::new(timeout);
    let now = Instant::now();
    let header = control.shared.header();
    let _interaction = header.native_operation.enter(Stage::EditorEvents);
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now),
        None
    );
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now + timeout * 10),
        None
    );
    header.editor_sequence.store(1, Ordering::Release);
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now + timeout * 10),
        None
    );
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now + timeout * 11),
        Some(Stage::EditorEvents)
    );
    header.editor_consumed.store(1, Ordering::Release);
    let _processing = header.dsp_operation.enter(Stage::Process);
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now + timeout * 12),
        None
    );
    assert_eq!(
        stalled_native_call(&control, &mut creator, &mut dsp, now + timeout * 13),
        Some(Stage::Process)
    );
}
