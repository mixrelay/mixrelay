use super::{FileFactory, RecordingStream, WaveFile};
use hound::{SampleFormat, WavSpec};
use mixrelay_test_support::{FixtureDir, rf64};
use std::{
    fs::File,
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct PartialIo {
    file: File,
    remaining: Arc<AtomicUsize>,
}

impl Write for PartialIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.remaining.load(Ordering::Relaxed);
        if remaining == 0 {
            return Err(io::Error::other("partial disk write"));
        }
        let written = self.file.write(&bytes[..bytes.len().min(remaining)])?;
        self.remaining.fetch_sub(written, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for PartialIo {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

impl RecordingStream for PartialIo {
    fn truncate(&mut self, length: u64) -> io::Result<()> {
        self.file.set_len(length)
    }
}

struct PartialFiles(Arc<AtomicUsize>);

impl FileFactory for PartialFiles {
    type Stream = PartialIo;

    fn create(&mut self, path: &Path) -> io::Result<Self::Stream> {
        Ok(PartialIo {
            file: File::create_new(path)?,
            remaining: Arc::clone(&self.0),
        })
    }
}

#[test]
fn partial_sample_or_channel_write_keeps_only_complete_frames_in_rf64_prefix() {
    let directory = FixtureDir::new("rec-partial-io").unwrap();
    for channels in [1, 2, 8] {
        for (bits, format) in [
            (16, SampleFormat::Int),
            (24, SampleFormat::Int),
            (32, SampleFormat::Float),
        ] {
            let frame_bytes = channels * (bits / 8);
            for partial_bytes in 0..frame_bytes {
                let path = directory.join(format!("{channels}-{bits}-{partial_bytes}.wav"));
                check_partial_write(&path, channels, bits, format, partial_bytes);
            }
        }
    }
}

fn check_partial_write(path: &Path, channels: u16, bits: u16, format: SampleFormat, partial: u16) {
    let remaining = Arc::new(AtomicUsize::new(usize::MAX));
    let spec = WavSpec {
        channels,
        sample_rate: 48_000,
        bits_per_sample: bits,
        sample_format: format,
    };
    let mut file = WaveFile::new(path, spec, PartialFiles(Arc::clone(&remaining))).unwrap();
    for _ in 0..channels * 2 {
        write_sample(&mut file, format).unwrap();
    }
    remaining.store(usize::from(partial), Ordering::Relaxed);
    let failure = (0..channels)
        .find_map(|_| write_sample(&mut file, format).err())
        .unwrap();
    assert!(failure.contains("partial disk write"));
    remaining.store(usize::MAX, Ordering::Relaxed);
    assert!(file.finalize().unwrap_err().contains("partial disk write"));
    let metadata = rf64::inspect(path).unwrap();
    assert_eq!(metadata.frames, 2);
    assert_eq!(
        metadata.data_bytes,
        u64::from(channels) * u64::from(bits / 8) * 2
    );
    assert_eq!(
        std::fs::metadata(path).unwrap().len(),
        metadata.data_offset + metadata.data_bytes + metadata.data_bytes % 2
    );
    let mut reader = rf64::open(path).unwrap();
    if format == SampleFormat::Float {
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
}

fn write_sample<F: FileFactory>(
    file: &mut WaveFile<F>,
    format: SampleFormat,
) -> Result<(), String> {
    if format == SampleFormat::Float {
        file.write_sample(0.25_f32)
    } else {
        file.write_sample(-123_i32)
    }
}

struct BufferedFiles(PartialFiles);

impl FileFactory for BufferedFiles {
    type Stream = BufWriter<PartialIo>;

    fn create(&mut self, path: &Path) -> io::Result<Self::Stream> {
        self.0
            .create(path)
            .map(|stream| BufWriter::with_capacity(17, stream))
    }
}

#[test]
fn recovery_flushes_buffered_pcm_before_truncating_and_never_reappends_the_failed_tail() {
    let directory = FixtureDir::new("rec-buffer-fault").unwrap();
    let path = directory.join("buffered.wav");
    let remaining = Arc::new(AtomicUsize::new(usize::MAX));
    let spec = WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 24,
        sample_format: SampleFormat::Int,
    };
    let mut file = WaveFile::new(
        &path,
        spec,
        BufferedFiles(PartialFiles(Arc::clone(&remaining))),
    )
    .unwrap();
    remaining.store(5, Ordering::Relaxed);
    let accepted_samples = (0_u64..32)
        .find(|_| write_sample(&mut file, spec.sample_format).is_err())
        .unwrap();
    assert!(accepted_samples >= 2, "the buffer accepted complete frames");
    remaining.store(usize::MAX, Ordering::Relaxed);
    assert!(file.finalize().unwrap_err().contains("partial disk write"));
    let metadata = rf64::inspect(&path).unwrap();
    assert_eq!(metadata.frames, accepted_samples / 2);
    assert_eq!(metadata.data_bytes, metadata.frames * 6);
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        metadata.data_offset + metadata.data_bytes + metadata.data_bytes % 2
    );
    let mut reader = rf64::open(path).unwrap();
    assert!(
        reader
            .samples::<i32>()
            .all(|sample| sample.unwrap() == -123)
    );
}
