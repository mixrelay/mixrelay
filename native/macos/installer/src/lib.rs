//! macOS distribution transaction planning and unprivileged filesystem staging.
//!
//! `FilesystemAdapter` stages and restores the six owned payload paths under an
//! explicit filesystem root. It does not implement launchd, `CoreAudio`, signing, or
//! privilege acquisition. A platform adapter supplies those operations via
//! [`PlatformBackend`], which enforces one ordered transaction,
//! retained shared lease guards, and explicit recovery reporting.
//! Native adapter integration and clean-machine lifecycle verification remain
//! separate release gates.

mod backend;
mod contract;
#[cfg(unix)]
mod coordinated;
#[cfg(unix)]
mod filesystem;
mod launchd;
#[cfg(unix)]
mod leases;
#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub mod process_exit;
#[cfg(unix)]
mod release;
#[cfg(target_os = "macos")]
pub mod removal;
mod request;
mod runtime_definition;
mod transaction;
mod universal;
mod versions;
pub use versions::optional_driver_install_needed;

pub use backend::{
    LeaseKind, PlatformBackend, Preflight, PreflightError, PreflightResult, RollbackFailure,
};
pub use contract::{
    APP_LEASE_PATH, APP_PATH, DRIVER_MANAGER_LEASE_PATH, DRIVER_PATH, ENGINE_LEASE_PATH, HAL_PATH,
    INSTALLER_LEASE_PATH, LAUNCHD_PATH, LIFECYCLE_DIRECTORY, OwnedTarget, REMOVAL_LAUNCHD_PATH,
    RUNTIME_ENGINE_PATH, RUNTIME_LAUNCHD_PATH, UNINSTALLER_PATH,
};
#[cfg(unix)]
pub use coordinated::{
    CoordinatedBackend, CoordinatedError, CoordinatedRecoveryDiagnostics, ServiceLifecycle,
};
#[cfg(unix)]
pub use filesystem::{
    FilesystemAdapter, FilesystemCommitted, FilesystemError, FilesystemLayout,
    FilesystemRecoveryDiagnostics, FilesystemSnapshot, FilesystemStageState, FilesystemStaged,
    TargetPresence,
};
pub use launchd::{LaunchdConfiguration, LaunchdError, ServiceAccount};
#[cfg(unix)]
pub use leases::{EditorLeaseGuard, LeaseError, LifecycleDirectory, LifecycleLeaseGuard};
#[cfg(target_os = "macos")]
pub use native::{
    NativeError, NativeServiceLifecycle, NativeServiceRegistration, NativeServiceSnapshot,
    ResolvedServiceAccount, check_coreaudio_response, cleanup_user_data, graph_app_ready,
    hal_is_loaded, initialize_app_lifecycle_storage, initialize_editor_lifecycle_storage,
    initialize_lifecycle_storage, install_graph_app, require_lifecycle_storage, require_root,
    resolve_console_account, resolve_install_account, resolve_removal_account,
};
#[cfg(unix)]
pub use release::{ReleaseError, ReleasePayloadVerifier, SigningTeam};
pub use request::{InstallSources, OperationKind, TransactionRequest};
pub use transaction::{
    CleanupStatus, ExecutionResult, RecoveryPhase, RecoveryStatus, TransactionError,
    TransactionOutcome, TransactionState, execute,
};
pub use universal::{UniversalError, validate_universal_binary};
