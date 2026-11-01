//! Licence policy runs against queued HTTP replies; the real JSON transport runs against a
//! bounded loopback HTTP server.
use super::*;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Mutex,
};

const KEY: &str = "synthetic-test-key";
const ORG: &str = "10000000-0000-4000-8000-000000000001";
const BENEFIT: &str = "20000000-0000-4000-8000-000000000002";
const LICENSE: &str = "70000000-0000-4000-8000-000000000007";
const SEAT: &str = "a0000000-0000-4000-8000-000000000001";
const OTHER: &str = "b0000000-0000-4000-8000-000000000002";

fn product() -> ProductIdentity {
    ProductIdentity::new(ORG, BENEFIT).unwrap()
}
fn key() -> SecretString {
    SecretString::from(KEY)
}
fn license(instance: Option<&str>) -> serde_json::Value {
    serde_json::json!({"id": LICENSE, "organization_id": ORG, "benefit_id": BENEFIT,
        "status":"granted", "limit_activations":100, "expires_at":null,
        "activation":instance.map(|id| serde_json::json!({"id":id,"license_key_id":LICENSE})),
        "key":KEY,"customer":{"email":"do-not-return@example.invalid"}})
}
fn activated() -> serde_json::Value {
    serde_json::json!({"id":SEAT,"license_key_id":LICENSE,"label":"MixRelay Mac", "license_key":license(None)})
}
fn not_found(detail: &str) -> serde_json::Value {
    serde_json::json!({"error":"ResourceNotFound","detail":detail})
}
#[derive(Debug)]
struct Request {
    path: String,
    fields: serde_json::Value,
}
type Requests = Arc<Mutex<Vec<Request>>>;
struct Reply {
    status: u16,
    body: String,
    version: &'static str,
}
impl Reply {
    fn json(status: u16, body: &serde_json::Value) -> Self {
        Self {
            status,
            body: body.to_string(),
            version: API_VERSION,
        }
    }
    fn empty(status: u16) -> Self {
        Self {
            status,
            body: String::new(),
            version: API_VERSION,
        }
    }
}
async fn server(replies: Vec<Reply>) -> (LicenseClient, Requests, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!(
        "http://{}/v1/customer-portal/license-keys",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let task = tokio::spawn(async move {
        for reply in replies {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0 && bytes.len() < 8192);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    assert!(headers.contains("content-type: application/json"));
                    assert!(headers.contains("polar-version: 2026-10"));
                    assert!(!headers.contains("authorization:"));
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0 && bytes.len() < 8192);
                bytes.extend_from_slice(&buffer[..count]);
            }
            let path = String::from_utf8_lossy(&bytes[..header_end])
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_owned();
            let fields = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
            captured.lock().await.push(Request { path, fields });
            // Zero simulates the server committing a mutation then losing the connection.
            if reply.status != 0 {
                let response = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nPolar-Version: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.version,
                    reply.body.len(),
                    reply.body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    let http = Http {
        client: Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap(),
        origin,
    };
    (
        LicenseClient::with_http(product(), Arc::new(http)),
        requests,
        task,
    )
}

/// Answers each request with the next queued reply and records it like the loopback server.
struct QueuedHttp {
    replies: std::sync::Mutex<std::collections::VecDeque<Reply>>,
    requests: Requests,
}

impl LicenseHttp for QueuedHttp {
    fn post<'a>(
        &'a self,
        operation: &'a str,
        fields: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>> {
        Box::pin(async move {
            self.requests.lock().await.push(Request {
                path: format!("/v1/customer-portal/license-keys/{operation}"),
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), serde_json::Value::from(value)))
                    .collect::<serde_json::Map<_, _>>()
                    .into(),
            });
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("a queued reply");
            // Zero stands for a mutation the provider committed before the connection was lost.
            if reply.status == 0 {
                return Err(LicenseError::Unavailable);
            }
            Ok(LicenseHttpResponse {
                status: reply.status,
                body: reply.body.into_bytes(),
            })
        })
    }
}

/// Fails the test unless every queued reply was requested.
struct Queue(Arc<QueuedHttp>);

