use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Mutex;

use super::*;
use crate::client;

const KEY: &str = "synthetic-test-key";

fn product() -> ProductIdentity {
    ProductIdentity::new(1, 2, 3).expect("test product")
}

fn response(flag: &str, status: &str, instance: Option<&str>) -> String {
    serde_json::json!({
        flag: true,
        "license_key": {"id": 7, "status": status, "key": KEY},
        "instance": instance.map(|id| serde_json::json!({"id": id})),
        "meta": {"store_id": 1, "product_id": 2, "variant_id": 3,
                 "customer_email": "do-not-return@example.invalid"}
    })
    .to_string()
}

type Requests = Arc<Mutex<Vec<String>>>;

struct MemoryHttp {
    replies: Mutex<VecDeque<(u16, String)>>,
    requests: Requests,
}

impl LicenseHttp for MemoryHttp {
    fn post<'a>(
        &'a self,
        operation: &'a str,
        form: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>> {
        Box::pin(async move {
            let form = form
                .into_iter()
                .map(|(key, value)| format!("{key}={}", value.replace(' ', "+")))
                .collect::<Vec<_>>()
                .join("&");
            self.requests
                .lock()
                .await
                .push(format!("POST /{operation} {form}"));
            let (status, body) = self
                .replies
                .lock()
                .await
                .pop_front()
                .expect("expected HTTP request");
            Ok(LicenseHttpResponse {
                status,
                body: body.into_bytes(),
            })
        })
    }
}

fn mock(replies: Vec<(u16, String)>) -> (LicenseClient, Requests) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let http = MemoryHttp {
        replies: Mutex::new(replies.into()),
        requests: requests.clone(),
    };
    (
        LicenseClient::with_http(product(), Arc::new(http)),
        requests,
    )
}

fn key() -> SecretString {
    SecretString::from(KEY)
}

#[test]
fn product_identifiers_are_required() {
    for ids in [(0, 2, 3), (1, 0, 3), (1, 2, 0)] {
        assert_eq!(
            ProductIdentity::new(ids.0, ids.1, ids.2),
            Err(LicenseError::InvalidConfiguration)
        );
    }
    assert!(LicenseClient::new(product()).is_ok());
}

#[test]
fn arguments_and_debug_output_do_not_expose_keys() {
    for input in ["", " ", "line\nfeed", "null\0byte", &"x".repeat(256)] {
        assert_eq!(
            client::check_argument(input),
            Err(LicenseError::InvalidInput)
        );
    }
    assert!(client::check_argument("MixRelay Mac").is_ok());
    assert!(!format!("{:?}", key()).contains(KEY));
}

#[tokio::test]
async fn activation_prevalidates_and_passes_only_public_license_arguments() {
    let (client, requests) = mock(vec![
        (200, response("valid", "inactive", None)),
        (200, response("activated", "active", Some("seat-1"))),
    ]);
    let result = client
        .activate(&key(), "MixRelay Mac", || Ok(()))
        .await
        .expect("activate");
    assert_eq!(result.license_id(), 7);
    assert_eq!(result.instance_id(), "seat-1");
    let requests = requests.lock().await;
    assert!(requests[0].starts_with("POST /validate "));
    assert!(requests[1].starts_with("POST /activate "));
    assert!(requests[1].contains("instance_name=MixRelay+Mac"));
    let output = serde_json::to_string(&result).expect("safe result");
    assert!(!output.contains(KEY));
    assert!(!output.contains("customer"));
}

