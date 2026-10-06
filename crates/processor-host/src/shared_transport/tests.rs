use std::{
    fs::OpenOptions,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use mixrelay_test_support::FixtureDir;

use super::*;
use crate::{ProcessorStateError, STATE_CAPACITY, WORKER_READY};

fn layout() -> AudioLayout {
    AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 4,
    }
}

fn mapping_length(layout: AudioLayout) -> u64 {
    let samples = (layout.inputs + layout.outputs) * layout.block_frames;
    u64::try_from(size_of::<SharedHeader>() + CHUNK_CAPACITY + samples * size_of::<f32>()).unwrap()
}

fn f32_from_u64(value: u64) -> f32 {
    f32::from(u16::try_from(value).expect("test value fits f32 exactly"))
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
fn responses_need_no_additional_transport_file() {
    let directory = FixtureDir::new("processor-response-files").unwrap();
    let path = directory.join("audio");
    let _transport = create(&path, layout()).unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
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
        assert!(!parent.submit(1, &[], 0, || {}));
        if format.inputs != 0 && format.outputs != 0 && format.block_frames != 0 {
            let input = vec![0.0; format.inputs * format.block_frames];
            assert!(parent.submit(1, &input, format.block_frames, || {}));
            assert_eq!(
                worker.receive(|samples| assert!(samples.iter().all(|sample| *sample == 0.0))),
                Some(format.block_frames)
            );
            assert_eq!(
                worker.receive(|_| panic!("input is held until output publication")),
                None
            );
            assert!(worker.publish(|output| output.fill(0.5)));
        } else {
            assert_eq!(
                worker.receive(|_| panic!("invalid input was published")),
                None
            );
        }
        let mut output = vec![0.0; format.outputs * format.block_frames];
        assert!(!parent.receive(&mut output, format.block_frames + 1, |_| {}));
        if format.inputs != 0 && format.outputs != 0 && format.block_frames != 0 {
            assert!(parent.receive(&mut output, format.block_frames, |_| {}));
            assert!(!parent.receive(&mut output, format.block_frames, |_| {}));
        } else {
            assert!(!parent.receive(&mut output, format.block_frames, |_| {}));
        }
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
    assert_eq!(
        worker.receive(|input| assert_eq!(input, [1.0, 2.0])),
        Some(1)
    );
    assert!(worker.publish(|output| output.fill(0.25)));
    assert!(!parent.receive(&mut [0.0; 7], 4, |_| {}));
    assert_eq!(
        parent.acknowledged_output(),
        0,
        "short output cannot release the slot"
    );
    let mut startup = [f32::NAN; 6];
    assert!(parent.receive(&mut startup, 3, |_| {}));
    assert_eq!(startup.map(f32::to_bits), [0.0_f32; 6].map(f32::to_bits));
    assert_eq!(parent.acknowledged_output(), 1);
    assert!(parent.submit(2, &[3.0; 6], 3, || {}));
    let mut output = [0.0; 2];
    assert!(parent.receive(&mut output, 1, |_| {}));
    assert_eq!(output.map(f32::to_bits), [0.25_f32; 2].map(f32::to_bits));
}

#[test]
fn partial_pcm_submission_exposes_only_actual_input_frames_to_worker() {
    let directory = FixtureDir::new("processor-partial-input-frames").unwrap();
    let path = directory.join("audio");
    let partial_layout = AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 8,
    };
    let (mut parent, _) = create(&path, partial_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    let input = [1.0, 10.0, 2.0, 20.0];

    assert!(parent.submit(1, &input, 2, || {}));
    assert_eq!(
        worker.receive(|source| {
            assert_eq!(source, input);
        }),
        Some(2)
    );
}

#[test]
fn partial_pcm_publication_exposes_only_actual_output_frames_to_parent() {
    let directory = FixtureDir::new("processor-partial-output-frames").unwrap();
    let path = directory.join("audio");
    let partial_layout = AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 8,
    };
    let (mut parent, _) = create(&path, partial_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    let input = [1.0, 10.0, 2.0, 20.0];
    let output = [3.0, 30.0, 4.0, 40.0];

    assert!(parent.submit(1, &input, 2, || {}));
    assert_eq!(worker.receive(|source| assert_eq!(source, input)), Some(2));
    assert!(worker.publish(|destination| {
        assert_eq!(destination.len(), output.len());
        destination.copy_from_slice(&output);
    }));
    assert!(parent.submit(2, &[0.0; 12], 6, || {}));
    let mut received = [0.0; 4];
    assert!(parent.receive(&mut received, 2, |_| {}));
    assert_eq!(received.map(f32::to_bits), output.map(f32::to_bits));
}

#[test]
fn one_frame_callbacks_keep_the_fixed_delay_without_startup_holes() {
    let directory = FixtureDir::new("processor-one-frame-delay").unwrap();
    let path = directory.join("audio");
    let delayed_layout = AudioLayout {
        inputs: 1,
        outputs: 1,
        block_frames: 8,
    };
    let (mut parent, _) = create(&path, delayed_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();

    for sequence in 1..=24_u64 {
        let mut output = [0.0; 1];
        let ready = parent.receive(&mut output, 1, |_| {});
        assert!(ready);
        if sequence <= 8 {
            assert_eq!(output[0].to_bits(), 0.0_f32.to_bits());
        } else {
            assert_eq!(output[0].to_bits(), f32_from_u64(sequence - 8).to_bits());
        }
        assert!(parent.submit(sequence, &[f32_from_u64(sequence)], 1, || {}));
        assert_eq!(
            worker.receive(|input| assert_eq!(input, [f32_from_u64(sequence)])),
            Some(1)
        );
        assert!(worker.publish(|output| output.copy_from_slice(&[f32_from_u64(sequence)])));
    }
}

#[test]
fn returned_tail_is_retained_for_the_next_uneven_callback() {
    let directory = FixtureDir::new("processor-retained-tail").unwrap();
    let path = directory.join("audio");
    let delayed_layout = AudioLayout {
        inputs: 1,
        outputs: 1,
        block_frames: 8,
    };
    let (mut parent, _) = create(&path, delayed_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(parent.submit(1, &[0.0; 8], 8, || {}));
    assert_eq!(worker.receive(|input| assert_eq!(input, [0.0; 8])), Some(8));
    assert!(worker.publish(|output| {
        for (index, sample) in output.iter_mut().enumerate() {
            *sample = f32::from(u16::try_from(index).unwrap());
        }
    }));

    let mut first = [0.0; 4];
    assert!(parent.receive(&mut first, 4, |_| {}));
    assert_eq!(
        first.map(f32::to_bits),
        [0.0_f32, 1.0, 2.0, 3.0].map(f32::to_bits)
    );
    assert!(parent.submit(2, &[0.0; 4], 4, || {}));
    let mut tail = [0.0; 4];
    assert!(parent.receive(&mut tail, 4, |_| {}));
    assert_eq!(
        tail.map(f32::to_bits),
        [4.0_f32, 5.0, 6.0, 7.0].map(f32::to_bits)
    );
}

#[test]
fn busy_submission_leaves_a_timed_gap_without_shifting_later_output() {
    let directory = FixtureDir::new("processor-busy-gap").unwrap();
    let path = directory.join("audio");
    let delayed_layout = AudioLayout {
        inputs: 1,
        outputs: 1,
        block_frames: 4,
    };
    let (mut parent, _) = create(&path, delayed_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(parent.submit(1, &[1.0; 4], 4, || {}));
    assert!(!parent.submit(2, &[2.0; 4], 4, || {}));
    assert_eq!(worker.receive(|input| assert_eq!(input, [1.0; 4])), Some(4));
    assert!(worker.publish(|output| output.fill(1.0)));
    let mut gap = [9.0; 4];
    assert!(!parent.receive(&mut gap, 4, |_| {}));
    assert_eq!(gap.map(f32::to_bits), [9.0_f32; 4].map(f32::to_bits));

    assert_eq!(
        worker.receive(|_| panic!("busy submission must not publish input")),
        None
    );
    assert!(parent.submit(3, &[3.0; 4], 4, || {}));
    assert_eq!(worker.receive(|input| assert_eq!(input, [3.0; 4])), Some(4));
    assert!(worker.publish(|output| output.fill(3.0)));
    let mut later = [0.0; 4];
    assert!(parent.receive(&mut later, 4, |_| {}));
    assert_eq!(later.map(f32::to_bits), [3.0_f32; 4].map(f32::to_bits));
}

#[test]
fn late_output_is_acknowledged_but_never_replayed() {
    let directory = FixtureDir::new("processor-late-output").unwrap();
    let path = directory.join("audio");
    let delayed_layout = AudioLayout {
        inputs: 1,
        outputs: 1,
        block_frames: 4,
    };
    let (mut parent, _) = create(&path, delayed_layout).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(parent.submit(1, &[1.0; 4], 4, || {}));
    assert!(!parent.submit(2, &[2.0; 4], 4, || {}));
    assert_eq!(worker.receive(|_| {}), Some(4));
    let mut output = [0.0; 4];
    assert!(!parent.receive(&mut output, 4, |_| {}));
    assert!(worker.publish(|output| output.fill(1.0)));
    assert!(!parent.receive(&mut output, 4, |_| {}));
    assert_eq!(parent.acknowledged_output(), 1);
    assert!(!parent.receive(&mut output, 4, |_| {}));
}

#[test]
fn malformed_metadata_is_bounded_and_releases_the_output_slot() {
    let directory = FixtureDir::new("processor-malformed-packet").unwrap();
    let path = directory.join("audio");
    let (mut parent, _) = create(&path, layout()).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(parent.submit(1, &[1.0; 8], 4, || {}));
    assert_eq!(worker.receive(|_| {}), Some(4));
    assert!(worker.publish(|output| output.fill(1.0)));
    worker
        .header()
        .output_frames
        .store(u32::MAX, Ordering::Relaxed);
    let mut output = [0.0; 8];
    assert!(!parent.receive(&mut output, 4, |_| {}));
    assert_eq!(parent.acknowledged_output(), 1);
    assert!(parent.submit(2, &[2.0; 8], 4, || {}));
}

#[test]
fn zero_frame_submission_does_not_advance_the_source_timeline() {
    let directory = FixtureDir::new("processor-zero-frame").unwrap();
    let path = directory.join("audio");
    let (mut parent, _) = create(&path, layout()).unwrap();
    let (mut worker, _) = worker(&path).unwrap();
    assert!(!parent.submit(1, &[], 0, || {}));
    assert!(parent.submit(1, &[1.0; 8], 4, || {}));
    assert_eq!(worker.receive(|_| {}), Some(4));
    assert_eq!(worker.header().input_first_frame.load(Ordering::Acquire), 0);
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
    assert_eq!(worker.receive(|input| assert_eq!(input, [0.5; 8])), Some(4));
    assert!(
        catch_unwind(AssertUnwindSafe(|| worker.publish(|output| {
            output[0] = 9.0;
            panic!("copy failed");
        })))
        .is_err()
    );
    assert!(!parent.receive(&mut [0.0; 8], 4, |_| {}));
    assert!(worker.publish(|output| output.fill(0.25)));
    let mut output = [0.0; 8];
    assert!(parent.receive(&mut output, 4, |_| {}));
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
fn full_admitted_state_round_trips_without_growing_mapping() {
    let directory = FixtureDir::new("processor-state-chunk-size").unwrap();
    let path = directory.join("audio");
    let layout = AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 128,
    };
    let (audio, control) = create(&path, layout).unwrap();
    let (_, worker) = worker(&path).unwrap();
    let mapping_size = mapping_length(layout);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), mapping_size);
    control
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let child = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.pending_request().is_none() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        let request = worker.pending_request().unwrap().0;
        worker.publish(request, Ok(vec![0xa5; STATE_CAPACITY]));
    });
    assert_eq!(
        control
            .try_lock()
            .unwrap()
            .request_blob(0, Duration::from_secs(2))
            .unwrap(),
        vec![0xa5; STATE_CAPACITY]
    );
    child.join().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), mapping_size);
    assert_eq!(
        control.header().state_length.load(Ordering::Acquire),
        u32::try_from(STATE_CAPACITY).unwrap()
    );
    drop(audio);
    drop(control);
}

#[test]
fn thirty_two_two_channel_128_frame_mappings_do_not_reserve_state_capacity() {
    let directory = FixtureDir::new("processor-mapping-total").unwrap();
    let layout = AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 128,
    };
    let mut transports = Vec::new();
    let mut total = 0_u64;
    for index in 0..32 {
        let path = directory.join(format!("audio-{index}"));
        let (audio, control) = create(&path, layout).unwrap();
        total += std::fs::metadata(&path).unwrap().len();
        transports.push((audio, control));
    }
    let per_mapping = mapping_length(layout);
    assert_eq!(total, 32 * per_mapping);
    const { assert!(CHUNK_CAPACITY < STATE_CAPACITY) };
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 32);
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
            if worker_audio
                .receive(|input| samples[..input.len()].copy_from_slice(input))
                .is_some()
            {
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
        while !audio.receive(&mut output, 4, |_| {}) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(output.map(f32::to_bits), [sample; 8].map(f32::to_bits));
    }
    snapshots.join().unwrap();
    finished.store(true, Ordering::Release);
    child.join().unwrap();
}
