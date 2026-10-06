use std::{collections::VecDeque, sync::Arc};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Mutex,
};

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

async fn mock(
    replies: Vec<(u16, String)>,
) -> (LicenseClient, Requests, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("mock bind");
    let address = listener.local_addr().expect("mock address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let task = tokio::spawn(async move {
        for (status, body) in VecDeque::from(replies) {
            let (mut socket, _) = listener.accept().await.expect("mock accept");
            let request = read_request(&mut socket).await;
            captured.lock().await.push(request);
            let response = format!(
                "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("mock reply");
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .expect("mock client");
    (
        LicenseClient {
            client,
            product: product(),
            origin: format!("http://{address}"),
        },
        requests,
        task,
    )
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0_u8; 1024];
        let count = socket.read(&mut chunk).await.expect("mock read");
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
        let request = String::from_utf8_lossy(&bytes);
        if let Some((headers, body)) = request.split_once("\r\n\r\n") {
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.parse().ok())
                })
                .expect("request content length");
            if body.len() >= length {
                return request.into_owned();
            }
        }
        assert!(bytes.len() < 16_384, "bounded fixture request");
    }
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
async fn activation_prevalidates_and_posts_form_without_merchant_credentials() {
    let (client, requests, task) = mock(vec![
        (200, response("valid", "inactive", None)),
        (200, response("activated", "active", Some("seat-1"))),
    ])
    .await;
    let result = client
        .activate(&key(), "MixRelay Mac")
        .await
        .expect("activate");
    assert_eq!(result.license_id(), 7);
    assert_eq!(result.instance_id(), "seat-1");
    task.await.expect("mock completion");
    let requests = requests.lock().await;
    assert!(requests[0].starts_with("POST /validate "));
    assert!(requests[1].starts_with("POST /activate "));
    for request in requests.iter() {
        let lowered = request.to_ascii_lowercase();
        assert!(lowered.contains("accept: application/json"));
        assert!(lowered.contains("content-type: application/x-www-form-urlencoded"));
        assert!(!lowered.contains("authorization:"));
    }
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
        let (client, requests, task) = mock(vec![(200, wrong.to_string())]).await;
        assert_eq!(
            client.activate(&key(), "Mac").await,
            Err(LicenseError::WrongProduct)
        );
        task.await.expect("mock completion");
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn validation_requires_exact_instance_and_active_license() {
    for (status, instance, error) in [
        ("expired", Some("seat-1"), LicenseError::Rejected),
        ("disabled", Some("seat-1"), LicenseError::Rejected),
        ("inactive", Some("seat-1"), LicenseError::Rejected),
        (
            "active",
            Some("another-seat"),
            LicenseError::InvalidResponse,
        ),
        ("active", None, LicenseError::InvalidResponse),
    ] {
        let (client, _, task) = mock(vec![(200, response("valid", status, instance))]).await;
        assert_eq!(client.validate(&key(), "seat-1").await, Err(error));
        task.await.expect("mock completion");
    }
}

#[tokio::test]
async fn deactivation_validates_and_only_releases_the_requested_seat() {
    let (client, requests, task) = mock(vec![
        (200, response("valid", "active", Some("seat-1"))),
        (200, response("deactivated", "inactive", None)),
    ])
    .await;
    client
        .deactivate(&key(), "seat-1")
        .await
        .expect("deactivate");
    task.await.expect("mock completion");
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
        let (client, requests, task) = mock(vec![(status, body)]).await;
        let result = client.validate(&key(), "seat-1").await;
        assert_eq!(result, Err(error));
        assert!(!format!("{result:?}").contains(KEY));
        task.await.expect("mock completion");
        assert_eq!(requests.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn valid_instance_and_missing_or_changed_binding_are_checked() {
    let (client, _, task) = mock(vec![(200, response("valid", "active", Some("seat-1")))]).await;
    assert_eq!(
        client
            .validate(&key(), "seat-1")
            .await
            .expect("validate")
            .instance_id(),
        "seat-1"
    );
    task.await.expect("mock completion");
    let mut missing: serde_json::Value =
        serde_json::from_str(&response("valid", "active", Some("seat-1"))).expect("fixture");
    for field in ["meta", "license_key"] {
        let before = missing[field].take();
        let (client, _, task) = mock(vec![(200, missing.to_string())]).await;
        assert_eq!(
            client.validate(&key(), "seat-1").await,
            Err(LicenseError::InvalidResponse)
        );
        task.await.expect("mock completion");
        missing[field] = before;
    }
    for instance in ["", "\n", &"x".repeat(256)] {
        let (client, _, task) =
            mock(vec![(200, response("valid", "active", Some(instance)))]).await;
        assert_eq!(
            client.validate(&key(), "seat-1").await,
            Err(LicenseError::InvalidResponse)
        );
        task.await.expect("mock completion");
    }
}
