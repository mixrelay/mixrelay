use mixrelay_audio_io::VirtualEndpointId;

use super::*;

#[test]
fn client_is_fixed_loopback_and_commands_use_canonical_wire_fields() {
    let client = DriverClient::new().unwrap();
    assert_eq!(client.endpoint, "http://127.0.0.1:19342/api/v1/driver");
    let command = DriverCommand::Remove {
        expected_revision: 7,
        endpoint_id: VirtualEndpointId::from("fixture"),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&command_bytes(&command).unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"action":"remove","expectedRevision":7,"endpointId":"fixture"})
    );
    assert_eq!(
        serde_json::from_value::<DriverCommand>(value).unwrap(),
        command
    );
}

#[test]
fn shared_sample_format_command_uses_one_pcm_field_without_direction() {
    let command = DriverCommand::SetSampleFormat {
        expected_revision: 7,
        endpoint_id: VirtualEndpointId::from("fixture"),
        sample_format: mixrelay_audio_io::NativeSampleFormat::Signed16,
    };
    let value: serde_json::Value =
        serde_json::from_slice(&command_bytes(&command).unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "action": "set_sample_format",
            "expectedRevision": 7,
            "endpointId": "fixture",
            "sampleFormat": "signed16",
        })
    );
    assert_eq!(
        serde_json::from_value::<DriverCommand>(value).unwrap(),
        command
    );
}

#[test]
fn oversized_commands_and_replies_are_rejected_before_json_decode() {
    let endpoint_id = VirtualEndpointId::new("x".repeat(JSON_LIMIT));
    assert!(
        command_bytes(&DriverCommand::Remove {
            expected_revision: 0,
            endpoint_id
        })
        .is_err()
    );
    assert!(check_size(JSON_LIMIT).is_ok());
    assert!(check_size(JSON_LIMIT + 1).is_err());
}

#[test]
fn service_rejections_keep_status_and_error_message() {
    let error = decode_response(
        reqwest::StatusCode::CONFLICT,
        br#"{"error":"Driver revision changed"}"#,
    )
    .unwrap_err();
    assert!(
        matches!(error, DriverError::Remote { status: 409, message } if message == "Driver revision changed")
    );
    assert!(decode_response(reqwest::StatusCode::OK, b"not JSON").is_err());
}

#[tokio::test]
async fn lifecycle_client_stops_real_http_inventory_and_reactivates_retained_settings() {
    use axum::http::{HeaderMap, HeaderValue, header::HOST};
    let fixture = mixrelay_test_support::FixtureDir::new("driver-lifecycle-client").unwrap();
    let (backend, observed) = crate::test_backend::FakeBackend::ready();
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let expected = observed.lock().settings.devices.clone();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = tokio::spawn(async move {
        axum::serve(listener, crate::router(manager)).await.unwrap();
    });
    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("127.0.0.1:19342"));
    let client = DriverClient {
        client: reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .unwrap(),
        endpoint: format!("http://{address}{SNAPSHOT_ROUTE}"),
    };
    let stopped = client.set_active(false).await.unwrap();
    assert!(stopped.settings.devices.is_empty());
    assert!(observed.lock().settings.devices.is_empty());
    let resumed = client.set_active(true).await.unwrap();
    assert_eq!(resumed.settings.devices, expected);
    service.abort();
}

#[tokio::test]
async fn absent_native_driver_can_quit_without_attempting_a_stop() {
    use axum::http::{HeaderMap, HeaderValue, header::HOST};
    let fixture = mixrelay_test_support::FixtureDir::new("driver-absent-quit").unwrap();
    let (backend, observed) = crate::test_backend::FakeBackend::ready();
    observed.lock().state = crate::DriverState::NotInstalled;
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = tokio::spawn(async move {
        axum::serve(listener, crate::router(manager)).await.unwrap();
    });
    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("127.0.0.1:19342"));
    let client = DriverClient {
        client: reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .unwrap(),
        endpoint: format!("http://{address}{SNAPSHOT_ROUTE}"),
    };
    let absent = client.set_active(false).await.unwrap();
    assert_eq!(absent.state, crate::DriverState::NotInstalled);
    assert!(absent.settings.devices.is_empty());
    assert_eq!(observed.lock().applies, 0);
    // An installed but unloaded driver must not be treated as safely stopped.
    observed.lock().state = crate::DriverState::InstalledNotLoaded;
    assert!(client.set_active(false).await.is_err());
    assert_eq!(observed.lock().applies, 0);
    service.abort();
}
