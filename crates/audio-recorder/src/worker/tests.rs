use super::Worker;
use crate::{
    Encoding, RecorderAction, RecorderRequest, RecorderState, RecorderStatus, RecorderTap,
    transport::{PACKET_FRAMES, PACKETS, Transport},
};
use mixrelay_test_support::FixtureDir;
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn single_rf64_take_keeps_original_path_and_total_frames_across_pause_resume_and_stop() {
    let directory = FixtureDir::new("recorder-rf64-lifecycle").unwrap();
    let path = directory.join("first.wav").to_str().unwrap().to_owned();
    let transport = Transport::new(2);
    let tap = RecorderTap::new(Arc::clone(&transport));
    let status = Arc::new(Mutex::new(RecorderStatus::default()));
    let mut worker = Worker::new(transport, Arc::clone(&status));
    let request = RecorderRequest {
        action: RecorderAction::Record,
        path: Some(path.clone()),
        sample_rate: None,
        encoding: Some(Encoding::Float32),
    };
    tap.capture(&[], 48_000);
    worker.record(&request).unwrap();
    tap.capture(&[0.25; 6], 48_000);
    worker.stop(true).unwrap();
    let paused = status.lock().clone();
    assert_eq!(
        (paused.state, paused.frames, paused.path.as_deref()),
        (RecorderState::Paused, 3, Some(path.as_str()))
    );
    worker.record(&request).unwrap();
    tap.capture(&[0.5; 2], 48_000);
    worker.stop(false).unwrap();
    let stopped = status.lock().clone();
    assert_eq!(
        (stopped.state, stopped.frames, stopped.path.as_deref()),
        (RecorderState::Stopped, 4, Some(path.as_str()))
    );
    let mut second = mixrelay_test_support::rf64::open(&path).unwrap();
    assert_eq!(
        second
            .samples::<f32>()
            .map(|sample| sample.unwrap().to_bits())
            .collect::<Vec<_>>(),
        [0.25_f32, 0.25, 0.25, 0.25, 0.25, 0.25, 0.5, 0.5].map(f32::to_bits)
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn a_half_second_writer_stall_preserves_paced_quantum_pcm_in_fixed_memory() {
    let directory = FixtureDir::new("recorder-stalled-writer").unwrap();
    let path = directory.join("packed.wav");
    let transport = Transport::new(8);
    let tap = RecorderTap::new(Arc::clone(&transport));
    let status = Arc::new(Mutex::new(RecorderStatus::default()));
    let mut worker = Worker::new(transport, Arc::clone(&status));
    tap.capture(&[], 48_000);
    worker
        .record(&RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: Some(96_000),
            encoding: Some(Encoding::Pcm24),
        })
        .unwrap();
    // Deliberately do not service the worker for 750 consecutive 32-frame
    // callbacks: 500 ms of input at 48 kHz. No wall-clock/timer assumption.
    let allocation = allocation_counter::measure(|| {
        for _ in 0..750 {
            tap.capture(&[0.25; 32 * 8], 48_000);
        }
    });
    assert_eq!((allocation.count_total, allocation.count_current), (0, 0));
    worker.stop(false).unwrap();
    let snapshot = status.lock().clone();
    assert_eq!(
        (snapshot.state, snapshot.frames),
        (RecorderState::Stopped, 48_000)
    );
    let mut reader = mixrelay_test_support::rf64::open(path).unwrap();
    assert_eq!(
        (
            reader.duration(),
            reader.spec().channels,
            reader.spec().sample_rate
        ),
        (48_000, 8, 96_000)
    );
    assert!(reader.samples::<i32>().all(|sample| sample.unwrap() != 0));
}

#[test]
fn admission_error_finalizes_a_valid_unpublished_partial_packet() {
    let directory = FixtureDir::new("recorder-partial-prefix").unwrap();
    let path = directory.join("prefix.wav");
    let transport = Transport::new(2);
    let tap = RecorderTap::new(Arc::clone(&transport));
    let status = Arc::new(Mutex::new(RecorderStatus::default()));
    let mut worker = Worker::new(transport, Arc::clone(&status));
    tap.capture(&[], 48_000);
    worker
        .record(&RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: None,
            encoding: Some(Encoding::Float32),
        })
        .unwrap();
    tap.capture(&[0.25; 64], 48_000);
    worker.drain().unwrap();
    assert_eq!(status.lock().frames, 0);
    tap.capture(&[0.5], 48_000);
    assert!(worker.stop(false).unwrap_err().contains("invalid PCM"));
    let snapshot = status.lock().clone();
    assert_eq!(
        (snapshot.state, snapshot.frames),
        (RecorderState::Failed, 32)
    );
    let mut reader = mixrelay_test_support::rf64::open(path).unwrap();
    assert_eq!(reader.duration(), 32);
    assert!(
        reader
            .samples::<f32>()
            .all(|sample| sample.unwrap().to_bits() == 0.25_f32.to_bits())
    );
}

