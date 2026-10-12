//! Explicit requests accepted by the lifecycle transaction core.

use std::path::{Path, PathBuf};

/// Prebuilt files supplied to an install transaction.
///
/// These are source inputs only. A backend must validate identifiers, slices,
/// signatures, ownership, and exact destination policy during preflight; this
/// type does not interpret an unsigned manifest or provide destination paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallSources {
    app: PathBuf,
    hal: PathBuf,
    driver: PathBuf,
    launchd: PathBuf,
    remover: Option<(PathBuf, PathBuf)>,
    runtime: Option<(PathBuf, PathBuf)>,
}

impl InstallSources {
    /// Creates an install source set from four explicit prebuilt artifacts.
    #[must_use]
    pub fn new(
        app: impl Into<PathBuf>,
        hal: impl Into<PathBuf>,
        driver: impl Into<PathBuf>,
        launchd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            app: app.into(),
            hal: hal.into(),
            driver: driver.into(),
            launchd: launchd.into(),
            remover: None,
            runtime: None,
        }
    }

    /// Adds the separately installed remover app and its on-demand helper plist.
    #[must_use]
    pub fn with_remover(mut self, app: impl Into<PathBuf>, plist: impl Into<PathBuf>) -> Self {
        self.remover = Some((app.into(), plist.into()));
        self
    }

    /// Includes the audio engine and its fixed on-demand launch template.
    #[must_use]
    pub fn with_runtime(mut self, engine: impl Into<PathBuf>, plist: impl Into<PathBuf>) -> Self {
        self.runtime = Some((engine.into(), plist.into()));
        self
    }

    /// Returns the runtime engine, absent only in legacy fixture requests.
    #[must_use]
    pub fn runtime_engine(&self) -> Option<&Path> {
        self.runtime.as_ref().map(|(engine, _)| engine.as_path())
    }

    /// Returns the fixed runtime launch template.
    #[must_use]
    pub fn runtime_launchd(&self) -> Option<&Path> {
        self.runtime.as_ref().map(|(_, plist)| plist.as_path())
    }

    /// Returns the optional remover app source (absent in legacy fixture requests).
    #[must_use]
    pub fn remover(&self) -> Option<&Path> {
        self.remover.as_ref().map(|(app, _)| app.as_path())
    }

    /// Returns the optional fixed root helper definition source.
    #[must_use]
    pub fn removal_launchd(&self) -> Option<&Path> {
        self.remover.as_ref().map(|(_, plist)| plist.as_path())
    }

    /// Returns the application source path.
    #[must_use]
    pub fn app(&self) -> &Path {
        &self.app
    }

    /// Returns the HAL source path.
    #[must_use]
    pub fn hal(&self) -> &Path {
        &self.hal
    }

    /// Returns the driver source path.
    #[must_use]
    pub fn driver(&self) -> &Path {
        &self.driver
    }

    /// Returns the launchd source path.
    #[must_use]
    pub fn launchd(&self) -> &Path {
        &self.launchd
    }
}

/// The single coordinated operation supported by the installer core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionRequest {
    /// Replace the four exact owned targets using the validated sources.
    Install(InstallSources),
    /// Remove only the four exact owned targets and the launchd registration.
    Remove,
}

impl TransactionRequest {
    /// Creates an installation request.
    #[must_use]
    pub fn install(sources: InstallSources) -> Self {
        Self::Install(sources)
    }

    /// Creates a removal request.
    #[must_use]
    pub const fn remove() -> Self {
        Self::Remove
    }

    /// Returns the operation kind without exposing source paths to removal code.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::Install(_) => OperationKind::Install,
            Self::Remove => OperationKind::Remove,
        }
    }
}

/// The transaction direction selected before any platform operation begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    /// Install or replace the exact owned distribution.
    Install,
    /// Remove only the exact owned distribution.
    Remove,
}
