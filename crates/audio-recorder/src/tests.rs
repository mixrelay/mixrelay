use super::{Encoding, RecorderAction, RecorderRegistry, RecorderRequest, RecorderState};
use mixrelay_test_support::FixtureDir;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn request(action: RecorderAction) -> RecorderRequest {
    RecorderRequest {
        action,
        path: None,
        sample_rate: None,
        encoding: None,
    }
}

#[test]
fn preparation_uses_the_shared_one_to_eight_channel_contract() {
    let registry = RecorderRegistry::new();
    for channels in 1..=mixrelay_audio_io::formats::MAX_RECORDING_CHANNELS {
        assert!(
            registry
                .prepare(&format!("channels-{channels}"), channels)
                .is_ok()
        );
    }
    for channels in [
        0,
        mixrelay_audio_io::formats::MAX_RECORDING_CHANNELS + 1,
        64,
        u16::MAX,
    ] {
        assert!(registry.prepare("unsupported", channels).is_err());
    }
}

fn start(registry: &RecorderRegistry, path: &Path, rate: u32, encoding: Encoding) {
    registry
        .command(
            "rec",
            RecorderRequest {
                action: RecorderAction::Record,
                path: Some(path.to_str().unwrap().to_owned()),
                sample_rate: Some(rate),
                encoding: Some(encoding),
            },
        )
        .unwrap();
}

fn read(path: &Path) -> (hound::WavSpec, Vec<f32>) {
    let mut reader = hound::WavReader::open(path).unwrap();
    (
        reader.spec(),
        reader.samples::<f32>().map(Result::unwrap).collect(),
    )
}

#[test]
fn float_capture_pause_resume_and_stop_preserve_exact_interleaved_pcm() {
    let directory = FixtureDir::new("recorder-pause").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 2).unwrap();
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    let first = [0.25, -0.5, 0.125, -0.25];
    tap.capture(&first, 48_000);
    assert_eq!(
        registry
            .command("rec", request(RecorderAction::Pause))
            .unwrap()
            .frames,
        2
    );
    tap.capture(&[9.0; 8], 48_000);
    registry
        .command("rec", request(RecorderAction::Record))
        .unwrap();
    tap.capture(&first, 48_000);
    let stopped = registry
        .command("rec", request(RecorderAction::Stop))
        .unwrap();
    assert_eq!((stopped.state, stopped.frames), (RecorderState::Stopped, 4));
    let (spec, samples) = read(&path);
    assert_eq!((spec.channels, spec.sample_rate), (2, 48_000));
    assert_eq!(samples, first.repeat(2));
}

#[test]
fn stopped_rate_observation_and_create_new_never_overwrite_existing_data() {
    let directory = FixtureDir::new("recorder-create").unwrap();
    let path = directory.join("existing.wav");
    std::fs::write(&path, b"owned-existing-file").unwrap();
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 1).unwrap();
    let mut record = request(RecorderAction::Record);
    record.path = Some(path.to_str().unwrap().to_owned());
    assert!(registry.command("rec", record.clone()).is_err());
    tap.capture(&[], 48_000);
    assert!(registry.command("rec", record).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"owned-existing-file");
    assert!(
        registry
            .status("rec")
            .error
            .unwrap()
            .contains("cannot create")
    );
}

#[test]
fn same_identity_survives_preparation_discard_and_expired_tap_reprepare() {
    let directory = FixtureDir::new("recorder-reprepare").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    drop(registry.prepare("rec", 1).unwrap());
    let active = registry.prepare("rec", 1).unwrap();
    registry.reap_unused().unwrap();
    active.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    drop(registry.prepare("rec", 1).unwrap());
    registry.reap_unused().unwrap();
    assert_eq!(registry.status("rec").state, RecorderState::Recording);
    assert!(registry.prepare("rec", 2).is_err());
    active.capture(&[0.75; 8], 48_000);
    registry
        .command("rec", request(RecorderAction::Stop))
        .unwrap();
    assert_eq!(read(&path).1, [0.75; 8]);
}

#[test]
fn retired_session_churn_does_not_require_a_status_client() {
    let registry = RecorderRegistry::new();
    let retained = registry.prepare("retained", 1).unwrap();
    for index in 0..40 {
        drop(registry.prepare(&format!("retired-{index}"), 1).unwrap());
    }
    retained.capture(&[], 48_000);
    registry.reap_unused().unwrap();
    assert!(registry.prepare("retained", 2).is_err());
}

