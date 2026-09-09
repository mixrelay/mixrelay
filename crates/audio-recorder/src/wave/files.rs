use hound::{Sample, WavSpec, WavWriter};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Seek, Write},
    path::{Path, PathBuf},
};

/// The narrow file-creation boundary also permits deterministic I/O faults in
/// tests. Production has one implementation and always uses `create_new`.
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

/// Owns only per-file state. Take duration, resampling and dither stay outside
/// this owner so a file boundary cannot reset or duplicate signal processing.
pub(super) struct FileSeries<F: FileFactory = DiskFiles> {
    factory: F,
    writer: Option<WavWriter<F::Stream>>,
    spec: WavSpec,
    original: PathBuf,
    current: PathBuf,
    part: u64,
    frames: u64,
    max_frames: u64,
    error: Option<String>,
}

impl<F: FileFactory> FileSeries<F> {
    pub(super) fn new(path: &Path, spec: WavSpec, factory: F) -> Result<Self, String> {
        let mut files = Self {
            factory,
            writer: None,
            spec,
            original: path.to_owned(),
            current: path.to_owned(),
            part: 1,
            frames: 0,
            // Reserve more than the entire WAV header and possible padding.
            // The resulting limit is whole frames, never a split channel frame.
            max_frames: (u64::from(u32::MAX) - 256)
                / (u64::from(spec.channels) * u64::from(spec.bits_per_sample / 8)),
            error: None,
        };
        files.open_current()?;
        Ok(files)
    }

    pub(super) fn prepare_frame(&mut self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.frames < self.max_frames {
            return Ok(());
        }
        let result = self.advance();
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }

    fn advance(&mut self) -> Result<(), String> {
        self.finish_current()?;
        self.part = self.part.checked_add(1).ok_or_else(|| {
            format!(
                "recording file sequence exhausted: {}",
                self.current.display()
            )
        })?;
        self.current = part_path(&self.original, self.part);
        self.open_current()?;
        self.frames = 0;
        Ok(())
    }

    fn open_current(&mut self) -> Result<(), String> {
        let stream = self.factory.create(&self.current).map_err(|error| {
            format!(
                "cannot create recording {}: {error}",
                self.current.display()
            )
        })?;
        self.writer = Some(WavWriter::new(stream, self.spec).map_err(|error| {
            format!(
                "cannot create WAV header {}: {error}",
                self.current.display()
            )
        })?);
        Ok(())
    }

    pub(super) fn write_sample<S: Sample>(&mut self, sample: S) -> Result<(), String> {
        let result = self
            .writer
            .as_mut()
            .expect("prepare_frame owns an open writer before sample writes")
            .write_sample(sample)
            .map_err(|error| format!("recording write failed {}: {error}", self.current.display()));
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }

    pub(super) fn complete_frame(&mut self) {
        self.frames += 1;
    }

    fn finish_current(&mut self) -> Result<(), String> {
        self.writer.take().map_or(Ok(()), |writer| {
            writer.finalize().map_err(|error| {
                format!(
                    "WAV finalization failed {}: {error}",
                    self.current.display()
                )
            })
        })
    }

    pub(super) fn finalize(&mut self) -> Result<(), String> {
        let finalization = self.finish_current();
        match (self.error.take(), finalization) {
            (None, result) => result,
            (Some(error), Ok(())) => Err(error),
            (Some(error), Err(finalization)) => Err(format!("{error}; {finalization}")),
        }
    }

    #[cfg(test)]
    pub(super) fn limit_frames(&mut self, limit: u64) {
        assert!(limit > 0 && limit <= self.max_frames);
        self.max_frames = limit;
    }
}

fn part_path(original: &Path, part: u64) -> PathBuf {
    let mut name = original
        .file_stem()
        .expect("a successfully created recording path has a file name")
        .to_os_string();
    name.push(format!("-{part:03}."));
    name.push(original.extension().unwrap_or_else(|| "wav".as_ref()));
    original.with_file_name(name)
}

#[cfg(test)]
mod tests;