impl Queue {
    fn finish(self) {
        assert!(self.0.replies.lock().unwrap().is_empty(), "unused replies");
    }
}

fn queued(replies: Vec<Reply>) -> (LicenseClient, Requests, Queue) {
    let http = Arc::new(QueuedHttp {
        replies: std::sync::Mutex::new(replies.into()),
        requests: Requests::default(),
    });
    (
        LicenseClient::with_http(product(), http.clone()),
        http.requests.clone(),
        Queue(http),
    )
}

#[test]
fn configuration_and_arguments_are_checked_without_exposing_keys() {
    for invalid in [
        "",
        "1",
        "00000000-0000-0000-0000-000000000000",
        "not-a-uuid",
    ] {
        assert_eq!(
            ProductIdentity::new(invalid, BENEFIT),
            Err(LicenseError::InvalidConfiguration)
        );
        assert_eq!(
            ProductIdentity::new(ORG, invalid),
            Err(LicenseError::InvalidConfiguration)
        );
    }
    for invalid in ["", " ", "line\nfeed", "null\0byte", &"x".repeat(256)] {
        assert_eq!(check_argument(invalid), Err(LicenseError::InvalidInput));
    }
    assert!(LicenseClient::new(product()).is_ok());
    assert!(!format!("{:?}", key()).contains(KEY));
}

#[tokio::test]
async fn activation_prevalidates_product_and_records_intent_before_one_mutation() {
    let (client, requests, task) = server(vec![
        Reply::json(200, &license(None)),
        Reply::json(200, &activated()),
    ])
    .await;
    let result = client
        .activate(&key(), "MixRelay Mac", || {
            assert_eq!(requests.try_lock().unwrap().len(), 1);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(result.license_id(), LICENSE);
    assert_eq!(result.instance_id(), SEAT);
    task.await.unwrap();
    let requests = requests.lock().await;
    assert_eq!(
        requests[0].path,
        "/v1/customer-portal/license-keys/validate"
    );
    assert_eq!(
        requests[0].fields,
        serde_json::json!({"key":KEY,"organization_id":ORG,"benefit_id":BENEFIT})
    );
    assert_eq!(
        requests[1].path,
        "/v1/customer-portal/license-keys/activate"
    );
    assert_eq!(
        requests[1].fields,
        serde_json::json!({"key":KEY,"organization_id":ORG,"label":"MixRelay Mac"})
    );
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains(KEY) && !encoded.contains("customer"));
}

#[tokio::test]
async fn unlimited_keys_keep_a_readable_local_device_and_never_create_remote_activations() {
    let mut unlimited = license(None);
    unlimited["limit_activations"] = serde_json::Value::Null;
    let (client, requests, task) = queued(vec![
        Reply::json(200, &unlimited),
        Reply::json(200, &unlimited),
    ]);
    let activation = client
        .activate(&key(), "Studio Mac", || panic!("no remote mutation"))
        .await
        .unwrap();
    assert_eq!(activation.instance_id(), "local:Studio Mac");
    assert_eq!(
        client
            .validate(&key(), activation.instance_id())
            .await
            .unwrap(),
        activation
    );
    client
        .deactivate(&key(), activation.instance_id())
        .await
        .unwrap();
    task.finish();
    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.path.ends_with("/validate")
        && r.fields.get("activation_id").is_none()
        && r.fields.get("label").is_none()));
}

#[tokio::test]
async fn foreign_keys_and_failed_intent_never_consume_a_seat() {
    for field in ["organization_id", "benefit_id"] {
        let mut wrong = license(None);
        wrong[field] = OTHER.into();
        let (client, requests, task) = queued(vec![Reply::json(200, &wrong)]);
        assert_eq!(
            client
                .activate(&key(), "Mac", || panic!("foreign key"))
                .await,
            Err(LicenseError::WrongProduct)
        );
        task.finish();
        assert_eq!(requests.lock().await.len(), 1);
    }
    let (client, requests, task) = queued(vec![Reply::json(200, &license(None))]);
    assert_eq!(
        client
            .activate(&key(), "Mac", || Err(LicenseError::StorageUnavailable))
            .await,
        Err(LicenseError::StorageUnavailable)
    );
    task.finish();
    assert_eq!(requests.lock().await.len(), 1);
}

