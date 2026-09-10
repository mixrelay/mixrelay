use super::{ControlProxyClient, ControlProxyResponse, Method, Value};

fn route(graph_id: &str, node_id: Option<&str>) -> Result<String, String> {
    let mut url = reqwest::Url::parse(super::CONTROL_ORIGIN).map_err(|error| error.to_string())?;
    let mut segments = url
        .path_segments_mut()
        .map_err(|()| "invalid control origin")?;
    segments.extend(["api", "v1", "graphs", graph_id, "nodes"]);
    if let Some(node) = node_id {
        segments.extend([node, "control"]);
    } else {
        segments.push("controls");
    }
    drop(segments);
    Ok(url.path().to_owned())
}

/// Reads supported controls and authoritative states for one graph.
#[tauri::command]
pub(crate) async fn control_get_node_controls(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::GET, &route(&graph_id, None)?, None)
        .await
}

/// Applies node control through the shared capture reservation/publication path.
/// Returns transport/preparation errors without committing partial native captures.
#[tauri::command]
pub(crate) async fn control_update_node_control(
    client: tauri::State<'_, ControlProxyClient>,
    #[cfg(target_os = "macos")] captures: tauri::State<
        '_,
        std::sync::Arc<crate::macos_audio::ApplicationCaptureState>,
    >,
    graph_id: String,
    node_id: String,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    let route = route(&graph_id, Some(&node_id))?;
    #[cfg(target_os = "macos")]
    {
        let _guard = client.capture_gate.lock().await;
        super::application_capture::node_control(&client, captures.inner(), route, request).await
    }
    #[cfg(not(target_os = "macos"))]
    client.request(Method::PUT, &route, Some(request)).await
}

#[cfg(test)]
mod tests {
    use super::route;

    #[test]
    fn catalog_and_command_routes_keep_graph_and_node_identity_separate() {
        assert_eq!(
            route("main", None).unwrap(),
            "/api/v1/graphs/main/nodes/controls"
        );
        assert_eq!(
            route("graph/2", Some("processor/?#")).unwrap(),
            "/api/v1/graphs/graph%2F2/nodes/processor%2F%3F%23/control"
        );
    }
}
