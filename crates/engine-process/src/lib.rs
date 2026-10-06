//! Private desktop/engine process ownership, independent of audio processing.
//! The desktop retains a pipe writer; its closure (including desktop death)
//! requests the engine's existing graceful drain. Readiness uses the child pipe,
//! never an arbitrary process listening on the public control port.

mod child;
mod parent;

pub use child::{EngineOwner, shutdown};
pub use parent::DesktopEngine;

/// Selects the private owned-child launch mode, not a public control command.
pub const DESKTOP_ARGUMENT: &str = "--desktop-owned";

/// Exact acknowledgement after the owned control listener is prepared.
/// Audio device and clock health remain the existing runtime status contract.
pub const READY: &[u8; 8] = b"MRREADY\n";
