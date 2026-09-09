//! macOS transport ABI and resource ownership shared by the HAL and engine.
//!
//! This crate owns mappings/messages, not device policy, graph selection or UI.
//! The static library is also used by native ABI contract tests without invoking
//! Cargo recursively from a build script.
#![cfg(target_os = "macos")]

mod anonymous_memory;
pub mod application_clock;
pub mod source_registry;
pub use anonymous_memory::create as create_shared_memory;
pub mod control_inventory;
pub mod endpoint_identity;
pub mod endpoint_transport;
pub mod hal_io;
pub mod shared_audio_buffer;
pub mod source_control;
pub mod transport_inspection;
