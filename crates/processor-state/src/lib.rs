//! Immutable, content-addressed storage for processor state and portable bundles.

mod bundle;
mod error;
mod reference;
mod store;

pub use bundle::{
    MAX_BUNDLE_MANIFEST_BYTES, MAX_BUNDLE_PAYLOAD_BYTES, MAX_BUNDLE_REFERENCES, read_bundle,
    write_bundle,
};
pub use error::StateError;
pub use reference::StateRef;
pub use store::StateStore;

/// Maximum size of one processor-state blob.
pub const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;
