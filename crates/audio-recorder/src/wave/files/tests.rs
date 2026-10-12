use super::{FileFactory, WaveFile};
use hound::{SampleFormat, WavSpec};
use mixrelay_test_support::FixtureDir;
use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

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
        if self.fault.load(Ordering::Relaxed) == 3 {
            return Err(io::Error::other("seek failed"));
        }
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

#[test]
fn write_flush_and_header_seek_faults_report_the_original_file_without_creating_parts() {
    let directory = FixtureDir::new("recorder-rf64-faults").unwrap();
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    for fault in [1, 2, 3] {
        let path = directory.join(format!("fault-{fault}.wav"));
        let failure = Arc::new(AtomicU8::new(0));
        let mut file = WaveFile::new(&path, spec, FaultFiles(Arc::clone(&failure))).unwrap();
        file.write_sample(0.25_f32).unwrap();
        failure.store(fault, Ordering::Relaxed);
        if fault == 1 {
            let error = file.write_sample(0.5_f32).unwrap_err();
            assert!(error.contains(path.to_str().unwrap()) && error.contains("disk full"));
            failure.store(0, Ordering::Relaxed);
            // A subsequent valid sample cannot bypass the sticky write failure.
            assert!(file.write_sample(0.75_f32).is_err());
        }
        let error = file.finalize().unwrap_err();
        assert!(error.contains(path.to_str().unwrap()));
        assert!(error.contains(match fault {
            1 => "disk full",
            2 => "flush failed",
            _ => "seek failed",
        }));
        if fault == 1 {
            let mut reader = mixrelay_test_support::rf64::open(&path).unwrap();
            assert_eq!(reader.duration(), 1);
            assert_eq!(
                reader.samples::<f32>().next().unwrap().unwrap().to_bits(),
                0.25_f32.to_bits()
            );
        }
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
}
