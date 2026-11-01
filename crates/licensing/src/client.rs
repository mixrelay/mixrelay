//! Bounded public License API requests and injectable HTTP transport.

use std::time::Duration;

use reqwest::Client;
use secrecy::{ExposeSecret as _, SecretString};
use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    Activation, LicenseError, ProductIdentity, SystemUtcClock, UtcClock,
    response::{self, Activated, License},
};

const ORIGIN: &str = "https://api.polar.sh/v1/customer-portal/license-keys";
const SANDBOX_ORIGIN: &str = "https://sandbox-api.polar.sh/v1/customer-portal/license-keys";
const API_VERSION: &str = "2026-10";
const LOCAL_DEVICE: &str = "local:";
const MAX_RESPONSE_BYTES: usize = 65_536;

/// Bounded async requests to the public License API. Clone to reuse its connection pool.
#[derive(Clone)]
pub struct LicenseClient {
    http: Arc<dyn LicenseHttp>,
    product: ProductIdentity,
    clock: Arc<dyn UtcClock>,
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
    /// Post JSON fields to one public License API operation.
    fn post<'a>(
        &'a self,
        operation: &'a str,
        fields: Vec<(&'a str, &'a str)>,
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
    origin: String,
}

impl LicenseHttp for Http {
    fn post<'a>(
        &'a self,
        operation: &'a str,
        fields: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .client
                .post(format!("{}/{operation}", self.origin))
                .header(reqwest::header::ACCEPT, "application/json")
                .header("Polar-Version", API_VERSION)
                .json(
                    &fields
                        .into_iter()
                        .collect::<std::collections::BTreeMap<_, _>>(),
                )
                .send()
                .await
                .map_err(|_| LicenseError::Unavailable)?;
            let status = response.status().as_u16();
            check_status(status)?;
            if response
                .headers()
                .get("Polar-Version")
                .and_then(|value| value.to_str().ok())
                != Some(API_VERSION)
            {
                return Err(LicenseError::RequestFailed);
            }
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
        let client = http_client()?;
        Ok(Self::with_http(
            product,
            Arc::new(Http {
                client,
                origin: ORIGIN.into(),
            }),
            Arc::new(SystemUtcClock),
        ))
    }

    /// Create a sandbox client for development builds.
    ///
    /// # Errors
    /// Release builds reject sandbox configuration.
    pub fn sandbox(product: ProductIdentity) -> Result<Self, LicenseError> {
        if !cfg!(debug_assertions) {
            return Err(LicenseError::InvalidConfiguration);
        }
        Ok(Self::with_http(
            product,
            Arc::new(Http {
                client: http_client()?,
                origin: SANDBOX_ORIGIN.into(),
            }),
            Arc::new(SystemUtcClock),
        ))
    }

    /// Bind the public product to an injected, bounded HTTP transport, judging licence
    /// expiry at `clock`'s time.
    #[must_use]
    pub fn with_http(
        product: ProductIdentity,
        http: Arc<dyn LicenseHttp>,
        clock: Arc<dyn UtcClock>,
    ) -> Self {
        Self {
            http,
            product,
            clock,
        }
    }

    fn check(&self, license: &License) -> Result<(), LicenseError> {
        license.check(self.product, self.clock.now())
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
        let checked = self.license(key, None).await?;
        self.check(&checked)?;
        // Polar disallows remote activations when the benefit has no activation limit.
        // Keep the readable device label locally; no remote mutation can be duplicated.
        if checked.limit_activations.is_none() {
            if name.len() > 255 - LOCAL_DEVICE.len() {
                return Err(LicenseError::InvalidInput);
            }
            return checked.activation(&format!("{LOCAL_DEVICE}{name}"));
        }
        before_send()?;
        let reply = self
            .request("activate", key, Some(("label", name)))
            .await
            .map_err(uncertain_activation)?;
        let activated: Activated =
            serde_json::from_slice(&reply.body).map_err(|_| LicenseError::ActivationUncertain)?;
        self.check(&activated.license_key)
            .map_err(uncertain_activation)?;
        let instance = &activated.instance;
        response::check_id(&instance.id).map_err(uncertain_activation)?;
        if instance.license_key_id != checked.id || activated.license_key.id != checked.id {
            return Err(LicenseError::ActivationUncertain);
        }
        activated.license_key.activation(&instance.id)
    }

    /// Validate the saved device without renewing local entitlement on failure.
    ///
    /// # Errors
    /// Returns a definitive revocation only for a confirmed provider rejection.
    pub async fn validate(
        &self,
        key: &SecretString,
        instance: &str,
    ) -> Result<Activation, LicenseError> {
        check_argument(instance)?;
        if let Some(label) = instance.strip_prefix(LOCAL_DEVICE) {
            check_argument(label)?;
            let checked = self.license(key, None).await?;
            self.check(&checked)?;
            if checked.limit_activations.is_some() {
                return Err(LicenseError::InvalidResponse);
            }
            return checked.activation(instance);
        }
        response::check_id(instance).map_err(|_| LicenseError::InvalidInput)?;
        let checked = match self.license(key, Some(instance)).await {
            Ok(checked) => checked,
            Err(LicenseError::Revoked) => {
                // Polar uses the same 404 for missing keys and missing activations.
                let key_only = self.license(key, None).await?;
                self.check(&key_only)?;
                return Err(LicenseError::ActivationDeactivated);
            }
            Err(error) => return Err(error),
        };
        self.check(&checked)?;
        let activation = checked
            .activation
            .as_ref()
            .ok_or(LicenseError::InvalidResponse)?;
        if activation.id != instance || activation.license_key_id != checked.id {
            return Err(LicenseError::InvalidResponse);
        }
        checked.activation(instance)
    }

    /// Release this device. Unlimited keys have only local device state.
    ///
    /// # Errors
    /// Transient remote failures preserve the saved activation for a later explicit retry.
    pub async fn deactivate(&self, key: &SecretString, instance: &str) -> Result<(), LicenseError> {
        check_argument(instance)?;
        if let Some(label) = instance.strip_prefix(LOCAL_DEVICE) {
            return check_argument(label);
        }
        match self.validate(key, instance).await {
            Ok(_) => {}
            Err(LicenseError::Revoked | LicenseError::ActivationDeactivated) => return Ok(()),
            Err(error) => return Err(error),
        }
        match self
            .request("deactivate", key, Some(("activation_id", instance)))
            .await
        {
            Ok(reply) if reply.status == 204 && reply.body.is_empty() => Ok(()),
            Err(LicenseError::Revoked) => Ok(()),
            Ok(_) => Err(LicenseError::InvalidResponse),
            Err(error) => Err(error),
        }
    }

    async fn license(
        &self,
        key: &SecretString,
        instance: Option<&str>,
    ) -> Result<License, LicenseError> {
        let reply = self
            .request("validate", key, instance.map(|id| ("activation_id", id)))
            .await?;
        serde_json::from_slice(&reply.body).map_err(|_| LicenseError::InvalidResponse)
    }

    async fn request(
        &self,
        operation: &str,
        key: &SecretString,
        extra: Option<(&str, &str)>,
    ) -> Result<LicenseHttpResponse, LicenseError> {
        check_argument(key.expose_secret())?;
        let organization = self.product.organization.to_string();
        let benefit = self.product.benefit.to_string();
        let mut fields = vec![
            ("key", key.expose_secret()),
            ("organization_id", &organization),
        ];
        if operation == "validate" {
            fields.push(("benefit_id", &benefit));
        }
        fields.extend(extra);
        let reply = self.http.post(operation, fields).await?;
        check_status(reply.status)?;
        if reply.body.len() > MAX_RESPONSE_BYTES {
            return Err(LicenseError::InvalidResponse);
        }
        if !(200..300).contains(&reply.status) {
            return Err(response::error(reply.status, &reply.body));
        }
        if reply.status != 200 && !(operation == "deactivate" && reply.status == 204) {
            return Err(LicenseError::InvalidResponse);
        }
        Ok(reply)
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
        200..=299 | 400 | 403 | 404 => Ok(()),
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

fn http_client() -> Result<Client, LicenseError> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| LicenseError::Unavailable)
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
