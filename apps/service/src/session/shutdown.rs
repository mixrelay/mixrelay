//! Observes process shutdown signals before audio startup begins.

/// OS shutdown requests, caught from registration on, awaited on the control runtime and
/// never the audio thread. Registering before start-up work turns a stop during start-up
/// into an orderly shutdown instead of a killed process.
#[cfg(unix)]
pub struct ShutdownSignals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ShutdownSignals {
    /// Must run inside a Tokio runtime.
    ///
    /// # Errors
    /// Returns signal registration failures.
    pub fn register() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    /// Resolves at the first shutdown request, including one received before this call.
    ///
    /// # Errors
    /// Never fails today; kept fallible like other shutdown sources.
    pub async fn requested(mut self) -> std::io::Result<()> {
        tokio::select! {
            _ = self.terminate.recv() => Ok(()),
            _ = self.interrupt.recv() => Ok(()),
        }
    }
}

#[cfg(not(unix))]
pub struct ShutdownSignals;

#[cfg(not(unix))]
impl ShutdownSignals {
    /// # Errors
    /// Never fails on this platform.
    pub fn register() -> std::io::Result<Self> {
        Ok(Self)
    }

    /// # Errors
    /// Returns Ctrl-C registration failures.
    pub async fn requested(self) -> std::io::Result<()> {
        tokio::signal::ctrl_c().await
    }
}