#[tokio::test]
async fn foreign_keys_never_consume_an_activation() {
    for field in ["store_id", "product_id", "variant_id"] {
        let mut wrong: serde_json::Value =
            serde_json::from_str(&response("valid", "active", None)).expect("fixture");
        wrong["meta"][field] = 999.into();
        let (client, requests) = mock(vec![(200, wrong.to_string())]);
        assert_eq!(
            client.activate(&key(), "Mac", || Ok(())).await,
            Err(LicenseError::WrongProduct)
        );
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn validation_requires_exact_instance_and_active_license() {
    for (status, instance, error) in [
        ("expired", Some("seat-1"), LicenseError::Revoked),
        ("disabled", Some("seat-1"), LicenseError::Revoked),
        ("inactive", Some("seat-1"), LicenseError::Rejected),
        (
            "active",
            Some("another-seat"),
            LicenseError::InvalidResponse,
        ),
        ("active", None, LicenseError::InvalidResponse),
    ] {
        let (client, _) = mock(vec![(200, response("valid", status, instance))]);
        assert_eq!(client.validate(&key(), "seat-1").await, Err(error));
    }
}

#[tokio::test]
async fn deactivation_validates_and_only_releases_the_requested_seat() {
    let (client, requests) = mock(vec![
        (200, response("valid", "active", Some("seat-1"))),
        (200, response("deactivated", "inactive", None)),
    ]);
    client
        .deactivate(&key(), "seat-1")
        .await
        .expect("deactivate");
    let requests = requests.lock().await;
    assert!(requests[1].starts_with("POST /deactivate "));
    assert!(
        requests
            .iter()
            .all(|request| request.contains("instance_id=seat-1"))
    );
}

#[tokio::test]
async fn malformed_oversized_rejected_and_unavailable_responses_are_safe_errors() {
    for (status, body, error) in [
        (200, "not JSON".into(), LicenseError::InvalidResponse),
        (200, "{}".into(), LicenseError::InvalidResponse),
        (200, r#"{"valid":false}"#.into(), LicenseError::Rejected),
        (200, "x".repeat(65_537), LicenseError::InvalidResponse),
        (400, format!("rejected {KEY}"), LicenseError::RequestFailed),
        (422, format!("bad field {KEY}"), LicenseError::RequestFailed),
        (429, format!("limited {KEY}"), LicenseError::RateLimited),
        (500, format!("failure {KEY}"), LicenseError::Unavailable),
        (302, String::new(), LicenseError::Unavailable),
    ] {
        let (client, requests) = mock(vec![(status, body)]);
        let result = client.validate(&key(), "seat-1").await;
        assert_eq!(result, Err(error));
        assert!(!format!("{result:?}").contains(KEY));
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn valid_instance_and_missing_or_changed_binding_are_checked() {
    let (client, _) = mock(vec![(200, response("valid", "active", Some("seat-1")))]);
    assert_eq!(
        client
            .validate(&key(), "seat-1")
            .await
            .expect("validate")
            .instance_id(),
        "seat-1"
    );
    let mut missing: serde_json::Value =
        serde_json::from_str(&response("valid", "active", Some("seat-1"))).expect("fixture");
    for field in ["meta", "license_key"] {
        let before = missing[field].take();
        let (client, _) = mock(vec![(200, missing.to_string())]);
        assert_eq!(
            client.validate(&key(), "seat-1").await,
            Err(LicenseError::InvalidResponse)
        );
        missing[field] = before;
    }
    for instance in ["", "\n", &"x".repeat(256)] {
        let (client, _) = mock(vec![(200, response("valid", "active", Some(instance)))]);
        assert_eq!(
            client.validate(&key(), "seat-1").await,
            Err(LicenseError::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn provider_revocation_is_distinct_from_transient_and_unknown_failures() {
    for (status, body, expected) in [
        (
            404,
            serde_json::json!({"valid":false,"error":"license_key not found."}),
            LicenseError::Revoked,
        ),
        (
            400,
            serde_json::json!({"valid":false,"error":"license_key is disabled."}),
            LicenseError::Revoked,
        ),
        (
            400,
            serde_json::json!({"valid":false,"error":"instance not found."}),
            LicenseError::ActivationDeactivated,
        ),
        (
            200,
            serde_json::json!({"valid":false,"error":"unknown provider error"}),
            LicenseError::Rejected,
        ),
        (
            429,
            serde_json::json!({"valid":false,"error":"license_key is disabled."}),
            LicenseError::RateLimited,
        ),
        (
            503,
            serde_json::json!({"valid":false,"error":"license_key is disabled."}),
            LicenseError::Unavailable,
        ),
    ] {
        let (client, requests) = mock(vec![(status, body.to_string())]);
        assert_eq!(client.validate(&key(), "seat-1").await, Err(expected));
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn already_deactivated_seat_is_a_successful_release_even_if_removed_during_release() {
    let absent = r#"{"valid":false,"deactivated":false,"error":"instance not found."}"#.to_owned();
    for replies in [
        vec![(404, absent.clone())],
        vec![
            (200, response("valid", "active", Some("seat-1"))),
            (404, absent),
        ],
    ] {
        let count = replies.len();
        let (client, requests) = mock(replies);
        assert_eq!(client.deactivate(&key(), "seat-1").await, Ok(()));
        let requests = requests.lock().await;
        assert_eq!(requests.len(), count);
        assert!(
            requests
                .iter()
                .all(|request| request.contains("instance_id=seat-1"))
        );
    }
}

#[tokio::test]
async fn activation_requires_a_durable_intent_and_never_replays_an_uncertain_mutation() {
    let (client, requests) = mock(vec![(200, response("valid", "inactive", None))]);
    assert_eq!(
        client
            .activate(&key(), "Mac", || Err(LicenseError::StorageUnavailable))
            .await,
        Err(LicenseError::StorageUnavailable)
    );
    assert_eq!(requests.lock().await.len(), 1);
    for reply in [
        (503, String::new()),
        (200, "invalid JSON".into()),
        (400, "invalid JSON".into()),
        (422, "invalid field".into()),
    ] {
        let (client, requests) = mock(vec![(200, response("valid", "inactive", None)), reply]);
        assert_eq!(
            client.activate(&key(), "Mac", || Ok(())).await,
            Err(LicenseError::ActivationUncertain)
        );
        assert_eq!(requests.lock().await.len(), 2);
    }
}