#[test]
fn channel_replacement_preserves_the_retired_failed_takes_identity_path_and_error() {
    let directory = FixtureDir::new("recorder-replaced-error").unwrap();
    let path = directory.join("missing-parent/failed.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 1).unwrap();
    tap.capture(&[], 48_000);
    assert!(
        registry
            .command(
                "rec",
                RecorderRequest {
                    action: RecorderAction::Record,
                    path: Some(path.to_str().unwrap().into()),
                    sample_rate: None,
                    encoding: None
                }
            )
            .is_err()
    );
    drop(tap);
    let replacement = registry.prepare("rec", 2).unwrap();
    let error = registry.reap_unused().unwrap_err();
    assert!(
        error.contains("recorder rec")
            && error.contains(path.to_str().unwrap())
            && error.contains("cannot create")
    );
    replacement.capture(&[], 48_000);
    assert!(registry.prepare("rec", 1).is_err());
    assert!(registry.reap_unused().is_ok());
}

#[test]
fn stop_all_finalizes_live_taps_and_reports_failed_identity_and_path() {
    let directory = FixtureDir::new("recorder-shutdown").unwrap();
    let path = directory.join("good.wav");
    let bad = directory.join("missing-parent/bad.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 1).unwrap();
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    tap.capture(&[0.5; 4], 48_000);
    let failed = registry.prepare("failed-node", 1).unwrap();
    failed.capture(&[], 48_000);
    assert!(
        registry
            .command(
                "failed-node",
                RecorderRequest {
                    action: RecorderAction::Record,
                    path: Some(bad.to_str().unwrap().into()),
                    sample_rate: None,
                    encoding: None
                }
            )
            .is_err()
    );
    let error = registry.stop_all().unwrap_err();
    assert!(error.contains("failed-node") && error.contains(bad.to_str().unwrap()));
    assert_eq!(registry.status("rec").state, RecorderState::Stopped);
    tap.capture(&[1.0; 4], 48_000);
    assert_eq!(read(&path).1, [0.5; 4]);
}

#[test]
fn final_tap_retirement_finalizes_without_status_polling_or_explicit_stop() {
    let directory = FixtureDir::new("recorder-retire").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 1).unwrap();
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    tap.capture(&[0.5; 16], 48_000);
    drop(tap);
    let deadline = Instant::now() + Duration::from_secs(2);
    while registry.status("rec").state == RecorderState::Recording {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(read(&path).1, [0.5; 16]);
    registry.reap_unused().unwrap();
    assert_eq!(registry.status("rec").path, None);
}

#[test]
fn sample_rate_changes_flush_short_segments_and_keep_one_fixed_wav_rate() {
    let directory = FixtureDir::new("recorder-rate").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 1).unwrap();
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    tap.capture(&[0.25; 441], 44_100);
    tap.capture(&[0.25; 960], 96_000);
    tap.capture(&[0.25; 17], 48_000);
    let status = registry
        .command("rec", request(RecorderAction::Stop))
        .unwrap();
    let (spec, samples) = read(&path);
    assert_eq!(
        (spec.sample_rate, status.frames, samples.len()),
        (48_000, 977, 977)
    );
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(
        samples[100..400]
            .iter()
            .all(|sample| (*sample - 0.25).abs() < 0.002)
    );
    assert_eq!(&samples[960..], &[0.25; 17]);
}

#[test]
fn integer_encodings_clip_and_dither_at_the_final_quantizer() {
    let directory = FixtureDir::new("recorder-integer").unwrap();
    for (encoding, bits) in [(Encoding::Pcm16, 16), (Encoding::Pcm24, 24)] {
        let path = directory.join(format!("pcm{bits}.wav"));
        let registry = RecorderRegistry::new();
        let tap = registry.prepare("rec", 1).unwrap();
        tap.capture(&[], 48_000);
        start(&registry, &path, 48_000, encoding);
        tap.capture(&[-2.0, 2.0], 48_000);
        tap.capture(&[0.0; 2048], 48_000);
        registry
            .command("rec", request(RecorderAction::Stop))
            .unwrap();
        let mut reader = hound::WavReader::open(path).unwrap();
        assert_eq!(reader.spec().bits_per_sample, bits);
        let samples: Vec<i32> = reader.samples().map(Result::unwrap).collect();
        assert_eq!(&samples[..2], &[-(1 << (bits - 1)), (1 << (bits - 1)) - 1]);
        assert!(samples[2..].contains(&1) && samples[2..].contains(&-1));
        assert!(samples[2..].iter().all(|sample| sample.abs() <= 1));
    }
}

#[test]
fn capture_does_not_allocate_or_deallocate_and_invalid_pcm_fails_only_recorder() {
    let directory = FixtureDir::new("recorder-realtime").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    let tap = registry.prepare("rec", 2).unwrap();
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    let allocation = allocation_counter::measure(|| tap.capture(&[0.25; 4096], 48_000));
    assert_eq!(
        (
            allocation.count_total,
            allocation.count_current,
            allocation.bytes_total
        ),
        (0, 0, 0)
    );
    tap.capture(&[0.0; 3], 48_000);
    assert!(
        registry
            .command("rec", request(RecorderAction::Stop))
            .is_err()
    );
    assert_eq!(registry.status("rec").state, RecorderState::Failed);
}

#[test]
fn stop_racing_capture_finalizes_before_return_and_excludes_later_callbacks() {
    let directory = FixtureDir::new("recorder-race").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::new();
    let tap = Arc::new(registry.prepare("rec", 1).unwrap());
    tap.capture(&[], 48_000);
    start(&registry, &path, 48_000, Encoding::Float32);
    let other = Arc::clone(&tap);
    let producer = std::thread::spawn(move || {
        for _ in 0..64 {
            other.capture(&[0.125; 256], 48_000);
        }
    });
    let status = registry
        .command("rec", request(RecorderAction::Stop))
        .unwrap();
    producer.join().unwrap();
    tap.capture(&[1.0; 256], 48_000);
    assert_eq!(read(&path).1.len(), usize::try_from(status.frames).unwrap());
    assert!(
        read(&path)
            .1
            .iter()
            .all(|sample| sample.to_bits() == 0.125_f32.to_bits())
    );
}
