use std::time::Duration;

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::Request,
    http::{StatusCode, header::CONNECTION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use mixrelay_control_protocol::{ErrorCode, ProtocolError};

const BODY_LIMIT: usize = 16 * 1024 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(2);

/// Bound ingress before handing ownership to a mutation. Once dispatched, a native
/// transaction or recorder finalization must complete even while the server shuts down.
pub(super) async fn read_body(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(BODY_DEADLINE, to_bytes(body, BODY_LIMIT)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => return rejected(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(_) => {
            return rejected(
                StatusCode::REQUEST_TIMEOUT,
                "request body deadline exceeded",
            );
        }
    };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

fn rejected(status: StatusCode, reason: &str) -> Response {
    (
        status,
        [(CONNECTION, "close")],
        Json(ProtocolError {
            code: ErrorCode::GraphInvalid,
            parameters: serde_json::json!({"reason": reason}),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Router, middleware, routing::post};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::{Notify, oneshot},
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn incomplete_body_cannot_hold_graceful_shutdown_open() {
        let entered = Arc::new(Notify::new());
        let observed = entered.clone();
        let router = crate::router().layer(middleware::from_fn(move |request, next: Next| {
            let observed = observed.clone();
            async move {
                observed.notify_one();
                next.run(request).await
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(crate::bounded_listener(listener), router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("server");
        });
        let mut client = TcpStream::connect(address).await.expect("client");
        client.write_all(b"PUT /api/v1/audio/power HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{")
            .await.expect("partial request");
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .expect("request entered router");
        shutdown.send(()).expect("signal shutdown");
        tokio::time::timeout(BODY_DEADLINE + Duration::from_secs(2), server)
            .await
            .expect("shutdown must drain incomplete ingress")
            .expect("server task");
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
            .await
            .expect("connection closed")
            .expect("response");
        assert!(
            String::from_utf8(response)
                .expect("HTTP response")
                .starts_with("HTTP/1.1 408")
        );
    }

    #[tokio::test]
    async fn ingress_deadline_does_not_cancel_an_accepted_mutation() {
        let started = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let handler_started = started.clone();
        let handler_finish = finish.clone();
        let router = Router::new()
            .route(
                "/mutation",
                post(move || async move {
                    handler_started.notify_one();
                    handler_finish.notified().await;
                    StatusCode::NO_CONTENT
                }),
            )
            .layer(middleware::from_fn(read_body));
        let task = tokio::spawn(
            router.oneshot(
                Request::post("/mutation")
                    .body(Body::from("{}"))
                    .expect("request"),
            ),
        );
        started.notified().await;
        tokio::time::sleep(BODY_DEADLINE + Duration::from_millis(20)).await;
        assert!(!task.is_finished(), "accepted work must retain its owner");
        finish.notify_one();
        assert_eq!(
            task.await.expect("task").expect("response").status(),
            StatusCode::NO_CONTENT
        );
    }

    #[tokio::test]
    async fn non_reading_client_cannot_hold_graceful_shutdown_open() {
        let router = Router::new()
            .route("/large", post(|| async { vec![b'x'; 32 * 1024 * 1024] }))
            .layer(middleware::from_fn(read_body));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let (shutdown, stopped) = oneshot::channel();
        let mut server = tokio::spawn(async move {
            axum::serve(crate::bounded_listener(listener), router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("server");
        });
        let mut client = TcpStream::connect(address).await.expect("client");
        client
            .write_all(b"POST /large HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("request");
        let mut first_byte = [0];
        tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut first_byte))
            .await
            .expect("response began")
            .expect("first byte");
        shutdown.send(()).expect("signal shutdown");
        let drained = tokio::time::timeout(Duration::from_secs(3), &mut server).await;
        let stalled = drained.is_err();
        // Always release the fixture peer before reporting a failed drain assertion.
        drop(client);
        match drained {
            Ok(result) => result.expect("server task"),
            Err(_) => tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .expect("drain after peer disconnect")
                .expect("server task"),
        }
        assert!(
            !stalled,
            "non-reading peer held graceful shutdown beyond 3 seconds"
        );
    }

    #[tokio::test]
    async fn socket_deadline_preserves_dispatched_mutation_during_shutdown() {
        let started = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let handler_started = started.clone();
        let handler_finish = finish.clone();
        let router = Router::new()
            .route(
                "/mutation",
                post(move || async move {
                    handler_started.notify_one();
                    handler_finish.notified().await;
                    StatusCode::NO_CONTENT
                }),
            )
            .layer(middleware::from_fn(read_body));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(crate::bounded_listener(listener), router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("server");
        });
        let mut client = TcpStream::connect(address).await.expect("client");
        client
            .write_all(b"POST /mutation HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("request");
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("mutation dispatched");
        shutdown.send(()).expect("signal shutdown");
        tokio::time::sleep(BODY_DEADLINE + Duration::from_millis(20)).await;
        assert!(
            !server.is_finished(),
            "shutdown must wait for accepted mutation"
        );
        finish.notify_one();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("mutation finalized")
            .expect("server task");
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
            .await
            .expect("connection closed")
            .expect("response");
        assert!(
            String::from_utf8(response)
                .expect("HTTP response")
                .starts_with("HTTP/1.1 204")
        );
    }
}
