//! Where bundle payloads come from and go to. The bundle format depends only on these
//! ports; [`StateStore`](crate::StateStore) implements both on the filesystem.
use std::io::{Read, Write};

use crate::{StateError, StateRef};

/// Verified payloads a bundle is written from.
pub trait StateBlobSource {
    /// Fails unless the payload `reference` names is present and intact.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::MissingBlob`], [`StateError::Integrity`] or a read failure.
    fn verify(&self, reference: &StateRef) -> Result<(), StateError>;

    /// Streams the payload to `writer`, verifying it against `reference` as it goes.
    ///
    /// # Errors
    ///
    /// Returns the failures of [`verify`](Self::verify) or a write failure.
    fn copy_verified(&self, reference: &StateRef, writer: &mut dyn Write)
    -> Result<(), StateError>;
}

/// Keeps a bundle's payloads as they are read.
pub trait StateBlobSink {
    /// Keeps exactly `reference.byte_length()` bytes from `reader` if they hash to
    /// `reference`; nothing is kept otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::Integrity`] for a short or mismatched payload, or a storage
    /// failure.
    fn store_verified(&self, reader: &mut dyn Read, reference: &StateRef)
    -> Result<(), StateError>;
}
