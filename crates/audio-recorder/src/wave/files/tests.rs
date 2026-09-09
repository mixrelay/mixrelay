use super::{DiskFiles, FileFactory, FileSeries, part_path};
use hound::{SampleFormat, WavSpec};
use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tonenari_test_support::FixtureDir;

struct FaultIo {
    file: File,
    fault: Arc<AtomicU8>,
}

impl Write for FaultIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fault.load(Ordering::Relaxed) == 1 {
            return Err(io::Error::other("disk full"));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fault.load(Ordering::Relaxed) == 2 {
            return Err(io::Error::other("flush failed"));
        }
        self.file.flush()
    }
}

impl Seek for FaultIo {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

struct FaultFiles(Arc<AtomicU8>);
impl FileFactory for FaultFiles {
    type Stream = FaultIo;
    fn create(&mut self, path: &Path) -> io::Result<Self::Stream> {
        Ok(FaultIo {
            file: File::create_new(path)?,
            fault: Arc::clone(&self.0),
        })
    }
}

fn spec() -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    }
}

fn write_frame<F: FileFactory>(files: &mut FileSeries<F>) -> Result<(), String> {
    files.prepare_frame()?;
    files.write_sample(0.25_f32)?;
    files.complete_frame();
    Ok(())
}

#[test]
fn write_and_rollover_finalize_faults_include_the_file_and_preserve_earlier_parts() {
    let directory = FixtureDir::new("recorder-rollover-faults").unwrap();
    for fault in [1, 2] {
        let path = directory.join(format!("fault-{fault}.wav"));
        let failure = Arc::new(AtomicU8::new(0));
        let mut files = FileSeries::new(&path, spec(), FaultFiles(Arc::clone(&failure))).unwrap();
        files.limit_frames(1);
        write_frame(&mut files).unwrap();
        write_frame(&mut files).unwrap();
        let original = std::fs::read(&path).unwrap();
        failure.store(fault, Ordering::Relaxed);
        let error = if fault == 1 {
            files.write_sample(0.5_f32).unwrap_err()
        } else {
            files.prepare_frame().unwrap_err()
        };
        assert!(error.contains(part_path(&path, 2).to_str().unwrap()));
        assert!(error.contains(if fault == 1 {
            "disk full"
        } else {
            "finalization failed"
        }));
        assert!(files.prepare_frame().is_err());
        assert!(files.finalize().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 1);
        assert!(!part_path(&path, 3).exists());
    }
}

#[test]
fn numbered_paths_are_deterministic_and_riff_limits_fit_whole_frames() {
    let directory = FixtureDir::new("recorder-file-sequence").unwrap();
    let path = directory.join("take.part.wav");
    assert_eq!(part_path(&path, 2), directory.join("take.part-002.wav"));
    assert_eq!(part_path(&path, 1000), directory.join("take.part-1000.wav"));
    assert_eq!(
        part_path(&directory.join("take"), 2),
        directory.join("take-002.wav")
    );
    for (channels, bits) in [(1, 16), (2, 24), (8, 32)] {
        let path = directory.join(format!("{channels}-{bits}.wav"));
        let mut format = spec();
        format.channels = channels;
        format.bits_per_sample = bits;
        format.sample_format = SampleFormat::Int;
        let mut files = FileSeries::new(&path, format, DiskFiles).unwrap();
        let frame_bytes = u64::from(channels) * u64::from(bits / 8);
        let capacity = u64::from(u32::MAX) - 256;
        assert!(files.max_frames * frame_bytes <= capacity);
        assert!((files.max_frames + 1) * frame_bytes > capacity);
        files.finalize().unwrap();
    }
}
