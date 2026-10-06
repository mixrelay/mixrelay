use std::{io::Read, time::Duration};

use crate::{
    DRIVER_PORT, DriverCommand, DriverError, DriverSnapshot, SNAPSHOT_ROUTE, contract::JSON_LIMIT,
};

/// Cloneable client fixed to the independent local Driver service, with a five-second deadline.
#[derive(Debug, Clone)]
pub struct DriverClient {
    client: reqwest::Client,
    endpoint: String,
}

impl DriverClient {
    /// Creates the fixed loopback client without proxies or redirects.
    /// # Errors
    /// Returns HTTP client initialization errors.
    pub fn new() -> Result<Self, DriverError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            endpoint: format!("http://127.0.0.1:{DRIVER_PORT}{SNAPSHOT_ROUTE}"),
        })
    }

    /// Reads actual Driver state asynchronously.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors.
    pub async fn snapshot(&self) -> Result<DriverSnapshot, DriverError> {
        read_response(self.client.get(&self.endpoint).send().await?).await
    }

    /// Sends one bounded JSON command asynchronously.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors.
    pub async fn mutate(&self, command: &DriverCommand) -> Result<DriverSnapshot, DriverError> {
        let response = self
            .client
            .put(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(command_bytes(command)?)
            .send()
            .await?;
        read_response(response).await
    }

    /// Reads actual Driver state on a non-async control worker.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors. Must not run on a Tokio reactor.
    pub fn snapshot_blocking(&self) -> Result<DriverSnapshot, DriverError> {
        read_blocking_response(blocking_client()?.get(&self.endpoint).send()?)
    }

    /// Sends one command on a non-async control worker.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors. Must not run on a Tokio reactor.
    pub fn mutate_blocking(&self, command: &DriverCommand) -> Result<DriverSnapshot, DriverError> {
        let response = blocking_client()?
            .put(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(command_bytes(command)?)
            .send()?;
        read_blocking_response(response)
    }
}

fn blocking_client() -> Result<reqwest::blocking::Client, DriverError> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

fn command_bytes(command: &DriverCommand) -> Result<Vec<u8>, DriverError> {
    let bytes = serde_json::to_vec(command)?;
    check_size(bytes.len())?;
    Ok(bytes)
}

async fn read_response(mut response: reqwest::Response) -> Result<DriverSnapshot, DriverError> {
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        check_size(bytes.len().saturating_add(chunk.len()))?;
        bytes.extend_from_slice(&chunk);
    }
    decode_response(status, &bytes)
}

fn read_blocking_response(
    response: reqwest::blocking::Response,
) -> Result<DriverSnapshot, DriverError> {
    let status = response.status();
    let mut bytes = Vec::new();
    response.take(1_048_577).read_to_end(&mut bytes)?;
    check_size(bytes.len())?;
    decode_response(status, &bytes)
}

fn decode_response(
    status: reqwest::StatusCode,
    bytes: &[u8],
) -> Result<DriverSnapshot, DriverError> {
    if !status.is_success() {
        let message = serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
        return Err(DriverError::Remote {
            status: status.as_u16(),
            message,
        });
    }
    Ok(serde_json::from_slice(bytes)?)
}

fn check_size(length: usize) -> Result<(), DriverError> {
    if length > JSON_LIMIT {
        return Err(DriverError::Invalid("Driver JSON exceeds 1 MiB".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
