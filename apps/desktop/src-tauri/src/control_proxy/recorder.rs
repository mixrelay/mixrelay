use reqwest::Method;
use serde_json::Value;

use super::{ControlProxyClient, ControlProxyResponse, percent_encode_path_segment};

fn route(graph_id: &str, node_id: &str) -> String {
    format!(
        "/api/v1/graphs/{}/recorders/{}",
        percent_encode_path_segment(graph_id),
        percent_encode_path_segment(node_id)
    )
}

#[tauri::command]
pub(crate) async fn control_get_recorder_status(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
    node_id: String,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::GET, &route(&graph_id, &node_id), None)
        .await
}

#[tauri::command]
pub(crate) async fn control_set_recorder(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
    node_id: String,
    request: Value,
) -> Result<ControlProxyResponse, String> {
    client
        .request(Method::PUT, &route(&graph_id, &node_id), Some(request))
        .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn route_preserves_graph_and_node_segments() {
        assert_eq!(
            super::route("a/b", "c?d"),
            "/api/v1/graphs/a%2Fb/recorders/c%3Fd"
        );
    }
}
