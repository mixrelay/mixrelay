use axum::{Json, Router, routing::get};
use tonenari_control_protocol::AppStatus;

pub const HEALTH_ROUTE: &str = "/health";
pub const STATUS_ROUTE: &str = "/api/v1/status";

pub fn router() -> Router {
    Router::new()
        .route(HEALTH_ROUTE, get(|| async { "ok" }))
        .route(STATUS_ROUTE, get(status))
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
    async fn health_endpoint_is_lightweight_and_ready() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri(HEALTH_ROUTE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 16)
            .await
            .unwrap();

        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn status_endpoint_uses_versioned_contract() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri(STATUS_ROUTE)
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
