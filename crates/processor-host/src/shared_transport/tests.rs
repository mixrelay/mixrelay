use std::{
    fs::OpenOptions,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tonenari_test_support::FixtureDir;

use super::*;
use crate::{ProcessorStateError, WORKER_READY};

fn layout() -> AudioLayout {
    AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 4,
    }
}

#[test]
fn worker_lease_survives_either_endpoint_and_releases_after_both() {
    let directory = FixtureDir::new("processor-lease").unwrap();
    let path = directory.join("audio");
    let _parent = create(&path, layout()).unwrap();
    let (audio, control) = worker(&path).unwrap();
    assert!(worker(&path).is_err());
    drop(audio);
    assert!(worker(&path).is_err());
    drop(control);
    let (audio, control) = worker(&path).unwrap();
    drop(control);
    assert!(worker(&path).is_err());
    drop(audio);
    assert!(worker(&path).is_ok());
}

#[test]
fn failed_admission_releases_worker_lease() {
    let directory = FixtureDir::new("processor-invalid-mapping").unwrap();
    let path = directory.join("audio");
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.set_len(8).unwrap();
    assert!(worker(&path).is_err());
    assert!(file.try_lock().is_ok());
    file.unlock().unwrap();
    file.set_len(0).unwrap();
    assert!(worker(&path).is_err());
    assert!(file.try_lock().is_ok());
}

#[test]
fn creation_rejects_overflow_without_creating_a_file_or_replacing_an_owner() {
    let directory = FixtureDir::new("processor-create-bounds").unwrap();
    let path = directory.join("audio");
    assert!(
        create(
            &path,
            AudioLayout {
                inputs: usize::MAX,
                ..layout()
            }
        )
        .is_err()
    );
    assert!(!path.exists());
    let _owner = create(&path, layout()).unwrap();
    assert!(create(&path, layout()).is_err());
    assert!(worker(&path).is_ok());
}