#[tokio::test]
async fn validation_requires_granted_status_exact_activation_and_product_binding() {
    let (client, _, task) = queued(vec![Reply::json(200, &license(Some(SEAT)))]);
    assert_eq!(
        client.validate(&key(), SEAT).await.unwrap().instance_id(),
        SEAT
    );
    task.finish();
    for field in [
        "id",
        "organization_id",
        "benefit_id",
        "status",
        "expires_at",
        "limit_activations",
    ] {
        let mut malformed = license(Some(SEAT));
        malformed.as_object_mut().unwrap().remove(field);
        let (client, _, task) = queued(vec![Reply::json(200, &malformed)]);
        assert!(client.validate(&key(), SEAT).await.is_err(), "{field}");
        task.finish();
    }
    for (field, value, expected) in [
        (
            "status",
            serde_json::json!("revoked"),
            LicenseError::Revoked,
        ),
        (
            "status",
            serde_json::json!("disabled"),
            LicenseError::Revoked,
        ),
        (
            "status",
            serde_json::json!("unknown"),
            LicenseError::InvalidResponse,
        ),
        (
            "expires_at",
            serde_json::json!("2000-01-01T00:00:00Z"),
            LicenseError::Revoked,
        ),
        (
            "expires_at",
            serde_json::json!("broken"),
            LicenseError::InvalidResponse,
        ),
        (
            "limit_activations",
            serde_json::json!(-1),
            LicenseError::InvalidResponse,
        ),
        (
            "activation",
            serde_json::json!({"id":OTHER,"license_key_id":LICENSE}),
            LicenseError::InvalidResponse,
        ),
        (
            "activation",
            serde_json::json!({"id":SEAT,"license_key_id":OTHER}),
            LicenseError::InvalidResponse,
        ),
        (
            "activation",
            serde_json::Value::Null,
            LicenseError::InvalidResponse,
        ),
    ] {
        let mut body = license(Some(SEAT));
        body[field] = value;
        let (client, _, task) = queued(vec![Reply::json(200, &body)]);
        assert_eq!(
            client.validate(&key(), SEAT).await,
            Err(expected),
            "{field}"
        );
        task.finish();
    }
}

