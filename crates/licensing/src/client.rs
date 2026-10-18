//! Bounded public License API requests and injectable HTTP transport.

use std::time::Duration;

use reqwest::Client;
use secrecy::{ExposeSecret as _, SecretString};
use std::{future::Future, pin::Pin, sync::Arc};

use crate::{Activation, LicenseError, ProductIdentity, response::Response};

const ORIGIN: &str = "https://api.lemonsqueezy.com/v1/licenses";
const MAX_RESPONSE_BYTES: usize = 65_536;

/// Bounded async requests to the public License API. Clone to reuse its connection pool.
#[derive(Clone)]
pub struct LicenseClient {
    http: Arc<dyn LicenseHttp>,
    product: ProductIdentity,
}

/// A bounded HTTP reply. Provider bodies stay inside the licensing boundary.
pub struct LicenseHttpResponse {
    /// HTTP status from the public License API.
    pub status: u16,
    /// Bounded JSON body; never log or display this value.
    pub body: Vec<u8>,
}

/// HTTP boundary for licensing; implementations must not retry mutations.
pub trait LicenseHttp: Send + Sync {
    /// Post a form to one public License API operation.
    fn post<'a>(
        &'a self,
        operation: &'a str,
        form: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>>;
}

impl std::fmt::Debug for LicenseClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LicenseClient")
            .field("product", &self.product)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for LicenseHttpResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LicenseHttpResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

struct Http {
    client: Client,
}

impl LicenseHttp for Http {
    fn post<'a>(
        &'a self,
        operation: &'a str,
        form: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .client
                .post(format!("{ORIGIN}/{operation}"))
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&form)
                .send()
                .await
                .map_err(|_| LicenseError::Unavailable)?;
            let status = response.status().as_u16();
            check_status(status)?;
            Ok(LicenseHttpResponse {
                status,
                body: read_response(response).await?,
            })
        })
    }
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
        Ok(Self::with_http(product, Arc::new(Http { client })))
    }

    /// Bind the public product to an injected, bounded HTTP transport.
    #[must_use]
    pub fn with_http(product: ProductIdentity, http: Arc<dyn LicenseHttp>) -> Self {
        Self { http, product }
    }

    /// Activate a new seat after checking the key's product without consuming a seat.
    ///
    /// `name` is a caller-supplied, non-sensitive device label, not a hardware fingerprint.
    /// Returns its provider ID on success. Repeated calls create seats; retain the result
    /// and call `validate` on subsequent checks. Network errors are ambiguous: do not retry
    /// automatically, since the provider may already have committed the activation.
    /// `before_send` must durably record an activation intent before the mutation is sent.
    ///
    /// # Errors
    /// Distinguishes invalid input, wrong product, rejection, rate limit and transport.
    pub async fn activate(
        &self,
        key: &SecretString,
        name: &str,
        before_send: impl FnOnce() -> Result<(), LicenseError>,
    ) -> Result<Activation, LicenseError> {
        check_argument(name)?;
        let checked = self.request("validate", key, None).await?;
        check_decision(checked.valid)?;
        checked.check_product(self.product)?;
        let expected_id = checked.license_id(&["active", "inactive"])?;
        before_send()?;
        let activated = self
            .request("activate", key, Some(("instance_name", name)))
            .await
            .map_err(uncertain_activation)?;
        check_decision(activated.activated).map_err(uncertain_activation)?;
        activated
            .check_product(self.product)
            .map_err(uncertain_activation)?;
        let activation = activated.activation().map_err(uncertain_activation)?;
        if activation.license_id != expected_id {
            return Err(LicenseError::ActivationUncertain);
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
        response.check_validation(self.product)?;
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
        let checked = match self.validate(key, instance).await {
            Ok(checked) => checked,
            Err(LicenseError::Revoked | LicenseError::ActivationDeactivated) => return Ok(()),
            Err(error) => return Err(error),
        };
        let response = self
            .request("deactivate", key, Some(("instance_id", instance)))
            .await?;
        if let Err(error) = response.check_validation_failure(response.deactivated, self.product) {
            return if matches!(
                error,
                LicenseError::Revoked | LicenseError::ActivationDeactivated
            ) {
                Ok(())
            } else {
                Err(error)
            };
        }
        response.check_product(self.product)?;
        if response.license_id(&["active", "inactive", "disabled", "expired"])?
            != checked.license_id
        {
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
        let response = self.http.post(operation, form).await?;
        check_status(response.status)?;
        if response.body.len() > MAX_RESPONSE_BYTES {
            return Err(LicenseError::InvalidResponse);
        }
        let decoded: Response = serde_json::from_slice(&response.body).map_err(|_| {
            if (200..300).contains(&response.status) {
                LicenseError::InvalidResponse
            } else {
                LicenseError::RequestFailed
            }
        })?;
        if !(200..300).contains(&response.status)
            && [decoded.valid, decoded.activated, decoded.deactivated].contains(&Some(true))
        {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(decoded)
    }
}

pub(super) fn check_argument(value: &str) -> Result<(), LicenseError> {
    if value.trim().is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err(LicenseError::InvalidInput);
    }
    Ok(())
}

fn check_status(status: u16) -> Result<(), LicenseError> {
    match status {
        200..=299 | 400 | 404 => Ok(()),
        429 => Err(LicenseError::RateLimited),
        400..=499 => Err(LicenseError::RequestFailed),
        _ => Err(LicenseError::Unavailable),
    }
}

fn uncertain_activation(error: LicenseError) -> LicenseError {
    match error {
        LicenseError::Rejected | LicenseError::RateLimited => error,
        _ => LicenseError::ActivationUncertain,
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
