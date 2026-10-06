use std::time::Duration;

use reqwest::{Client, Method, Response};
use serde_json::Value;

#[cfg(target_os = "macos")]
use super::RequestDelivery;
use super::{ControlProxyClient, ControlProxyResponse};

const OPERATION_HEADER: &str = "x-mixrelay-operation";

// The caller retains its admission guard across polling. An accepted mutation is
// submitted exactly once, even when later observation fails.
pub(super) async fn request(
    client: &ControlProxyClient,
    origin: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<ControlProxyResponse, String> {
    let mut request = client
        .client
        .request(method, format!("{origin}{path}"))
        .timeout(Duration::from_secs(30));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(error) => return Ok(failed(path, &error, false)),
    };
    let mut accepted: Option<String> = None;
    loop {
        let pending = operation_id(&response)?;
        if pending.is_none() {
            validate_terminal(&response, accepted.as_deref())?;
        }
        let status = response.status().as_u16();
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("local control response was invalid: {error}"))?;
        let Some(id) = pending else {
            if let Some(id) = accepted {
                acknowledge(&client.client, &operation_url(origin, &id)?).await;
            }
            return Ok(ControlProxyResponse {
                status,
                body,
                #[cfg(target_os = "macos")]
                delivery: RequestDelivery::ServerResponse,
            });
        };
        validate_receipt(&id, &body, accepted.as_deref())?;
        client.operation_liveness.notify_waiters();
        let url = operation_url(origin, &id)?;
        accepted = Some(id);
        tokio::time::sleep(Duration::from_millis(100)).await;
        response = match client
            .client
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return Ok(failed(path, &error, true)),
        };
    }
}

fn validate_terminal(response: &Response, accepted: Option<&str>) -> Result<(), String> {
    let Some(accepted) = accepted else {
        return Ok(());
    };
    if response
        .headers()
        .get("x-mixrelay-operation-result")
        .and_then(|header| header.to_str().ok())
        != Some(accepted)
    {
        return Err("local control operation result could not be confirmed".to_owned());
    }
    Ok(())
}

fn operation_id(response: &Response) -> Result<Option<String>, String> {
    let Some(header) = response.headers().get(OPERATION_HEADER) else {
        return Ok(None);
    };
    let id = header.to_str().map_err(|_| "invalid operation receipt")?;
    if response.status() != 202 || id.is_empty() || id.len() > 128 {
        return Err("invalid operation receipt".to_owned());
    }
    Ok(Some(id.to_owned()))
}

fn validate_receipt(id: &str, body: &Value, accepted: Option<&str>) -> Result<(), String> {
    if body.get("operationId").and_then(Value::as_str) != Some(id)
        || accepted.is_some_and(|previous| previous != id)
    {
        return Err("local control operation identity changed or was invalid".to_owned());
    }
    Ok(())
}

fn operation_url(origin: &str, id: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(origin).map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|()| "invalid control origin".to_owned())?
        .extend(["api", "v1", "operations", id]);
    Ok(url)
}

async fn acknowledge(client: &Client, url: &reqwest::Url) {
    // A lost acknowledgement only delays server-side receipt reclamation. It
    // must never replace an already observed transaction result with an error.
    if let Err(error) = client
        .delete(url.clone())
        .timeout(Duration::from_secs(1))
        .send()
        .await
    {
        tracing::debug!(?error, "control receipt acknowledgement failed");
    }
}

fn failed(path: &str, error: &reqwest::Error, accepted: bool) -> ControlProxyResponse {
    tracing::warn!(path, ?error, accepted, "control exchange failed");
    let code = if error.is_connect() || error.is_timeout() {
        "control_unavailable"
    } else {
        "control_request_failed"
    };
    ControlProxyResponse {
        status: 503,
        #[cfg(target_os = "macos")]
        delivery: if !accepted && error.is_connect() {
            RequestDelivery::NotSent
        } else {
            RequestDelivery::Uncertain
        },
        body: serde_json::json!({
            "code": code,
            "parameters": {"path": path, "reason": error.to_string()},
        }),
    }
}

#[cfg(test)]
mod tests;