#[test]
fn empty_geometry_and_short_blocks_preserve_the_transport_contract() {
    let directory = FixtureDir::new("processor-audio-bounds").unwrap();
    for (index, format) in [
        AudioLayout {
            inputs: 0,
            outputs: 0,
            block_frames: 0,
        },
        layout(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = directory.join(format!("audio-{index}"));
        let (mut parent, _) = create(&path, format).unwrap();
        let (mut worker, _) = worker(&path).unwrap();
        assert!(!parent.submit(0, &[], 0, || {}));
        assert!(!parent.submit(1, &[], format.block_frames + 1, || {}));
        assert!(parent.submit(1, &[], 0, || {}));
        assert!(!parent.submit(2, &[], 0, || {}));
        assert!(worker.receive(|input| assert!(input.iter().all(|sample| *sample == 0.0))));
        assert!(!worker.receive(|_| panic!("input is held until output publication")));
        assert!(worker.publish(|output| output.fill(0.5)));
        let mut output = vec![0.0; format.outputs * format.block_frames];
        assert_eq!(parent.receive(&mut output, format.block_frames + 1), None);
        assert_eq!(parent.receive(&mut output, format.block_frames), Some(1));
        assert_eq!(parent.receive(&mut output, format.block_frames), None);
    }
}

#[test]
fn short_pcm_slices_never_publish_or_acknowledge_a_slot() {
    let directory = FixtureDir::new("processor-short-pcm").unwrap();
    let path = directory.join("audio");
    let (mut parent, _) = create(&path, layout()).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(!parent.submit(1, &[1.0; 7], 4, || panic!(
        "rejected submission cannot start timing"
    )));
    assert!(!worker.input_pending());
    assert!(parent.submit(1, &[1.0, 2.0], 1, || assert!(
        !worker.input_pending(),
        "timing starts before the input publication"
    )));
    assert!(worker.receive(|input| assert_eq!(input, [1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])));
    assert!(worker.publish(|output| output.fill(0.25)));
    assert_eq!(parent.receive(&mut [0.0; 7], 4), None);
    assert_eq!(parent.receive(&mut [0.0; 8], 4), Some(1));
}

#[test]
fn panicking_pcm_copy_does_not_publish_partial_output_or_acknowledge_input() {
    let directory = FixtureDir::new("processor-copy-unwind").unwrap();
    let path = directory.join("audio");
    let (mut parent, _) = create(&path, layout()).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(parent.submit(1, &[0.5; 8], 4, || {}));
    assert!(
        catch_unwind(AssertUnwindSafe(
            || worker.receive(|_| panic!("copy failed"))
        ))
        .is_err()
    );
    assert!(!parent.submit(2, &[1.0; 8], 4, || {}));
    assert!(worker.receive(|input| assert_eq!(input, [0.5; 8])));
    assert!(
        catch_unwind(AssertUnwindSafe(|| worker.publish(|output| {
            output[0] = 9.0;
            panic!("copy failed");
        })))
        .is_err()
    );
    assert_eq!(parent.receive(&mut [0.0; 8], 4), None);
    assert!(worker.publish(|output| output.fill(0.25)));
    let mut output = [0.0; 8];
    assert_eq!(parent.receive(&mut output, 4), Some(1));
    assert_eq!(output.map(f32::to_bits), [0.25_f32; 8].map(f32::to_bits));
}

#[test]
fn stale_and_duplicate_state_responses_cannot_overwrite_the_current_request() {
    let directory = FixtureDir::new("processor-state-generation").unwrap();
    let path = directory.join("audio");
    let (_, parent) = create(&path, layout()).unwrap();
    let (_, worker) = worker(&path).unwrap();
    parent
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let child = thread::spawn(move || {
        for expected in 1..=2 {
            let deadline = Instant::now() + Duration::from_secs(2);
            while worker.pending_request().is_none() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            let request = worker.pending_request().unwrap().0;
            assert_eq!(request, expected);
            worker.publish(request - 1, Ok(vec![99]));
            worker.publish(request, Ok(vec![u8::try_from(request).unwrap()]));
            worker.publish(request, Ok(vec![99]));
        }
    });
    for expected in 1..=2 {
        assert_eq!(
            parent
                .try_lock()
                .unwrap()
                .request_blob(0, Duration::from_secs(2))
                .unwrap(),
            vec![expected]
        );
    }
    child.join().unwrap();
}

#[test]
fn malformed_state_length_is_rejected_without_borrowing_bytes() {
    let directory = FixtureDir::new("processor-state-length").unwrap();
    let path = directory.join("audio");
    let (_, parent) = create(&path, layout()).unwrap();
    let (_, worker) = worker(&path).unwrap();
    parent
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let child = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.pending_request().is_none() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        let header = worker.header();
        header.state_length.store(
            u32::try_from(STATE_CAPACITY + 1).unwrap(),
            Ordering::Relaxed,
        );
        header.state_response.store(1, Ordering::Release);
    });
    assert_eq!(
        parent
            .try_lock()
            .unwrap()
            .request_blob(0, Duration::from_secs(2)),
        Err(ProcessorStateError::InvalidResponse)
    );
    child.join().unwrap();
}

#[test]
fn state_snapshots_and_pcm_exchange_use_disjoint_regions_concurrently() {
    let directory = FixtureDir::new("processor-concurrent-regions").unwrap();
    let path = directory.join("audio");
    let (mut audio, control) = create(&path, layout()).unwrap();
    let (mut worker_audio, worker_control) = worker(&path).unwrap();
    control
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = Arc::clone(&finished);
    let child = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut samples = [0.0; 8];
        while !worker_finished.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            if worker_audio.receive(|input| samples.copy_from_slice(input)) {
                assert!(worker_audio.publish(|output| output.copy_from_slice(&samples)));
            }
            if let Some((request, _)) = worker_control.pending_request() {
                worker_control.publish(request, Ok(vec![0xa5; 4096]));
            }
            thread::yield_now();
        }
    });
    let snapshots = thread::spawn(move || {
        for _ in 0..64 {
            assert_eq!(
                control
                    .try_lock()
                    .unwrap()
                    .request_blob(0, Duration::from_secs(2))
                    .unwrap(),
                vec![0xa5; 4096]
            );
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    for sequence in 1..=1000 {
        let sample = f32::from(u16::try_from(sequence).unwrap());
        assert!(audio.submit(sequence, &[sample; 8], 4, || {}));
        let mut output = [0.0; 8];
        while audio.receive(&mut output, 4).is_none() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(output.map(f32::to_bits), [sample; 8].map(f32::to_bits));
    }
    snapshots.join().unwrap();
    finished.store(true, Ordering::Release);
    child.join().unwrap();
}
