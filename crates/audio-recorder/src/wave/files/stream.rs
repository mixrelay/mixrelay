//! File extent changes used to discard an incomplete final PCM frame.

use std::{
    fs::File,
    io::{self, BufWriter, Seek, Write},
};

pub(in crate::wave) trait RecordingStream: Write + Seek {
    fn truncate(&mut self, length: u64) -> io::Result<()>;
}

impl RecordingStream for File {
    fn truncate(&mut self, length: u64) -> io::Result<()> {
        self.set_len(length)
    }
}

impl<W: RecordingStream> RecordingStream for BufWriter<W> {
    fn truncate(&mut self, length: u64) -> io::Result<()> {
        // Flush before shortening: buffered bytes must not reappear after the
        // accepted prefix has been finalized or when the stream is dropped.
        self.flush()?;
        self.get_mut().truncate(length)
    }
}

#[cfg(test)]
impl RecordingStream for io::Cursor<Vec<u8>> {
    fn truncate(&mut self, length: u64) -> io::Result<()> {
        let length =
            usize::try_from(length).map_err(|_| io::Error::other("file size exhausted"))?;
        self.get_mut().truncate(length);
        Ok(())
    }
}
