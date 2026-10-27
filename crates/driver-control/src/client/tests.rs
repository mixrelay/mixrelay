use mixrelay_audio_io::VirtualEndpointId;

use super::*;

mod blocking;

#[test]
fn clients_use_the_socket_directories_and_commands_use_canonical_wire_fields() {
    let client = DriverClient::with_socket_path("/fixture/driver.sock").unwrap();
    assert_eq!(client.endpoint, "http://mixrelay-driver/api/v1/driver");
    // Whatever this environment holds, the constructors follow the directory rules.
    assert_eq!(
        DriverClient::new().ok().map(|client| client.socket_path),
        socket_directory()
            .ok()
            .map(|directory| directory.join(DRIVER_SOCKET))
    );
    assert_eq!(
        DriverClient::session()
            .ok()
            .map(|client| client.socket_path),
        session_socket_directory()
            .ok()
            .map(|directory| directory.join(DRIVER_SOCKET))
    );
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
    let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
    let (backend, observed) = crate::test_backend::FakeBackend::ready();
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let expected = observed.lock().settings.devices.clone();
    let (client, service) = serve(&fixture, crate::router(manager)).await;
    let stopped = client.set_active(false).await.unwrap();
    assert!(stopped.settings.devices.is_empty());
    assert!(observed.lock().settings.devices.is_empty());
    let resumed = client.set_active(true).await.unwrap();
    assert_eq!(resumed.settings.devices, expected);
    service.abort();
    assert!(service.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn absent_native_driver_can_quit_without_attempting_a_stop() {
    let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
    let (backend, observed) = crate::test_backend::FakeBackend::ready();
    observed.lock().state = crate::DriverState::NotInstalled;
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let (client, service) = serve(&fixture, crate::router(manager)).await;
    let absent = client.set_active(false).await.unwrap();
    assert_eq!(absent.state, crate::DriverState::NotInstalled);
    assert!(absent.settings.devices.is_empty());
    assert_eq!(observed.lock().applies, 0);
    // An installed but unloaded driver must not be treated as safely stopped.
    observed.lock().state = crate::DriverState::InstalledNotLoaded;
    assert!(client.set_active(false).await.is_err());
    assert_eq!(observed.lock().applies, 0);
    service.abort();
    assert!(service.await.unwrap_err().is_cancelled());
}

async fn serve(
    fixture: &mixrelay_test_support::FixtureDir,
    app: axum::Router,
) -> (DriverClient, tokio::task::JoinHandle<()>) {
    let uid = mixrelay_local_socket::current_uid();
    let (listener, socket) = mixrelay_local_socket::bind(fixture.path(), DRIVER_SOCKET, uid)
        .await
        .unwrap();
    let client = DriverClient::with_socket_path(socket.path()).unwrap();
    let listener = mixrelay_local_socket::AuthorizedListener::new(listener, socket, vec![uid, 0]);
    let service = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (client, service)
}

#[tokio::test]
async fn blocking_and_async_clients_share_socket_inventory_and_revision_errors() {
    let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
    let (backend, observed) = crate::test_backend::FakeBackend::ready();
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let (client, service) = serve(&fixture, crate::router(manager)).await;
    let initial = client.snapshot().await.unwrap();
    let worker = client.clone();
    let stopped = tokio::task::spawn_blocking(move || {
        let snapshot = worker.snapshot_blocking().unwrap();
        assert_eq!(snapshot, initial);
        let configured = worker
            .mutate_blocking(&DriverCommand::Configure {
                expected_revision: snapshot.revision,
                spec: snapshot.settings.devices[0].spec.clone(),
            })
            .unwrap();
        let stopped = worker
            .mutate_blocking(&DriverCommand::Deactivate {
                expected_revision: configured.revision,
            })
            .unwrap();
        assert!(stopped.settings.devices.is_empty());
        assert!(matches!(
            worker.mutate_blocking(&DriverCommand::Activate {
                expected_revision: 0
            }),
            Err(DriverError::Remote { status: 409, .. })
        ));
        stopped
    })
    .await
    .unwrap();
    assert_eq!(client.snapshot().await.unwrap(), stopped);
    let resumed = client.set_active(true).await.unwrap();
    assert_eq!(resumed.settings.devices, observed.lock().settings.devices);
    let configured = client
        .mutate(&DriverCommand::Configure {
            expected_revision: resumed.revision,
            spec: resumed.settings.devices[0].spec.clone(),
        })
        .await
        .unwrap();
    assert_eq!(configured.settings.devices, resumed.settings.devices);
    service.abort();
    assert!(service.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn explicit_missing_socket_fails_without_falling_back_to_session_service() {
    let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
    let client = DriverClient::with_socket_path(fixture.join(DRIVER_SOCKET)).unwrap();
    assert!(matches!(client.snapshot().await, Err(DriverError::Http(_))));
    assert!(matches!(
        client
            .mutate(&DriverCommand::Activate {
                expected_revision: 0
            })
            .await,
        Err(DriverError::Http(_))
    ));
    tokio::task::spawn_blocking(move || {
        assert!(matches!(
            client.snapshot_blocking(),
            Err(DriverError::Http(_))
        ));
        assert!(matches!(
            client.mutate_blocking(&DriverCommand::Deactivate {
                expected_revision: 0
            }),
            Err(DriverError::Http(_))
        ));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn socket_clients_keep_response_limits_json_errors_and_no_redirects() {
    use axum::{
        Router,
        http::{StatusCode, header::LOCATION},
        response::IntoResponse,
        routing::get,
    };
    for (status, bytes) in [
        (StatusCode::OK, vec![b'x'; JSON_LIMIT + 1]),
        (StatusCode::OK, b"not JSON".to_vec()),
        (StatusCode::BAD_GATEWAY, b"service unavailable".to_vec()),
        (StatusCode::TEMPORARY_REDIRECT, b"redirect refused".to_vec()),
    ] {
        let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
        let app = Router::new().route(
            SNAPSHOT_ROUTE,
            get(move || {
                let bytes = bytes.clone();
                async move { (status, [(LOCATION, "/unexpected")], bytes).into_response() }
            }),
        );
        let (client, service) = serve(&fixture, app).await;
        let asynchronous = client.snapshot().await.unwrap_err();
        let blocking = tokio::task::spawn_blocking(move || client.snapshot_blocking())
            .await
            .unwrap()
            .unwrap_err();
        match asynchronous {
            DriverError::Invalid(message) => {
                assert_eq!(message, "Driver JSON exceeds 1 MiB");
                assert!(matches!(blocking, DriverError::Invalid(_)));
            }
            DriverError::Json(_) => assert!(matches!(blocking, DriverError::Json(_))),
            DriverError::Remote {
                status: actual,
                message,
            } => {
                assert_eq!(actual, status.as_u16());
                assert!(
                    matches!(blocking, DriverError::Remote { status, message: detail }
                    if status == actual && detail == message)
                );
            }
            error => panic!("unexpected response error: {error}"),
        }
        service.abort();
        assert!(service.await.unwrap_err().is_cancelled());
    }
}

#[test]
fn runtime_latency_replies_decode_and_rejections_stay_errors() {
    assert!(matches!(
        decode_latency_response(reqwest::StatusCode::NOT_FOUND, b"not found"),
        Err(DriverError::Remote { status: 404, .. })
    ));
    assert_eq!(
        decode_latency_response(
            reqwest::StatusCode::OK,
            br#"{"accepted":true,"lease":null}"#
        )
        .unwrap(),
        crate::LatencyReply {
            accepted: true,
            lease: None
        }
    );
    assert!(matches!(
        decode_latency_response(reqwest::StatusCode::CONFLICT, b"lease conflict"),
        Err(DriverError::Remote { status: 409, .. })
    ));
    assert!(decode_latency_response(reqwest::StatusCode::OK, b"invalid").is_err());
}

#[tokio::test]
async fn claiming_clients_reach_the_policy_with_their_verified_peer() {
    let fixture = mixrelay_test_support::FixtureDir::ipc("drv").unwrap();
    let (backend, _) = crate::test_backend::FakeBackend::ready();
    let manager = crate::DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let record = seen.clone();
    let app = crate::router_with_access(
        manager,
        std::sync::Arc::new(move |request: &crate::AccessRequest| {
            record.lock().push(*request);
            request.claimed
        }),
    );
    let uid = mixrelay_local_socket::current_uid();
    let (listener, socket) = mixrelay_local_socket::bind(fixture.path(), DRIVER_SOCKET, uid)
        .await
        .unwrap();
    let plain = DriverClient::with_socket_path(socket.path()).unwrap();
    let listener = mixrelay_local_socket::AuthorizedListener::new(listener, socket, vec![uid]);
    let service = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<mixrelay_local_socket::Peer>(),
        )
        .await
        .unwrap();
    });
    let claiming = plain.clone().claiming_paid_access(true);
    assert!(!plain.snapshot().await.unwrap().settings.can_create);
    assert!(claiming.snapshot().await.unwrap().settings.can_create);
    let blocking = claiming.clone();
    let snapshot = tokio::task::spawn_blocking(move || {
        let snapshot = blocking.snapshot_blocking().unwrap();
        assert!(snapshot.settings.can_create);
        let plain = blocking.claiming_paid_access(false);
        assert!(!plain.snapshot_blocking().unwrap().settings.can_create);
        snapshot
    })
    .await
    .unwrap();
    assert!(snapshot.settings.can_create);
    let seen = seen.lock().clone();
    assert_eq!(
        seen.iter()
            .map(|request| request.claimed)
            .collect::<Vec<_>>(),
        [false, true, true, false]
    );
    for request in seen {
        let peer = request.peer.expect("connection info");
        assert_eq!(peer.uid, uid);
        assert_eq!(peer.audit_token.is_some(), cfg!(target_os = "macos"));
    }
    service.abort();
}
