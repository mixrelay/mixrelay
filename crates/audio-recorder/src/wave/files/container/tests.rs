use super::WaveWriter;
use hound::{Sample, SampleFormat, WavSpec};
use mixrelay_test_support::{FixtureDir, rf64};
use std::{
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom},
    path::Path,
};

#[cfg(feature = "native-lab")]
use std::process::Command;

fn spec(channels: u16, bits: u16, format: SampleFormat) -> WavSpec {
    WavSpec {
        channels,
        sample_rate: 48_000,
        bits_per_sample: bits,
        sample_format: format,
    }
}

#[test]
fn every_encoding_is_rf64_with_exact_sizes_counts_padding_and_pcm() {
    let directory = FixtureDir::new("recorder-rf64-formats").unwrap();
    for channels in [1, 2, 8] {
        for (bits, format) in [
            (16, SampleFormat::Int),
            (24, SampleFormat::Int),
            (32, SampleFormat::Float),
        ] {
            for frames in [0, 1, 3] {
                let path = directory.join(format!("{channels}-{bits}-{frames}.wav"));
                check_format(&path, spec(channels, bits, format), frames);
            }
        }
    }
}

fn check_format(path: &Path, format: WavSpec, frames: u64) {
    let mut writer =
        WaveWriter::new(BufWriter::new(File::create_new(path).unwrap()), format).unwrap();
    for _ in 0..frames * u64::from(format.channels) {
        if format.sample_format == SampleFormat::Float {
            writer.write_sample(0.25_f32).unwrap();
        } else {
            writer.write_sample(-123_i32).unwrap();
        }
    }
    writer.finalize().unwrap();
    let metadata = rf64::inspect(path).unwrap();
    assert_eq!(metadata.frames, frames);
    assert_eq!(
        metadata.data_bytes,
        frames * u64::from(format.channels) * u64::from(format.bits_per_sample / 8)
    );
    let mut reader = rf64::open(path).unwrap();
    assert_eq!(reader.spec(), format);
    if format.sample_format == SampleFormat::Float {
        assert!(
            reader
                .samples::<f32>()
                .all(|sample| sample.unwrap().to_bits() == 0.25_f32.to_bits())
        );
    } else {
        assert!(
            reader
                .samples::<i32>()
                .all(|sample| sample.unwrap() == -123)
        );
    }
    let bytes = std::fs::read(path).unwrap();
    if metadata.data_bytes % 2 == 1 {
        assert_eq!(bytes.last(), Some(&0));
    }
    if format.sample_format == SampleFormat::Float {
        let fact = usize::try_from(metadata.data_offset).unwrap() - 20;
        assert_eq!(&bytes[fact..fact + 4], b"fact");
        assert_eq!(&bytes[fact + 8..fact + 12], &u32::MAX.to_le_bytes());
    }
}

/// Jump through a sparse zero prefix, then write actual PCM across 4 GiB using
/// the production sample writer and finalizer. This checks addressing, not load.
fn sparse_recording(path: &Path, format: WavSpec) -> (u64, Vec<u8>) {
    let mut writer =
        WaveWriter::new(BufWriter::new(File::create_new(path).unwrap()), format).unwrap();
    let frame_bytes = u64::from(format.channels) * u64::from(format.bits_per_sample / 8);
    let prefix_frames = (u64::from(u32::MAX) - 256) / frame_bytes;
    writer.data_bytes = prefix_frames * frame_bytes;
    let tail_offset = writer.header.len() as u64 + writer.data_bytes;
    writer.stream.seek(SeekFrom::Start(tail_offset)).unwrap();
    let mut expected = Vec::new();
    for sample in 0..256 * i32::from(format.channels) {
        if format.sample_format == SampleFormat::Float {
            let value = (f32::from(i16::try_from(sample).unwrap()) + 1.0) / 4096.0;
            writer.write_sample(value).unwrap();
            value.write(&mut expected, 32).unwrap();
        } else {
            writer.write_sample(sample - 123).unwrap();
            (sample - 123)
                .write(&mut expected, format.bits_per_sample)
                .unwrap();
        }
    }
    writer.finalize().unwrap();
    let metadata = rf64::inspect(path).unwrap();
    assert!(metadata.riff_bytes > u64::from(u32::MAX));
    assert!(metadata.data_bytes > u64::from(u32::MAX));
    assert_eq!(metadata.frames, prefix_frames + 256);
    let mut file = File::open(path).unwrap();
    file.seek(SeekFrom::Start(tail_offset)).unwrap();
    let mut actual = vec![0; expected.len()];
    file.read_exact(&mut actual).unwrap();
    assert_eq!(actual, expected);
    (prefix_frames, expected)
}

