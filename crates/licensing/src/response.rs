//! Decode Polar decisions without exposing echoed keys, customer data or error text.

use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{Activation, LicenseError, ProductIdentity};

#[derive(Deserialize)]
pub(super) struct License {
    pub id: String,
    organization_id: String,
    benefit_id: String,
    status: String,
    // Missing policy fields must not turn a malformed reply into unlimited access.
    #[serde(deserialize_with = "required_option")]
    pub limit_activations: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    expires_at: Option<String>,
    pub activation: Option<Instance>,
}

#[derive(Deserialize)]
pub(super) struct Instance {
    pub id: String,
    pub license_key_id: String,
}

#[derive(Deserialize)]
pub(super) struct Activated {
    #[serde(flatten)]
    pub instance: Instance,
    pub license_key: License,
}

impl License {
    /// Accepts only a granted, unexpired licence for `expected`, judged at `now`.
    pub(super) fn check(
        &self,
        expected: ProductIdentity,
        now: OffsetDateTime,
    ) -> Result<(), LicenseError> {
        if self.organization_id != expected.organization.to_string()
            || self.benefit_id != expected.benefit.to_string()
        {
            return Err(LicenseError::WrongProduct);
        }
        check_id(&self.id)?;
        match self.status.as_str() {
            "granted" => {}
            "revoked" | "disabled" => return Err(LicenseError::Revoked),
            _ => return Err(LicenseError::InvalidResponse),
        }
        if let Some(value) = &self.expires_at {
            let expiry = OffsetDateTime::parse(value, &Rfc3339)
                .map_err(|_| LicenseError::InvalidResponse)?;
            if expiry <= now {
                return Err(LicenseError::Revoked);
            }
        }
        if self.limit_activations == Some(0) {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(())
    }

    pub(super) fn activation(&self, instance: &str) -> Result<Activation, LicenseError> {
        Ok(Activation {
            license_id: self.id.clone(),
            purchased_major: env!("CARGO_PKG_VERSION_MAJOR")
                .parse()
                .map_err(|_| LicenseError::InvalidConfiguration)?,
            instance_id: instance.to_owned(),
        })
    }
}

pub(super) fn check_id(value: &str) -> Result<(), LicenseError> {
    uuid::Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.hyphenated().to_string() == value)
        .map(|_| ())
        .ok_or(LicenseError::InvalidResponse)
}

pub(super) fn error(status: u16, body: &[u8]) -> LicenseError {
    #[derive(Deserialize)]
    struct ErrorResponse {
        error: String,
        detail: String,
    }
    let Ok(response) = serde_json::from_slice::<ErrorResponse>(body) else {
        return LicenseError::RequestFailed;
    };
    // Only the documented resource error is a definitive rejection. Unknown routes,
    // version errors, proxies and malformed bodies cannot revoke an offline lease.
    if status == 404 && response.error == "ResourceNotFound" {
        return match response.detail.as_str() {
            "Not found" | "License key is no longer active." | "License key has expired." => {
                LicenseError::Revoked
            }
            _ => LicenseError::RequestFailed,
        };
    }
    match (status, response.error.as_str()) {
        (403, "NotPermitted") => LicenseError::Rejected,
        _ => LicenseError::RequestFailed,
    }
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}
