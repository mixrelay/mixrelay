//! License API, entitlement policy and native credential persistence.
//!
//! Remote requests, local entitlement policy and the shared native credential format.
//! The desktop owns purchase UI and Driver lifecycle. Licensing never stops running audio.

mod client;
mod clock;
mod credentials;
mod entitlement;
mod response;

pub use client::{LicenseClient, LicenseHttp, LicenseHttpResponse};
pub use clock::{SystemUtcClock, UtcClock};
pub use credentials::{
    StoredLicense, clear_local_license, has_local_paid_access, read_local_license,
    write_local_license,
};
pub use entitlement::{AccessSnapshot, AccessStatus, EntitlementRecord};
pub use secrecy::SecretString;

/// Publisher-controlled organisation and licence benefit; both IDs are public metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductIdentity {
    organization: uuid::Uuid,
    benefit: uuid::Uuid,
}

impl ProductIdentity {
    /// Bind a client to one paid licence benefit.
    ///
    /// # Errors
    /// Returns `InvalidConfiguration` unless both identifiers are non-nil UUIDs.
    pub fn new(organization: &str, benefit: &str) -> Result<Self, LicenseError> {
        let parse = |value: &str| {
            uuid::Uuid::parse_str(value)
                .ok()
                .filter(|id| !id.is_nil() && id.hyphenated().to_string() == value)
                .ok_or(LicenseError::InvalidConfiguration)
        };
        Ok(Self {
            organization: parse(organization)?,
            benefit: parse(benefit)?,
        })
    }
}

/// Verified device identity, either a Polar activation UUID or a local device label.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activation {
    license_id: String,
    instance_id: String,
    purchased_major: u64,
}

impl Activation {
    /// The provider's license record ID; this is not the secret license key.
    #[must_use]
    pub fn license_id(&self) -> &str {
        &self.license_id
    }

    /// The purchased major bound to the publisher's configured benefit.
    #[must_use]
    pub const fn purchased_major(&self) -> u64 {
        self.purchased_major
    }

    /// Whether this purchase covers a major version, including the early-adopter 0.x/1.x grant.
    #[must_use]
    pub const fn covers_major(&self, major: u64) -> bool {
        self.purchased_major == major || (self.purchased_major == 0 && major == 1)
    }

    /// Polar activation UUID, or `local:` followed by the label for an unlimited key.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

/// Safe error codes: provider response bodies, keys and customer data are never exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseError {
    /// The publisher has not supplied valid organisation and benefit identifiers.
    #[error("invalid licensing configuration")]
    InvalidConfiguration,
    /// A required argument is empty, oversized or contains control characters.
    #[error("invalid license request")]
    InvalidInput,
    /// The remote result belongs to a different organisation or licence benefit.
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
    #[error("activation result uncertain; contact support@rawsector.com")]
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
