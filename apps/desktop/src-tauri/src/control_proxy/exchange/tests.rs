use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    task::JoinHandle,
};

use super::*;

/// A one-shot engine on a socket in its own fixture directory, and a client of it.
fn server(
    replies: Vec<(u16, Option<&'static str>, &'static str)>,
) -> (String, ControlProxyClient, JoinHandle<Vec<String>>) {
    static FIXTURES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let fixture = mixrelay_test_support::FixtureDir::new(&format!(
        "exchange-{}",
        FIXTURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
    .expect("fixture");
    let path = fixture.join("engine.sock");
    let listener = UnixListener::bind(&path).expect("bind fixture");
    let client = ControlProxyClient::with_socket(Some(path));
    let origin = super::super::CONTROL_ORIGIN.to_owned();
    let task = tokio::spawn(async move {
        let _fixture = fixture;
        let mut requests = Vec::new();
        let mut accepted = None;
        for (status, receipt, body) in replies {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.expect("request header");
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
                assert!(request.len() < 8192, "bounded fixture request");
            }
            requests.push(String::from_utf8(request).expect("HTTP header"));
            let header =
                receipt.map_or_else(String::new, |id| format!("X-MixRelay-Operation: {id}\r\n"));
            let result = if receipt.is_none() && status != 404 {
                accepted.map_or_else(String::new, |id| {
                    format!("X-MixRelay-Operation-Result: {id}\r\n")
                })
            } else {
                String::new()
            };
            if receipt.is_some() {
                accepted = receipt;
            }
            let response = format!(
                "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{header}{result}\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.expect("reply");
        }
        requests
    });
    (origin, client, task)
}

#[tokio::test]
async fn polls_once_submitted_work_and_preserves_terminal_accepted_status() {
    let (origin, client, server) = server(vec![
        (202, Some("session-1"), r#"{"operationId":"session-1"}"#),
        (202, Some("session-1"), r#"{"operationId":"session-1"}"#),
        (202, None, r#"{"revision":12,"state":"pending"}"#),
        (204, None, ""),
    ]);
    let result = request(&client, &origin, Method::PUT, "/graph", None)
        .await
        .expect("terminal response");
    assert_eq!(result.status, 202);
    assert_eq!(result.body["revision"], 12);
    let requests = server.await.expect("fixture finished");
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("PUT /graph "));
    assert!(requests[1].starts_with("GET /api/v1/operations/session-1 "));
    assert!(requests[2].starts_with("GET /api/v1/operations/session-1 "));
    assert!(requests[3].starts_with("DELETE /api/v1/operations/session-1 "));
}

#[tokio::test]
async fn quick_accepted_status_is_not_an_operation_receipt() {
    let (origin, client, server) = server(vec![(202, None, r#"{"revision":4}"#)]);
    let result = request(&client, &origin, Method::PUT, "/graph", None)
        .await
        .expect("response");
    assert_eq!(result.body["revision"], 4);
    assert_eq!(server.await.expect("fixture").len(), 1);
}

#[tokio::test]
async fn failed_acknowledgement_preserves_terminal_error() {
    let (origin, client, server) = server(vec![
        (202, Some("session-2"), r#"{"operationId":"session-2"}"#),
        (409, None, r#"{"code":"graph_exchange_busy"}"#),
    ]);
    let result = request(&client, &origin, Method::PUT, "/graph", None)
        .await
        .expect("terminal response survives lost acknowledgement");
    assert_eq!(result.status, 409);
    assert_eq!(result.body["code"], "graph_exchange_busy");
    assert_eq!(server.await.expect("fixture").len(), 2);
}

#[tokio::test]
async fn lost_poll_is_uncertain_even_when_connect_fails() {
    let (origin, client, server) = server(vec![(
        202,
        Some("session-3"),
        r#"{"operationId":"session-3"}"#,
    )]);
    let result = request(&client, &origin, Method::PUT, "/graph", None)
        .await
        .expect("structured failure");
    assert_eq!(result.status, 503);
    #[cfg(target_os = "macos")]
    assert_eq!(result.delivery, RequestDelivery::Uncertain);
    assert!(
        result.body["parameters"]
            .get("nodeControlOutcome")
            .is_none()
    );
    assert_eq!(server.await.expect("fixture").len(), 1);
}

#[tokio::test]
async fn initial_connection_refusal_is_distinct_from_a_lost_poll() {
    let (origin, client, server) = server(Vec::new());
    server.await.expect("listener closed");
    let result = request(&client, &origin, Method::PUT, "/graph", None)
        .await
        .expect("structured failure");
    assert_eq!(result.status, 503);
    #[cfg(target_os = "macos")]
    assert_eq!(result.delivery, RequestDelivery::NotSent);
}

#[tokio::test]
async fn changed_receipt_is_rejected_without_resubmitting() {
    let (origin, client, server) = server(vec![
        (202, Some("session-4"), r#"{"operationId":"session-4"}"#),
        (202, Some("different"), r#"{"operationId":"different"}"#),
    ]);
    assert!(
        request(&client, &origin, Method::PUT, "/graph", None)
            .await
            .is_err()
    );
    assert_eq!(server.await.expect("fixture").len(), 2);
}

#[tokio::test]
async fn unknown_receipt_is_not_a_definitive_mutation_rejection() {
    let (origin, client, server) = server(vec![
        (202, Some("session-5"), r#"{"operationId":"session-5"}"#),
        (404, None, r#"{"code":"control_unavailable"}"#),
    ]);
    let result = request(&client, &origin, Method::PUT, "/graph", None).await;
    assert!(
        result
            .expect_err("unconfirmed outcome")
            .contains("could not be confirmed")
    );
    assert_eq!(server.await.expect("fixture").len(), 2);
}

#[test]
fn receipt_body_must_match_and_id_is_one_path_segment() {
    assert!(validate_receipt("one", &serde_json::json!({"operationId":"two"}), None).is_err());
    assert_eq!(
        operation_url("http://127.0.0.1", "a/b?c")
            .expect("url")
            .path(),
        "/api/v1/operations/a%2Fb%3Fc"
    );
}
