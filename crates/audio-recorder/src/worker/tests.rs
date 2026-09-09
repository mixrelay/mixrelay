use super::Worker;
use crate::{
    Encoding, RecorderAction, RecorderRequest, RecorderState, RecorderStatus, RecorderTap,
    transport::{PACKET_FRAMES, PACKETS, Transport},
};
use parking_lot::Mutex;
use std::sync::Arc;
use tonenari_test_support::FixtureDir;

#[test]
fn split_take_keeps_original_path_and_total_frames_across_pause_resume_and_stop() {
    let directory = FixtureDir::new("recorder-split-lifecycle").unwrap();
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
    worker.take.as_mut().unwrap().limit_file_frames(2);
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
    let mut second = hound::WavReader::open(directory.join("first-002.wav")).unwrap();
    assert_eq!(
        second
            .samples::<f32>()
            .map(|sample| sample.unwrap().to_bits())
            .collect::<Vec<_>>(),
        [0.25_f32, 0.25, 0.5, 0.5].map(f32::to_bits)
    );
    assert!(!directory.join("first-003.wav").exists());
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
    let mut reader = hound::WavReader::open(path).unwrap();
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
    let mut reader = hound::WavReader::open(path).unwrap();
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
    let mut reader = hound::WavReader::open(path).unwrap();
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
