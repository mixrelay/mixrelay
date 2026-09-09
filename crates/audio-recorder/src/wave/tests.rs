use super::Take;
use crate::{Encoding, RecorderAction, RecorderRequest};
use std::path::Path;
use tonenari_test_support::FixtureDir;

fn take(path: &Path, encoding: Encoding, channels: u16) -> Take {
    Take::new(
        &RecorderRequest {
            action: RecorderAction::Record,
            path: Some(path.to_str().unwrap().into()),
            sample_rate: Some(48_000),
            encoding: Some(encoding),
        },
        channels,
        48_000,
    )
    .unwrap()
}

fn pcm_words(path: &Path) -> Vec<u32> {
    let mut reader = hound::WavReader::open(path).unwrap();
    if reader.spec().sample_format == hound::SampleFormat::Float {
        reader
            .samples::<f32>()
            .map(|sample| sample.unwrap().to_bits())
            .collect()
    } else {
        reader
            .samples::<i32>()
            .map(|sample| u32::from_ne_bytes(sample.unwrap().to_ne_bytes()))
            .collect()
    }
}

#[test]
fn frame_boundary_rollover_preserves_exact_pcm_and_never_creates_an_empty_next_part() {
    for channels in [1, 2, 8] {
        let directory = FixtureDir::new("recorder-exact-rollover").unwrap();
        let first = directory.join("first.wav");
        let mut take = take(&first, Encoding::Float32, channels);
        take.limit_file_frames(3);
        let input: Vec<f32> = (0..channels * 6)
            .map(|sample| f32::from(sample) / 64.0)
            .collect();
        let boundary = usize::from(channels) * 3;
        take.write(&input[..boundary], 48_000).unwrap();
        take.flush_segment().unwrap();
        assert!(!directory.join("first-002.wav").exists());
        take.prepare_rate(48_000).unwrap();
        take.write(&input[boundary..], 48_000).unwrap();
        assert_eq!(take.frames(), 6);
        assert_eq!(take.finish().unwrap(), 6);
        assert!(!directory.join("first-003.wav").exists());
        let reader = hound::WavReader::open(&first).unwrap();
        assert_eq!((reader.spec().channels, reader.duration()), (channels, 3));
        let mut output = pcm_words(&first);
        output.extend(pcm_words(&directory.join("first-002.wav")));
        assert_eq!(
            output,
            input
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn rollover_keeps_src_fractional_tail_and_tpdf_state_identical_to_one_file() {
    let directory = FixtureDir::new("recorder-converted-rollover").unwrap();
    for encoding in [Encoding::Pcm16, Encoding::Pcm24, Encoding::Float32] {
        let single_path = directory.join(format!("single-{encoding:?}.wav"));
        let split_path = directory.join(format!("split-{encoding:?}.wav"));
        let mut single = take(&single_path, encoding, 2);
        let mut split = take(&split_path, encoding, 2);
        split.limit_file_frames(7);
        for (rate, frames) in [(44_100, 37_i16), (96_000, 19), (48_000, 3)] {
            let samples: Vec<f32> = (0..frames)
                .flat_map(|index| {
                    let value = f32::from(index + 1) / 128.0;
                    [value, -value]
                })
                .collect();
            single.write(&samples, rate).unwrap();
            split.write(&samples, rate).unwrap();
        }
        let frames = single.finish().unwrap();
        assert_eq!(split.finish().unwrap(), frames);
        let mut output = pcm_words(&split_path);
        for part in 2..=frames.div_ceil(7) {
            let path = directory.join(format!("split-{encoding:?}-{part:03}.wav"));
            assert!(u64::from(hound::WavReader::open(&path).unwrap().duration()) <= 7);
            output.extend(pcm_words(&path));
        }
        assert_eq!(output, pcm_words(&single_path));
        let next = frames.div_ceil(7) + 1;
        assert!(
            !directory
                .join(format!("split-{encoding:?}-{next:03}.wav"))
                .exists()
        );
    }
}

#[test]
fn next_part_collision_preserves_the_completed_prefix_and_existing_file() {
    let directory = FixtureDir::new("recorder-rollover-collision").unwrap();
    let first = directory.join("first.wav");
    let second = directory.join("first-002.wav");
    std::fs::write(&second, b"do not overwrite").unwrap();
    let mut take = take(&first, Encoding::Float32, 2);
    take.limit_file_frames(2);
    let error = take.write(&[0.25; 6], 48_000).unwrap_err();
    assert!(error.contains(second.to_str().unwrap()));
    assert_eq!(take.frames(), 2);
    assert!(
        take.finish()
            .unwrap_err()
            .contains(second.to_str().unwrap())
    );
    assert_eq!(pcm_words(&first), vec![0.25_f32.to_bits(); 4]);
    assert_eq!(std::fs::read(&second).unwrap(), b"do not overwrite");
    assert!(!directory.join("first-003.wav").exists());
}

#[test]
fn record_and_resume_prepare_the_observed_converter_before_pcm_admission() {
    let directory = FixtureDir::new("recorder-converter-preparation").unwrap();
    let mut take = Take::new(
        &RecorderRequest {
            action: RecorderAction::Record,
            path: Some(directory.join("prepared.wav").to_str().unwrap().into()),
            sample_rate: Some(96_000),
            encoding: Some(Encoding::Float32),
        },
        1,
        48_000,
    )
    .unwrap();
    assert_eq!(take.segment.as_ref().unwrap().rate, 48_000);
    assert!(take.segment.as_ref().unwrap().resampler.is_some());
    // A clock changed after preparation: the packet's own rate remains the
    // authority, rather than retagging it with the observed Record-time rate.
    take.write(&[0.5], 44_100).unwrap();
    assert_eq!(take.segment.as_ref().unwrap().rate, 44_100);
    take.flush_segment().unwrap();
    assert!(take.segment.is_none());
    take.prepare_rate(48_000).unwrap();
    assert_eq!(take.segment.as_ref().unwrap().rate, 48_000);
    take.write(&[0.25], 48_000).unwrap();
    assert_eq!(take.finish().unwrap(), 4);
}

#[test]
fn one_frame_conversion_tails_have_exact_duration_for_all_supported_rate_pairs() {
    let directory = FixtureDir::new("recorder-rate-pairs").unwrap();
    for &from in tonenari_audio_io::formats::SAMPLE_RATES {
        for &to in tonenari_audio_io::formats::SAMPLE_RATES {
            let path = directory.join(format!("{from}-{to}.wav"));
            let request = RecorderRequest {
                action: RecorderAction::Record,
                path: Some(path.to_str().unwrap().to_owned()),
                sample_rate: Some(to),
                encoding: Some(Encoding::Float32),
            };
            let mut take = Take::new(&request, 1, from).unwrap();
            take.write(&[0.5], from).unwrap();
            let expected = (u64::from(to) + u64::from(from / 2)) / u64::from(from);
            assert_eq!(take.finish().unwrap(), expected);
            assert_eq!(
                u64::from(hound::WavReader::open(path).unwrap().duration()),
                expected
            );
        }
    }
}
