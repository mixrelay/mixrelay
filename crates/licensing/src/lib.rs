//! Lemon Squeezy's public License API, isolated from the audio engine.
//!
//! Remote requests, local entitlement policy and the shared native credential format.
//! The desktop owns purchase UI and Driver lifecycle. Licensing never stops running audio.

mod client;
mod credentials;
mod entitlement;
mod response;

pub use client::{LicenseClient, LicenseHttp, LicenseHttpResponse};
pub use credentials::{
    LOCAL_LICENSE_SERVICE, StoredLicense, clear_local_license, has_local_paid_access,
};
pub use entitlement::{AccessSnapshot, AccessStatus, EntitlementRecord};
pub use secrecy::SecretString;

/// Immutable store/product/variant binding supplied by the publisher, not by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductIdentity {
    store: u64,
    product: u64,
    variant: u64,
}

impl ProductIdentity {
    /// Bind a client to one paid variant.
    ///
    /// IDs are public product metadata, not merchant credentials.
    ///
    /// # Errors
    /// Returns `InvalidConfiguration` if any identifier is zero.
    pub fn new(store: u64, product: u64, variant: u64) -> Result<Self, LicenseError> {
        if store == 0 || product == 0 || variant == 0 {
            return Err(LicenseError::InvalidConfiguration);
        }
        Ok(Self {
            store,
            product,
            variant,
        })
    }
}

/// Remote activation identity. Keep the instance ID to validate or release this seat.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activation {
    license_id: u64,
    instance_id: String,
    purchased_major: u64,
}

impl Activation {
    /// The provider's license record ID; this is not the secret license key.
    #[must_use]
    pub const fn license_id(&self) -> u64 {
        self.license_id
    }

    /// The purchased major bound to the publisher's configured variant.
    #[must_use]
    pub const fn purchased_major(&self) -> u64 {
        self.purchased_major
    }

    /// The seat identifier returned by the activation request.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

/// Safe error codes: provider response bodies, keys and customer data are never exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseError {
    /// The publisher has not supplied positive product identifiers.
    #[error("invalid licensing configuration")]
    InvalidConfiguration,
    /// A required argument is empty, oversized or contains control characters.
    #[error("invalid license request")]
    InvalidInput,
    /// The remote result belongs to a different store, product or variant.
    #[error("license belongs to another product")]
    WrongProduct,
    /// The provider rejected the key, instance, or activation limit.
    #[error("license request rejected")]
    Rejected,
    /// The provider confirmed that the key is disabled, expired or missing.
    #[error("license revoked")]
    Revoked,
    /// The provider confirmed that this activation no longer exists.
    #[error("activation deactivated")]
    ActivationDeactivated,
    /// A seat mutation may have committed; support must reconcile it before a new activation.
    #[error("activation result uncertain; contact support@mixrelay.com")]
    ActivationUncertain,
    /// The HTTP/API request failed; this is not a definitive entitlement rejection.
    #[error("license request failed")]
    RequestFailed,
    /// Retry later; a failed activation/deactivation may have committed remotely.
    #[error("licensing service unavailable")]
    Unavailable,
    /// The service asked the caller to wait; no automatic request loop is started.
    #[error("license request rate limited")]
    RateLimited,
    /// The response cannot establish a valid product/instance binding.
    #[error("invalid licensing response")]
    InvalidResponse,
    /// A stored entitlement contains impossible or inconsistent fields.
    #[error("invalid persisted licensing state")]
    InvalidState,
    /// The supplied clock moved backwards or is outside the supported range.
    #[error("invalid licensing clock")]
    ClockInvalid,
    /// Local credential or entitlement storage could not be read or committed.
    #[error("license storage unavailable")]
    StorageUnavailable,
}
