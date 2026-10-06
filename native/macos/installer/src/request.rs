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
        }
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
