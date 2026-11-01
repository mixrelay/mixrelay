//! Recording ports: the engine hands each recorder node's block to a sink and
//! never knows whether that sink writes files, feeds a test or discards audio.
use std::fmt;

/// Receives one recorder node's interleaved output on the audio thread.
pub trait RecordingSink: fmt::Debug + Send + Sync {
    /// Accepts one interleaved block and its actual processing rate.
    ///
    /// Runs on the audio thread: it must not allocate, lock, block or perform I/O.
    fn capture(&self, samples: &[f32], sample_rate: u32);
}

/// Creates recording sinks while a graph is prepared on the control thread.
pub trait RecordingSinkFactory {
    /// Returns the sink for the recording session `session` with `channels`
    /// interleaved channels. Preparing the same session again must keep its
    /// take, so a graph rebuild does not interrupt recording.
    ///
    /// # Errors
    ///
    /// Returns a stable diagnostic when the session cannot accept this channel count.
    fn prepare(&self, session: &str, channels: u16) -> Result<Box<dyn RecordingSink>, String>;
}
