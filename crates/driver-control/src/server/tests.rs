use axum::{body::Body, http::Request};
use mixrelay_test_support::FixtureDir;
use tower::ServiceExt as _;

use super::*;
use crate::test_backend::FakeBackend;

fn app(fixture: &FixtureDir) -> Router {
    let (backend, _) = FakeBackend::ready();
    router(DriverManager::open(backend, fixture.join("drivers.json")).unwrap())
}

#[tokio::test]
async fn snapshot_uses_independent_native_state_and_version() {
    let fixture = FixtureDir::new("driver-http-snapshot").unwrap();
    let (backend, _) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    manager.set_installed_version(Some("0.1.0".into()));
    let response = router(manager)
        .oneshot(
            Request::builder()
                .uri(SNAPSHOT_ROUTE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), JSON_LIMIT).await.unwrap();
    let snapshot: DriverSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snapshot.state, crate::DriverState::Ready);
    assert_eq!(snapshot.protocol_version, 1);
    assert_eq!(snapshot.installed_version.as_deref(), Some("0.1.0"));
    assert_eq!(snapshot.owner_process_id, Some(std::process::id()));
}

#[tokio::test]
async fn maintenance_initializes_later_ready_inventory_without_requests() {
    let fixture = FixtureDir::new("driver-http-startup").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    observed.lock().state = crate::DriverState::InstalledNotLoaded;
    let manager = Arc::new(Mutex::new(DriverManager::open(backend, &path).unwrap()));
    let maintenance = tokio::spawn(maintain_inventory(Arc::downgrade(&manager)));
    assert!(!path.exists());
    observed.lock().state = crate::DriverState::Ready;
    tokio::time::timeout(Duration::from_secs(3), async {
        while observed.lock().applies == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initialization completed without a request");
    assert_eq!(observed.lock().applies, 1);
    assert_eq!(
        manager.lock().snapshot().unwrap().state,
        crate::DriverState::Ready
    );
    assert!(path.exists());
    drop(manager);
    tokio::time::timeout(Duration::from_secs(3), maintenance)
        .await
        .expect("maintenance released its owner")
        .unwrap();
}

#[tokio::test]
async fn unavailable_router_reports_native_state_and_releases_its_owner_on_drop() {
    let fixture = FixtureDir::new("driver-http-unloaded").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    observed.lock().state = crate::DriverState::InstalledNotLoaded;
    let app = router(DriverManager::open(backend, &path).unwrap());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(SNAPSHOT_ROUTE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), JSON_LIMIT).await.unwrap();
    let snapshot: DriverSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snapshot.state, crate::DriverState::InstalledNotLoaded);
    assert!(!snapshot.settings.can_create);
    assert!(!path.exists());
    drop(app);
    tokio::time::timeout(Duration::from_secs(3), async {
        while Arc::strong_count(&observed) != 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("waiting startup task did not retain the router owner");
    observed.lock().state = crate::DriverState::Ready;
    assert_eq!(observed.lock().applies, 0);
    assert!(!path.exists());
}

#[tokio::test]
async fn waiting_startup_task_exits_when_its_router_owner_is_gone() {
    let fixture = FixtureDir::new("driver-http-startup-cancel").unwrap();
    let (backend, observed) = FakeBackend::ready();
    observed.lock().state = crate::DriverState::InstalledNotLoaded;
    let manager = Arc::new(Mutex::new(
        DriverManager::open(backend, fixture.join("drivers.json")).unwrap(),
    ));
    let owner = Arc::downgrade(&manager);
    let startup = tokio::spawn(maintain_inventory(owner.clone()));
    drop(manager);
    tokio::time::timeout(Duration::from_secs(3), startup)
        .await
        .expect("cancelled startup task exited")
        .unwrap();
    assert!(owner.upgrade().is_none());
    assert_eq!(observed.lock().applies, 0);
}

#[tokio::test]
async fn maintenance_recovers_failed_inventory_without_requests_and_releases_router_on_drop() {
    let fixture = FixtureDir::new("driver-http-recovery").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let manager = DriverManager::open(backend, &path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let readbacks = observed.lock().readbacks;
    let id = observed.lock().settings.devices[0]
        .spec
        .id
        .as_str()
        .to_owned();
    let app = router(manager);
    {
        let mut native = observed.lock();
        native.state = crate::DriverState::Unavailable;
        native.failed_devices.insert(id.clone());
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while observed.lock().readbacks == readbacks {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("failed owner recovered without an API request");
    assert_eq!(observed.lock().recovered_devices, [id]);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(observed.lock().readbacks, readbacks + 1);
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    drop(app);
    tokio::time::timeout(Duration::from_secs(3), async {
        while Arc::strong_count(&observed) != 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("maintenance retained a dropped router owner");
    assert_eq!(observed.lock().applies, 2);
}

#[tokio::test]
async fn untrusted_origins_hosts_and_non_json_mutations_are_rejected() {
    let fixture = FixtureDir::new("driver-http-origin").unwrap();
    let app = app(&fixture);
    for (header, value) in [
        (ORIGIN, "https://example.com"),
        (ORIGIN, "null"),
        (HOST, "example.com:19342"),
    ] {
        let request = Request::builder()
            .method(Method::PUT)
            .uri(SNAPSHOT_ROUTE)
            .header(header, value)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let request = Request::builder()
        .method(Method::PUT)
        .uri(SNAPSHOT_ROUTE)
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

#[tokio::test]
async fn exact_local_origin_is_allowed_and_body_limit_is_enforced() {
    let fixture = FixtureDir::new("driver-http-limits").unwrap();
    let app = app(&fixture);
    let request = Request::builder()
        .uri(SNAPSHOT_ROUTE)
        .header(ORIGIN, ORIGINS[0])
        .header(HOST, HOSTS[0])
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        ORIGINS[0]
    );
    let request = Request::builder()
        .method(Method::PUT)
        .uri(SNAPSHOT_ROUTE)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(vec![b'x'; JSON_LIMIT + 1]))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn stale_revision_and_missing_identity_are_distinct_failures() {
    let fixture = FixtureDir::new("driver-http-conflict").unwrap();
    let app = app(&fixture);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(SNAPSHOT_ROUTE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let snapshot: DriverSnapshot =
        serde_json::from_slice(&to_bytes(response.into_body(), JSON_LIMIT).await.unwrap()).unwrap();
    for (revision, expected_status) in [
        (snapshot.revision - 1, StatusCode::CONFLICT),
        (snapshot.revision, StatusCode::NOT_FOUND),
    ] {
        let body = serde_json::json!({"action":"remove","expectedRevision":revision,"endpointId":"absent"}).to_string();
        let request = Request::builder()
            .method(Method::PUT)
            .uri(SNAPSHOT_ROUTE)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected_status
        );
    }
}
