use std::time::Duration;

use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret as _, SecretString};

use crate::{Activation, LicenseError, ProductIdentity, response::Response};

const ORIGIN: &str = "https://api.lemonsqueezy.com/v1/licenses";
const MAX_RESPONSE_BYTES: usize = 65_536;

/// Bounded async requests to the public License API. Clone to reuse its connection pool.
#[derive(Clone)]
pub struct LicenseClient {
    client: Client,
    product: ProductIdentity,
    origin: String,
}

impl LicenseClient {
    /// Create an HTTPS client bound to the publisher's product.
    ///
    /// Requests have a ten-second timeout, do not follow redirects, and never automatically
    /// retry seat mutations.
    ///
    /// # Errors
    /// Returns `Unavailable` if TLS/client initialization fails.
    pub fn new(product: ProductIdentity) -> Result<Self, LicenseError> {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| LicenseError::Unavailable)?;
        Ok(Self {
            client,
            product,
            origin: ORIGIN.into(),
        })
    }

    /// Activate a new seat after checking the key's product without consuming a seat.
    ///
    /// `name` is a caller-supplied, non-sensitive device label, not a hardware fingerprint.
    /// Returns its provider ID on success. Repeated calls create seats; retain the result
    /// and call `validate` on subsequent checks. Network errors are ambiguous: do not retry
    /// automatically, since the provider may already have committed the activation.
    ///
    /// # Errors
    /// Distinguishes invalid input, wrong product, rejection, rate limit and transport.
    pub async fn activate(
        &self,
        key: &SecretString,
        name: &str,
    ) -> Result<Activation, LicenseError> {
        check_argument(name)?;
        let checked = self.request("validate", key, None).await?;
        check_decision(checked.valid)?;
        checked.check_product(self.product)?;
        let expected_id = checked.license_id(&["active", "inactive"])?;
        let activated = self
            .request("activate", key, Some(("instance_name", name)))
            .await?;
        check_decision(activated.activated)?;
        activated.check_product(self.product)?;
        let activation = activated.activation()?;
        if activation.license_id != expected_id {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(activation)
    }

    /// Validate this exact seat, including store/product/variant and returned instance ID.
    ///
    /// Errors are remote observations only: this client does not modify local audio or
    /// delete a previously saved activation on transport failure.
    ///
    /// # Errors
    /// Returns `InvalidResponse` for a mismatched seat, `Rejected` for an invalid license,
    /// or the relevant product, rate-limit, input or transport error.
    pub async fn validate(
        &self,
        key: &SecretString,
        instance: &str,
    ) -> Result<Activation, LicenseError> {
        check_argument(instance)?;
        let response = self
            .request("validate", key, Some(("instance_id", instance)))
            .await?;
        check_decision(response.valid)?;
        response.check_product(self.product)?;
        let activation = response.activation()?;
        if activation.instance_id != instance {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(activation)
    }

    /// Release only a validated seat of this product.
    ///
    /// Returns an error if validation or release fails. Only confirmed success permits
    /// removing the caller's saved credential. No retry or shutdown action is performed.
    ///
    /// # Errors
    /// Propagates validation errors or a rejected, malformed or unavailable release result.
    pub async fn deactivate(&self, key: &SecretString, instance: &str) -> Result<(), LicenseError> {
        let checked = self.validate(key, instance).await?;
        let response = self
            .request("deactivate", key, Some(("instance_id", instance)))
            .await?;
        check_decision(response.deactivated)?;
        response.check_product(self.product)?;
        if response.license_id(&["active", "inactive"])? != checked.license_id {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(())
    }

    async fn request(
        &self,
        operation: &str,
        key: &SecretString,
        extra: Option<(&str, &str)>,
    ) -> Result<Response, LicenseError> {
        check_argument(key.expose_secret())?;
        let mut form = vec![("license_key", key.expose_secret())];
        form.extend(extra);
        let response = self
            .client
            .post(format!("{}/{operation}", self.origin))
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|_| LicenseError::Unavailable)?;
        check_status(response.status())?;
        let bytes = read_response(response).await?;
        serde_json::from_slice(&bytes).map_err(|_| LicenseError::InvalidResponse)
    }
}

pub(super) fn check_argument(value: &str) -> Result<(), LicenseError> {
    if value.trim().is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err(LicenseError::InvalidInput);
    }
    Ok(())
}

fn check_status(status: StatusCode) -> Result<(), LicenseError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        429 => Err(LicenseError::RateLimited),
        400..=499 => Err(LicenseError::RequestFailed),
        _ => Err(LicenseError::Unavailable),
    }
}

fn check_decision(flag: Option<bool>) -> Result<(), LicenseError> {
    match flag {
        Some(true) => Ok(()),
        Some(false) => Err(LicenseError::Rejected),
        None => Err(LicenseError::InvalidResponse),
    }
}

async fn read_response(mut response: reqwest::Response) -> Result<Vec<u8>, LicenseError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(LicenseError::InvalidResponse);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| LicenseError::Unavailable)?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(LicenseError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
