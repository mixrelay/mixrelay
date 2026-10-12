use super::*;
async fn send(backend: Arc<TestBackend>, body: serde_json::Value) -> axum::response::Response {
    router_with_backend(backend, ControlOperations::new())
        .oneshot(
            Request::put(VIRTUAL_ENDPOINT_BUFFER_ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn endpoint_buffer_routes_owning_runtime_identity_and_native_observation() {
    let backend = TestBackend::with_result(Ok(GraphApplyResponse {
        protocol_version: PROTOCOL_VERSION,
        revision: 7,
        state: GraphApplyState::Active,
        persistence_error: None,
    }));
    let response = send(
        backend.clone(),
        json!({"expectedRevision":7,"endpointId":"selected-device",
        "expectedBufferFrames":512,"expectedPreferredFrames":256,"bufferFrames":128}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let request = backend.last_endpoint_buffer.lock().unwrap();
    let request = request.as_ref().unwrap();
    assert_eq!(request.expected_revision, 7);
    assert_eq!(request.endpoint_id.as_str(), "selected-device");
    assert_eq!(request.expected_buffer_frames, Some(512));
    assert_eq!(request.expected_preferred_frames, Some(256));
    assert_eq!(request.buffer_frames, 128);
    assert!(backend.last_apply.lock().unwrap().is_none());
}
#[tokio::test]
async fn endpoint_buffer_rejects_malformed_requests_without_mutation() {
    for body in [
        json!({"bufferFrames":128}),
        json!({"expectedRevision":7,"endpointId":"selected-device",
        "expectedBufferFrames":512,"bufferFrames":-1}),
        json!({"expectedRevision":7,"endpointId":"selected-device",
        "expectedBufferFrames":512,"bufferFrames":128,"global":true}),
    ] {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 7,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        assert_eq!(
            send(backend.clone(), body).await.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert!(backend.last_endpoint_buffer.lock().unwrap().is_none());
    }
}
#[tokio::test]
async fn endpoint_buffer_preserves_native_failure_and_saved_intent_errors() {
    let backend = TestBackend::with_result(Ok(GraphApplyResponse {
        protocol_version: PROTOCOL_VERSION,
        revision: 7,
        state: GraphApplyState::Active,
        persistence_error: Some("save failed after native apply".into()),
    }));
    let response = send(
        backend,
        json!({"expectedRevision":7,"endpointId":"selected-device",
        "expectedBufferFrames":null,"bufferFrames":128}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let response: GraphApplyResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(response.persistence_error.is_some());
}

#[tokio::test]
async fn endpoint_buffer_native_rejection_preserves_protocol_error() {
    let error = ProtocolError {
        code: ErrorCode::AudioSessionReconfigureFailed,
        parameters: json!({"reason":"native readback failed"}),
    };
    let backend = TestBackend::with_result(Err(error.clone()));
    let response = send(backend, json!({"expectedRevision":7,"endpointId":"selected-device", "expectedBufferFrames":512,"expectedPreferredFrames":128,"bufferFrames":256})).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<ProtocolError>(&bytes).unwrap(),
        error
    );
}