#[tokio::test]
async fn not_found_disabled_revoked_and_expired_are_conclusive_but_not_network_errors() {
    for detail in [
        "Not found",
        "License key is no longer active.",
        "License key has expired.",
    ] {
        let (client, _, task) = queued(vec![
            Reply::json(404, &not_found(detail)),
            Reply::json(404, &not_found(detail)),
        ]);
        assert_eq!(
            client.validate(&key(), SEAT).await,
            Err(LicenseError::Revoked)
        );
        task.finish();
    }
    let (client, _, task) = queued(vec![
        Reply::json(404, &not_found("Not found")),
        Reply::json(200, &license(None)),
    ]);
    assert_eq!(
        client.validate(&key(), SEAT).await,
        Err(LicenseError::ActivationDeactivated)
    );
    task.finish();
    for (status, body, expected) in [
        (400, "invalid", LicenseError::RequestFailed),
        (403, "invalid", LicenseError::RequestFailed),
        (404, "not JSON", LicenseError::RequestFailed),
        (422, "{}", LicenseError::RequestFailed),
        (429, "revoked", LicenseError::RateLimited),
        (503, "revoked", LicenseError::Unavailable),
        (302, "redirect", LicenseError::Unavailable),
        (0, "", LicenseError::Unavailable),
        (200, "{}", LicenseError::InvalidResponse),
        (200, "invalid", LicenseError::InvalidResponse),
    ] {
        let (client, requests, task) = queued(vec![Reply {
            status,
            body: body.into(),
            version: API_VERSION,
        }]);
        let result = client.validate(&key(), SEAT).await;
        assert_eq!(result, Err(expected));
        assert!(!format!("{result:?}").contains(KEY));
        task.finish();
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn deactivation_accepts_only_204_or_confirmed_absence_and_keeps_uncertain_failures() {
    for reply in [Reply::empty(204), Reply::json(404, &not_found("Not found"))] {
        let (client, requests, task) = queued(vec![Reply::json(200, &license(Some(SEAT))), reply]);
        assert_eq!(client.deactivate(&key(), SEAT).await, Ok(()));
        task.finish();
        let requests = requests.lock().await;
        assert_eq!(
            requests[1].path,
            "/v1/customer-portal/license-keys/deactivate"
        );
        assert_eq!(
            requests[1].fields,
            serde_json::json!({"key":KEY,"organization_id":ORG,"activation_id":SEAT})
        );
    }
    let (client, _, task) = queued(vec![
        Reply::json(404, &not_found("Not found")),
        Reply::json(200, &license(None)),
    ]);
    assert_eq!(client.deactivate(&key(), SEAT).await, Ok(()));
    task.finish();
    for (reply, expected) in [
        (Reply::empty(0), LicenseError::Unavailable),
        (Reply::empty(429), LicenseError::RateLimited),
        (
            Reply::json(200, &serde_json::json!({})),
            LicenseError::InvalidResponse,
        ),
        (Reply::empty(503), LicenseError::Unavailable),
    ] {
        let (client, _, task) = queued(vec![Reply::json(200, &license(Some(SEAT))), reply]);
        assert_eq!(client.deactivate(&key(), SEAT).await, Err(expected));
        task.finish();
    }
}

#[tokio::test]
async fn uncertain_mutations_never_retry_and_cannot_return_unbound_activations() {
    let mut wrong = activated();
    wrong["license_key_id"] = OTHER.into();
    for (reply, expected) in [
        (Reply::empty(0), LicenseError::ActivationUncertain),
        (Reply::empty(503), LicenseError::ActivationUncertain),
        (Reply::empty(422), LicenseError::ActivationUncertain),
        (Reply::empty(200), LicenseError::ActivationUncertain),
        (Reply::json(200, &wrong), LicenseError::ActivationUncertain),
        (Reply::empty(429), LicenseError::RateLimited),
        (
            Reply::json(
                403,
                &serde_json::json!({"error":"NotPermitted","detail":"License key activation limit already reached"}),
            ),
            LicenseError::Rejected,
        ),
    ] {
        let (client, requests, task) = queued(vec![Reply::json(200, &license(None)), reply]);
        assert_eq!(
            client.activate(&key(), "Mac", || Ok(())).await,
            Err(expected)
        );
        task.finish();
        assert_eq!(requests.lock().await.len(), 2);
    }
}

#[tokio::test]
async fn oversized_replies_and_unsupported_api_versions_never_revoke_or_grant() {
    for reply in [
        Reply {
            status: 200,
            body: "x".repeat(65_537),
            version: API_VERSION,
        },
        Reply {
            status: 404,
            body: not_found("Not found").to_string(),
            version: "2027-01",
        },
    ] {
        let (client, _, task) = server(vec![reply]).await;
        assert!(matches!(
            client.validate(&key(), SEAT).await,
            Err(LicenseError::InvalidResponse | LicenseError::RequestFailed)
        ));
        task.await.unwrap();
    }
}

struct FixedClock(time::OffsetDateTime);

impl UtcClock for FixedClock {
    fn now(&self) -> time::OffsetDateTime {
        self.0
    }
}

#[tokio::test]
async fn a_licence_expires_exactly_at_its_expiry_time_on_the_given_clock() {
    let at = |value| {
        Arc::new(FixedClock(
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .unwrap(),
        ))
    };
    for (now, expected) in [
        ("2030-05-31T23:59:59Z", Ok(SEAT.to_owned())),
        ("2030-06-01T00:00:00Z", Err(LicenseError::Revoked)),
        ("2030-06-01T00:00:01Z", Err(LicenseError::Revoked)),
    ] {
        let mut body = license(Some(SEAT));
        body["expires_at"] = "2030-06-01T00:00:00Z".into();
        let (client, _, task) = queued(vec![Reply::json(200, &body)]);
        let client = client.with_clock(at(now));
        assert_eq!(
            client
                .validate(&key(), SEAT)
                .await
                .map(|activation| activation.instance_id().to_owned()),
            expected,
            "{now}"
        );
        task.finish();
    }
}