#[test]
fn sparse_files_cross_real_4_gib_boundary_without_counter_wrap_or_rollover() {
    let directory = FixtureDir::new("recorder-rf64-large").unwrap();
    for (index, format) in [
        spec(1, 16, SampleFormat::Int),
        spec(1, 24, SampleFormat::Int),
        spec(8, 32, SampleFormat::Float),
    ]
    .into_iter()
    .enumerate()
    {
        sparse_recording(&directory.join(format!("{index}.wav")), format);
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[test]
fn unfinished_frame_and_u64_exhaustion_fail_without_silent_size_wrap() {
    let directory = FixtureDir::new("recorder-rf64-invalid").unwrap();
    let mut writer = WaveWriter::new(
        BufWriter::new(File::create_new(directory.join("partial.wav")).unwrap()),
        spec(2, 16, SampleFormat::Int),
    )
    .unwrap();
    writer.write_sample(12_i32).unwrap();
    assert!(
        writer
            .finalize()
            .unwrap_err()
            .to_string()
            .contains("unfinished")
    );
    let mut writer = WaveWriter::new(
        std::io::Cursor::new(Vec::new()),
        spec(1, 32, SampleFormat::Float),
    )
    .unwrap();
    writer.data_bytes = u64::MAX;
    assert!(writer.write_sample(0.25_f32).is_err());
    assert!(writer.finalize().is_err());
}

#[test]
#[cfg(feature = "native-lab")]
fn ffmpeg_decodes_all_small_rf64_formats() {
    let directory = FixtureDir::new("recorder-rf64-ffmpeg").unwrap();
    for channels in [1, 2, 8] {
        for (bits, format, raw) in [
            (16, SampleFormat::Int, "s16le"),
            (24, SampleFormat::Int, "s24le"),
            (32, SampleFormat::Float, "f32le"),
        ] {
            let path = directory.join(format!("{channels}-{bits}.wav"));
            let mut writer = WaveWriter::new(
                BufWriter::new(File::create_new(&path).unwrap()),
                spec(channels, bits, format),
            )
            .unwrap();
            let mut expected = Vec::new();
            for _ in 0..3 * channels {
                if format == SampleFormat::Float {
                    writer.write_sample(0.25_f32).unwrap();
                    0.25_f32.write(&mut expected, bits).unwrap();
                } else {
                    writer.write_sample(-123_i32).unwrap();
                    (-123_i32).write(&mut expected, bits).unwrap();
                }
            }
            writer.finalize().unwrap();
            let decoded = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&path)
                .args(["-f", raw, "-acodec", &format!("pcm_{raw}"), "pipe:1"])
                .output()
                .unwrap();
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            assert_eq!(decoded.stdout, expected);
        }
    }
}

#[test]
#[cfg(feature = "native-lab")]
fn ffmpeg_decodes_pcm_after_the_4_gib_boundary() {
    let directory = FixtureDir::new("recorder-rf64-large-ffmpeg").unwrap();
    let path = directory.join("large.wav");
    let (prefix_frames, expected) = sparse_recording(&path, spec(2, 32, SampleFormat::Float));
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=sample_rate,channels,duration_ts",
            "-of",
            "json",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(metadata["streams"][0]["duration_ts"], prefix_frames + 256);
    assert_eq!(metadata["streams"][0]["sample_rate"], "48000");
    assert_eq!(metadata["streams"][0]["channels"], 2);
    let decoded = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-ss",
            &format!(
                "{:.12}",
                f64::from(u32::try_from(prefix_frames).unwrap()) / 48_000.0
            ),
            "-i",
        ])
        .arg(&path)
        .args(["-f", "f32le", "-acodec", "pcm_f32le", "pipe:1"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(decoded.stdout, expected);
}
