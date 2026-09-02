use axum::{Json, Router, routing::get};
use tonenari_control_protocol::AppStatus;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/status", get(status))
}

async fn status() -> Json<AppStatus> {
    Json(AppStatus::default())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn status_endpoint_uses_versioned_contract() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let status: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(status["protocolVersion"], 1);
        assert_eq!(status["driverState"], "not_installed");
    }
}
