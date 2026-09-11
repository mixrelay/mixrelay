use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::Value;
#[cfg(target_os = "macos")]
use std::sync::Arc;

#[cfg(target_os = "macos")]
use crate::macos_audio::ApplicationCaptureState;
#[cfg(target_os = "macos")]
mod application_capture;
#[cfg(target_os = "macos")]
pub(crate) mod local;
pub(crate) mod node_control;
pub(crate) mod recorder;
pub(crate) mod workspace;

const CONTROL_ORIGIN: &str = "http://127.0.0.1:19341";
const GRAPH_CATALOG_ROUTE: &str = "/api/v1/graphs";

fn graph_layout_route(graph_id: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(CONTROL_ORIGIN).map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|()| "invalid control origin".to_owned())?
        .extend(["api", "v1", "graphs", graph_id, "layout"]);
    Ok(url.path().to_owned())
}

// Layout writes do not prepare macOS application taps or submit audio revisions.
#[tauri::command]
pub(crate) async fn control_get_graph_layout(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::GET, &graph_layout_route(&graph_id)?, None)
        .await
}

#[tauri::command]
pub(crate) async fn control_set_graph_layout(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::PUT, &graph_layout_route(&graph_id)?, Some(request))
        .await
}

#[derive(Debug, Clone)]
pub(crate) struct ControlProxyClient {
    client: Client,
    #[cfg(target_os = "macos")]
    capture_gate: Arc<tauri::async_runtime::Mutex<()>>,
    closing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    requests: std::sync::Arc<tokio::sync::RwLock<()>>,
}

