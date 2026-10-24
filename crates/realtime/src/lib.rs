//! Realtime scheduling and wakeups for audio threads this product owns. See
//! `docs/design/realtime-and-control-protocol.md`.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
