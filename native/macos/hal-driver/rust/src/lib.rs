//! Multi-device `AudioServerPlugIn` state behind a narrow C ABI.
//!
//! The control plane owns descriptors, metadata and reclamation. Real-time
//! lookups use fixed slots and reader pins: removing a device cannot free a
//! runtime under a callback, and recycled slots receive new object generations.
#![cfg(target_os = "macos")]

mod abi;
mod clock;
mod control;
mod control_session;
mod device;
#[cfg(target_os = "macos")]
mod inventory;
#[cfg(target_os = "macos")]
mod native;
mod output_control;
#[cfg(target_os = "macos")]
mod properties;
mod registry;
mod transport;

#[path = "../../../../../crates/audio-io/src/formats.rs"]
pub mod formats;

#[cfg(test)]
mod tests;