impl Default for ControlProxyClient {
    fn default() -> Self {
        Self {
            client: Client::new(),
            closing: std::sync::Arc::default(),
            requests: std::sync::Arc::default(),
            #[cfg(target_os = "macos")]
            capture_gate: Arc::default(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ControlProxyResponse {
    status: u16,
    body: Value,
    #[cfg(target_os = "macos")]
    #[serde(skip)]
    delivery: RequestDelivery,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestDelivery {
    ServerResponse,
    NotSent,
    Uncertain,
}

impl ControlProxyClient {
    pub(crate) fn close_admission(&self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
    }

    #[cfg(target_os = "macos")]
    pub(crate) async fn drain(&self) -> Result<(), String> {
        tokio::time::timeout(std::time::Duration::from_secs(35), async {
            // Same order as capture-aware commands: preparation, then HTTP admission.
            let _capture = self.capture_gate.lock().await;
            let _requests = self.requests.write().await;
        })
        .await
        .map_err(|_| "Controls are still finishing; audio ownership was retained.".to_owned())
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<ControlProxyResponse, String> {
        let _admission = self.requests.read().await;
        if self.closing.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(rejected("control_unavailable"));
        }
        let mut request = self
            .client
            .request(method, format!("{CONTROL_ORIGIN}{path}"))
            .timeout(std::time::Duration::from_secs(30));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(path, ?error, "control request failed");
                let code = if error.is_connect() || error.is_timeout() {
                    "control_unavailable"
                } else {
                    "control_request_failed"
                };
                return Ok(ControlProxyResponse {
                    status: 503,
                    #[cfg(target_os = "macos")]
                    delivery: if error.is_connect() {
                        RequestDelivery::NotSent
                    } else {
                        RequestDelivery::Uncertain
                    },
                    body: serde_json::json!({
                        "code": code,
                        "parameters": { "path": path, "reason": error.to_string() },
                    }),
                });
            }
        };
        let status = response.status().as_u16();
        let body = response.json().await.map_err(|error| {
            tracing::warn!(path, status, ?error, "invalid control response");
            format!("local control response was invalid: {error}")
        })?;
        Ok(ControlProxyResponse {
            status,
            body,
            #[cfg(target_os = "macos")]
            delivery: RequestDelivery::ServerResponse,
        })
    }
}

fn rejected(code: &str) -> ControlProxyResponse {
    ControlProxyResponse {
        status: 409,
        body: serde_json::json!({"code": code, "parameters": {"nodeControlOutcome": "not_applied"}}),
        #[cfg(target_os = "macos")]
        delivery: RequestDelivery::NotSent,
    }
}

#[tauri::command]
pub(crate) async fn control_get_status(
    client: tauri::State<'_, ControlProxyClient>,
    #[cfg(target_os = "macos")] captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
) -> Result<ControlProxyResponse, String> {
    let response = client.request(Method::GET, "/api/v1/status", None).await?;
    #[cfg(target_os = "macos")]
    return application_capture::power_status(&client, captures.inner(), response).await;
    #[cfg(not(target_os = "macos"))]
    Ok(response)
}

#[tauri::command]
pub(crate) async fn control_reconfigure_session(
    client: tauri::State<'_, ControlProxyClient>,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::PUT, "/api/v1/audio/session", Some(request))
        .await
}

#[tauri::command]
pub(crate) async fn control_set_engine_power(
    client: tauri::State<'_, ControlProxyClient>,
    #[cfg(target_os = "macos")] captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
    enabled: bool,
) -> Result<ControlProxyResponse, String> {
    #[cfg(target_os = "macos")]
    let _guard = client.capture_gate.lock().await;
    #[cfg(target_os = "macos")]
    if enabled {
        return application_capture::enable(&client, captures.inner()).await;
    }
    let response = client
        .request(
            Method::PUT,
            "/api/v1/audio/power",
            Some(serde_json::json!({"enabled":enabled})),
        )
        .await?;
    #[cfg(target_os = "macos")]
    return application_capture::synchronize_response(&client, captures.inner(), response).await;
    #[cfg(not(target_os = "macos"))]
    Ok(response)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn control_get_graph(
    client: tauri::State<'_, ControlProxyClient>,
) -> Result<ControlProxyResponse, String> {
    client.request(Method::GET, "/api/v1/graph", None).await
}

#[tauri::command]
pub(crate) async fn control_set_virtual_endpoint_sample_rate(
    client: tauri::State<'_, ControlProxyClient>,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(
            Method::PUT,
            "/api/v1/audio/virtual-endpoint/format",
            Some(request),
        )
        .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn control_get_graph(
    client: tauri::State<'_, ControlProxyClient>,
    captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
) -> Result<ControlProxyResponse, String> {
    let _guard = client.capture_gate.lock().await;
    let response = client.request(Method::GET, "/api/v1/graph", None).await?;
    application_capture::synchronize_response(&client, captures.inner(), response).await
}

#[tauri::command]
pub(crate) async fn control_get_graph_status(
    client: tauri::State<'_, ControlProxyClient>,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::GET, "/api/v1/graph/status", None)
        .await
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn control_get_graph_catalog(
    client: tauri::State<'_, ControlProxyClient>,
) -> Result<ControlProxyResponse, String> {
    client.request(Method::GET, GRAPH_CATALOG_ROUTE, None).await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn control_get_graph_catalog(
    client: tauri::State<'_, ControlProxyClient>,
    captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
) -> Result<ControlProxyResponse, String> {
    let _guard = client.capture_gate.lock().await;
    let response = client
        .request(Method::GET, GRAPH_CATALOG_ROUTE, None)
        .await?;
    application_capture::synchronize_response(&client, captures.inner(), response).await
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn control_mutate_graph_catalog(
    client: tauri::State<'_, ControlProxyClient>,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::PUT, GRAPH_CATALOG_ROUTE, Some(request))
        .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn control_mutate_graph_catalog(
    client: tauri::State<'_, ControlProxyClient>,
    captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    let _guard = client.capture_gate.lock().await;
    let response = client
        .request(Method::PUT, GRAPH_CATALOG_ROUTE, Some(request))
        .await?;
    application_capture::synchronize_response(&client, captures.inner(), response).await
}

#[tauri::command]
pub(crate) async fn control_get_meters(
    client: tauri::State<'_, ControlProxyClient>,
) -> Result<ControlProxyResponse, String> {
    client.request(Method::GET, "/api/v1/meters", None).await
}

#[tauri::command]
pub(crate) async fn control_get_processors(
    client: tauri::State<'_, ControlProxyClient>,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::GET, "/api/v1/processors", None)
        .await
}

#[tauri::command]
pub(crate) async fn control_get_processor_editor(
    client: tauri::State<'_, ControlProxyClient>,
    node_id: String,
) -> Result<ControlProxyResponse, String> {
    let node_id = percent_encode_path_segment(&node_id);
    client
        .request(
            Method::GET,
            &format!("/api/v1/processors/{node_id}/editor"),
            None,
        )
        .await
}

#[tauri::command]
pub(crate) async fn control_rescan_processors(
    client: tauri::State<'_, ControlProxyClient>,
    paths: Vec<String>,
) -> Result<ControlProxyResponse, String> {
    client
        .request(
            Method::PUT,
            "/api/v1/processors/scan",
            Some(serde_json::json!({ "paths": paths })),
        )
        .await
}

#[tauri::command]
pub(crate) async fn control_set_processor_parameter(
    client: tauri::State<'_, ControlProxyClient>,
    node_id: String,
    parameter_id: u32,
    value: f64,
    expected_revision: u64,
) -> Result<ControlProxyResponse, String> {
    let node_id = percent_encode_path_segment(&node_id);
    let path = format!("/api/v1/processors/{node_id}/parameters/{parameter_id}");
    client
        .request(
            Method::PUT,
            &path,
            Some(serde_json::json!({ "value": value, "expectedRevision": expected_revision })),
        )
        .await
}

#[tauri::command]
pub(crate) async fn control_set_processor_editor(
    client: tauri::State<'_, ControlProxyClient>,
    node_id: String,
    open: bool,
    expected_revision: u64,
) -> Result<ControlProxyResponse, String> {
    let node_id = percent_encode_path_segment(&node_id);
    client
        .request(
            Method::PUT,
            &format!("/api/v1/processors/{node_id}/editor"),
            Some(serde_json::json!({ "open": open, "expectedRevision": expected_revision })),
        )
        .await
}

fn percent_encode_path_segment(value: &str) -> String {
    value.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
        encoded
    })
}

#[tauri::command]
pub(crate) async fn control_get_processor_editor_frame(
    client: tauri::State<'_, ControlProxyClient>,
    node_id: String,
) -> Result<ControlProxyResponse, String> {
    let node_id = percent_encode_path_segment(&node_id);
    client
        .request(
            Method::GET,
            &format!("/api/v1/processors/{node_id}/editor/frame"),
            None,
        )
        .await
}

#[tauri::command]
pub(crate) async fn control_send_processor_editor_input(
    client: tauri::State<'_, ControlProxyClient>,
    node_id: String,
    input: serde_json::Value,
) -> Result<ControlProxyResponse, String> {
    let node_id = percent_encode_path_segment(&node_id);
    client
        .request(
            Method::POST,
            &format!("/api/v1/processors/{node_id}/editor/input"),
            Some(input),
        )
        .await
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn control_apply_graph(
    client: tauri::State<'_, ControlProxyClient>,
    document: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::PUT, "/api/v1/graph", Some(document))
        .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn control_apply_graph(
    client: tauri::State<'_, ControlProxyClient>,
    captures: tauri::State<'_, Arc<ApplicationCaptureState>>,
    document: Value,
) -> Result<ControlProxyResponse, String> {
    let _guard = client.capture_gate.lock().await;
    application_capture::apply(&client, captures.inner(), document).await
}

// Only the macOS process-tap lifecycle needs to branch on the proxied status.
// Keep this helper behind the same boundary as its callers on other targets.
#[cfg(target_os = "macos")]
impl ControlProxyResponse {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[cfg(target_os = "macos")]
fn application_capture_error(reason: &str) -> ControlProxyResponse {
    ControlProxyResponse {
        status: 503,
        delivery: RequestDelivery::NotSent,
        body: serde_json::json!({
            "code": "audio_source_unavailable",
            "parameters": { "reason": reason },
        }),
    }
}

#[cfg(test)]
mod layout_tests {
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn closing_rejects_requests_and_waits_for_accepted_work() {
        let client = super::ControlProxyClient::default();
        let accepted = client.requests.read().await;
        client.close_admission();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), client.drain())
                .await
                .is_err()
        );
        drop(accepted);
        client.drain().await.expect("drained");
        let response = client
            .request(super::Method::PUT, "/not-sent", None)
            .await
            .expect("response");
        assert_eq!(response.delivery, super::RequestDelivery::NotSent);
        assert_eq!(response.status, 409);
    }

    #[test]
    fn layout_route_encodes_one_graph_segment_without_changing_the_endpoint() {
        assert_eq!(
            super::graph_layout_route("graph-1").unwrap(),
            "/api/v1/graphs/graph-1/layout"
        );
        assert_eq!(
            super::graph_layout_route("a/b?c#d").unwrap(),
            "/api/v1/graphs/a%2Fb%3Fc%23d/layout"
        );
    }
}
