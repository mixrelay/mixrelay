use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use mixrelay_local_socket::{
    DRIVER_AUTHORITY, DRIVER_SOCKET, session_socket_directory, socket_directory,
};

use crate::{DriverCommand, DriverError, DriverSnapshot, SNAPSHOT_ROUTE, contract::JSON_LIMIT};

/// Cloneable client fixed to the independent local Driver service, with a five-second deadline.
#[derive(Debug, Clone)]
pub struct DriverClient {
    client: reqwest::Client,
    endpoint: String,
    socket_path: PathBuf,
    paid_claim: bool,
}

impl DriverClient {
    /// Creates the current session's Unix socket client without proxies or redirects.
    /// # Errors
    /// Returns socket directory resolution or HTTP client initialization errors.
    pub fn new() -> Result<Self, DriverError> {
        Self::with_socket_path(socket_directory()?.join(DRIVER_SOCKET))
    }

    /// Creates a client of the session's Driver socket, ignoring a socket-directory
    /// override: the desktop app uses this, because the installed Driver service never
    /// sees an override set in the app's environment.
    /// # Errors
    /// Returns socket directory resolution or HTTP client initialization errors.
    pub fn session() -> Result<Self, DriverError> {
        Self::with_socket_path(session_socket_directory()?.join(DRIVER_SOCKET))
    }

    /// Creates a client for an explicit service socket, using the logical Driver authority.
    /// Privileged installers must use the installed service account's socket path.
    /// # Errors
    /// Returns HTTP client initialization errors.
    pub fn with_socket_path(socket_path: impl AsRef<Path>) -> Result<Self, DriverError> {
        let socket_path = socket_path.as_ref().to_path_buf();
        let client = reqwest::Client::builder()
            .unix_socket(socket_path.clone())
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            endpoint: format!("{DRIVER_AUTHORITY}{SNAPSHOT_ROUTE}"),
            socket_path,
            paid_claim: false,
        })
    }

    /// Claims paid access for the signed-in user on every snapshot and command. The
    /// service accepts the claim only from a process it can verify (see the service's
    /// paid-access policy); everyone else keeps free access.
    #[must_use]
    pub fn claiming_paid_access(mut self, paid: bool) -> Self {
        self.paid_claim = paid;
        self
    }

    fn claim(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.paid_claim {
            request.header(crate::PAID_ACCESS_HEADER, "1")
        } else {
            request
        }
    }

    fn claim_blocking(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if self.paid_claim {
            request.header(crate::PAID_ACCESS_HEADER, "1")
        } else {
            request
        }
    }

    /// Sends runtime timing from a non-realtime supervisor.
    /// # Errors
    /// Returns transport, bounded response, JSON or remote errors.
    pub async fn publish_latency(
        &self,
        command: &crate::LatencyCommand,
    ) -> Result<crate::LatencyReply, DriverError> {
        let mut response = self
            .client
            .put(format!("{DRIVER_AUTHORITY}{}", crate::LATENCY_ROUTE))
            .json(command)
            .send()
            .await?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            check_size(bytes.len().saturating_add(chunk.len()))?;
            bytes.extend_from_slice(&chunk);
        }
        decode_latency_response(status, &bytes)
    }

    /// Reads actual Driver state asynchronously.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors.
    pub async fn snapshot(&self) -> Result<DriverSnapshot, DriverError> {
        read_response(self.claim(self.client.get(&self.endpoint)).send().await?).await
    }

    /// Sends one bounded JSON command asynchronously.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors.
    pub async fn mutate(&self, command: &DriverCommand) -> Result<DriverSnapshot, DriverError> {
        let response = self
            .claim(self.client.put(&self.endpoint))
            // Native inventory ACK/drain can take six seconds; allow its bounded result.
            .timeout(
                if matches!(
                    command,
                    DriverCommand::Activate { .. } | DriverCommand::Deactivate { .. }
                ) {
                    Duration::from_secs(15)
                } else {
                    Duration::from_secs(5)
                },
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(command_bytes(command)?)
            .send()
            .await?;
        read_response(response).await
    }

    /// Starts or stops native devices while retaining their saved settings.
    /// Retries a bounded revision conflict from another settings client.
    /// # Errors
    /// Returns transport, native stop/start, persistence or revision errors.
    pub async fn set_active(&self, active: bool) -> Result<DriverSnapshot, DriverError> {
        for attempt in 0..3 {
            let snapshot = self.snapshot().await?;
            if !active
                && snapshot.state == crate::DriverState::NotInstalled
                && snapshot.settings.devices.is_empty()
            {
                return Ok(snapshot);
            }
            let expected_revision = snapshot.revision;
            let command = if active {
                DriverCommand::Activate { expected_revision }
            } else {
                DriverCommand::Deactivate { expected_revision }
            };
            let result = self.mutate(&command).await;
            if attempt < 2 && matches!(result, Err(DriverError::Remote { status: 409, .. })) {
                continue;
            }
            return result;
        }
        unreachable!("bounded revision retries return on the final attempt")
    }

    /// Reads actual Driver state on a non-async control worker.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors. Must not run on a Tokio reactor.
    pub fn snapshot_blocking(&self) -> Result<DriverSnapshot, DriverError> {
        read_blocking_response(
            self.claim_blocking(blocking_client(&self.socket_path)?.get(&self.endpoint))
                .send()?,
        )
    }

    /// Sends one command on a non-async control worker.
    /// # Errors
    /// Returns transport, size, JSON or service rejection errors. Must not run on a Tokio reactor.
    pub fn mutate_blocking(&self, command: &DriverCommand) -> Result<DriverSnapshot, DriverError> {
        let response = self
            .claim_blocking(blocking_client(&self.socket_path)?.put(&self.endpoint))
            // Native inventory ACK/drain can take six seconds; allow its bounded result.
            .timeout(
                if matches!(
                    command,
                    DriverCommand::Activate { .. } | DriverCommand::Deactivate { .. }
                ) {
                    Duration::from_secs(15)
                } else {
                    Duration::from_secs(5)
                },
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(command_bytes(command)?)
            .send()?;
        read_blocking_response(response)
    }
}

fn decode_latency_response(
    status: reqwest::StatusCode,
    bytes: &[u8],
) -> Result<crate::LatencyReply, DriverError> {
    if !status.is_success() {
        return Err(DriverError::Remote {
            status: status.as_u16(),
            message: String::from_utf8_lossy(bytes).into_owned(),
        });
    }
    Ok(serde_json::from_slice(bytes)?)
}

fn blocking_client(socket_path: &Path) -> Result<reqwest::blocking::Client, DriverError> {
    Ok(reqwest::blocking::Client::builder()
        .unix_socket(socket_path.to_path_buf())
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