#[test]
fn admission_overrun_finalizes_every_previously_accepted_packet_as_a_failed_take() {
    let directory = FixtureDir::new("recorder-overrun-prefix").unwrap();
    let path = directory.join("prefix.wav");
    let transport = Transport::new(1);
    let tap = RecorderTap::new(Arc::clone(&transport));
    let status = Arc::new(Mutex::new(RecorderStatus::default()));
    let mut worker = Worker::new(transport, Arc::clone(&status));
    tap.capture(&[], 48_000);
    worker
        .record(&RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: Some(48_000),
            encoding: Some(Encoding::Float32),
        })
        .unwrap();
    for _ in 0..=PACKETS {
        tap.capture(&[0.5; PACKET_FRAMES], 48_000);
    }
    assert!(worker.stop(false).is_err());
    assert_eq!(status.lock().state, RecorderState::Failed);
    assert_eq!(
        status.lock().frames,
        u64::try_from(PACKETS * PACKET_FRAMES).unwrap()
    );
    let mut reader = mixrelay_test_support::rf64::open(path).unwrap();
    assert_eq!(
        usize::try_from(reader.duration()).unwrap(),
        PACKETS * PACKET_FRAMES
    );
    assert!(
        reader
            .samples::<f32>()
            .all(|sample| sample.unwrap().to_bits() == 0.5_f32.to_bits())
    );

    let missing_path = directory.join("missing/new.wav");
    let rejected = worker.command(&RecorderRequest {
        action: RecorderAction::Record,
        path: Some(missing_path.to_str().unwrap().into()),
        sample_rate: None,
        encoding: None,
    });
    assert!(rejected.unwrap_err().contains("cannot create"));
    assert!(
        status
            .lock()
            .error
            .as_ref()
            .unwrap()
            .contains("cannot create")
    );
    assert!(
        worker
            .command(&RecorderRequest {
                action: RecorderAction::Stop,
                path: None,
                sample_rate: None,
                encoding: None
            })
            .unwrap_err()
            .contains("cannot create")
    );
}

#[test]
fn duration_limit_counts_output_frames_across_pause_resume_and_rate_changes() {
    for encoding in [Encoding::Pcm16, Encoding::Pcm24, Encoding::Float32] {
        let directory = FixtureDir::new("recorder-duration-limit").unwrap();
        let path = directory.join("limited.wav");
        let transport = Transport::new(2);
        let tap = RecorderTap::new(Arc::clone(&transport));
        let status = Arc::new(Mutex::new(RecorderStatus::default()));
        let mut worker = Worker::new(transport, Arc::clone(&status));
        worker.next_take_limit_seconds = Some(1);
        tap.capture(&[], 48_000);
        let request = RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: Some(48_000),
            encoding: Some(encoding),
        };
        worker.record(&request).unwrap();
        for _ in 0..100 {
            tap.capture(&[0.25; 480], 48_000);
            worker.drain().unwrap();
        }
        worker.stop(true).unwrap();
        let paused = status.lock().clone();
        assert_eq!(
            (paused.state, paused.frames),
            (RecorderState::Paused, 24_000)
        );
        // Paused callbacks count no time; an unrestricted resume cannot reset the cap.
        for _ in 0..100 {
            tap.capture(&[0.25; 480], 48_000);
        }
        worker.next_take_limit_seconds = None;
        worker.record(&request).unwrap();
        for _ in 0..102 {
            tap.capture(&[0.25; 960], 96_000);
            worker.drain().unwrap();
        }
        // Drain the SRC tail when the last accepted quantum lands just before the limit.
        worker.stop(false).unwrap();
        let finished = status.lock().clone();
        assert_eq!(
            (finished.state, finished.frames, finished.error),
            (RecorderState::Stopped, 48_000, None)
        );
        assert_eq!(
            mixrelay_test_support::rf64::inspect(&path).unwrap().frames,
            48_000
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn automatic_duration_stop_finalizes_without_a_command_and_releases_only_its_tap() {
    let directory = FixtureDir::new("recorder-auto-stop").unwrap();
    let path = directory.join("limited.wav");
    let transport = Transport::new(2);
    let tap = RecorderTap::new(Arc::clone(&transport));
    let status = Arc::new(Mutex::new(RecorderStatus::default()));
    let mut worker = Worker::new(transport, Arc::clone(&status));
    tap.capture(&[], 48_000);
    worker.next_take_limit_seconds = Some(1);
    worker
        .record(&RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: None,
            encoding: Some(Encoding::Float32),
        })
        .unwrap();
    for _ in 0..190 {
        tap.capture(&[0.5; 512], 48_000);
        worker.drain().unwrap();
    }
    let stopped = status.lock().clone();
    assert_eq!(
        (stopped.state, stopped.frames, stopped.error),
        (RecorderState::Stopped, 48_000, None)
    );
    assert!(worker.take.is_none());
    assert_eq!(
        mixrelay_test_support::rf64::open(&path).unwrap().duration(),
        48_000
    );
    tap.capture(&[0.5; 512], 48_000);
    worker.drain().unwrap();
    assert_eq!(status.lock().frames, 48_000);
    worker.next_take_limit_seconds = None;
    worker
        .record(&RecorderRequest {
            action: RecorderAction::Record,
            path: Some(directory.join("next.wav").to_str().unwrap().into()),
            sample_rate: None,
            encoding: None,
        })
        .unwrap();
    for _ in 0..200 {
        tap.capture(&[0.5; 512], 48_000);
        worker.drain().unwrap();
    }
    worker.stop(false).unwrap();
    assert_eq!(status.lock().frames, 51_200);
}
