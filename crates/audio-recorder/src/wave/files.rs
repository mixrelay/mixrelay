use hound::{Sample, WavSpec};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Seek, Write},
    path::{Path, PathBuf},
};

mod container;
use container::WaveWriter;

/// The narrow file-creation boundary permits deterministic I/O faults in tests.
/// Production always creates one new file, never overwriting existing recordings.
pub(super) trait FileFactory {
    type Stream: Write + Seek;
    fn create(&mut self, path: &Path) -> io::Result<Self::Stream>;
}

pub(super) struct DiskFiles;

impl FileFactory for DiskFiles {
    type Stream = BufWriter<File>;

    fn create(&mut self, path: &Path) -> io::Result<Self::Stream> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(BufWriter::new)
    }
}

/// Owns only the RF64 file. Take duration, resampling and dither stay outside.
pub(super) struct WaveFile<F: FileFactory = DiskFiles> {
    writer: Option<WaveWriter<F::Stream>>,
    path: PathBuf,
    error: Option<String>,
}

impl<F: FileFactory> WaveFile<F> {
    pub(super) fn new(path: &Path, spec: WavSpec, mut factory: F) -> Result<Self, String> {
        let stream = factory
            .create(path)
            .map_err(|error| format!("cannot create recording {}: {error}", path.display()))?;
        let writer = WaveWriter::new(stream, spec)
            .map_err(|error| format!("cannot create RF64 header {}: {error}", path.display()))?;
        Ok(Self {
            writer: Some(writer),
            path: path.to_owned(),
            error: None,
        })
    }

    pub(super) fn write_sample<S: Sample>(&mut self, sample: S) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let Some(writer) = self.writer.as_mut() else {
            return Err(format!(
                "recording {} is already finalized",
                self.path.display()
            ));
        };
        let result = writer
            .write_sample(sample)
            .map_err(|error| format!("recording write failed {}: {error}", self.path.display()));
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }

    pub(super) fn finalize(&mut self) -> Result<(), String> {
        let finalization = self.writer.take().map_or(Ok(()), |writer| {
            writer.finalize().map_err(|error| {
                format!("RF64 finalization failed {}: {error}", self.path.display())
            })
        });
        match (self.error.take(), finalization) {
            (None, result) => result,
            (Some(error), Ok(())) => Err(error),
            (Some(error), Err(finalization)) => Err(format!("{error}; {finalization}")),
        }
    }
}

#[cfg(test)]
mod tests;
