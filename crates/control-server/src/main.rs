use std::{future::Future, io, net::SocketAddr};

use mixrelay_control_server::{ControlOperations, bounded_listener};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // LAN access is opt-in. The initial server cannot leave loopback.
    let address = SocketAddr::from(([127, 0, 0, 1], 19_341));
    let listener = tokio::net::TcpListener::bind(address).await?;

    tracing::info!(%address, "MixRelay control server listening");
    let operations = ControlOperations::new();
    let result = axum::serve(
        bounded_listener(listener),
        mixrelay_control_server::router(operations.clone()),
    )
    .with_graceful_shutdown(stop_admission_on_signal(
        operations.clone(),
        tokio::signal::ctrl_c(),
    ))
    .await;
    operations.close_admission();
    operations.drain().await;
    result?;
    Ok(())
}

async fn stop_admission_on_signal(
    operations: ControlOperations,
    signal: impl Future<Output = io::Result<()>>,
) {
    if let Err(error) = signal.await {
        tracing::error!(%error, "control server shutdown signal failed");
    }
    operations.close_admission();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn successful_and_failed_signals_close_admission_before_returning() {
        for result in [Ok(()), Err(io::Error::other("signal observation failed"))] {
            let operations = ControlOperations::new();
            let router = mixrelay_control_server::router(operations.clone());
            let graph_request = || {
                Request::get(mixrelay_control_server::GRAPH_ROUTE)
                    .body(Body::empty())
                    .unwrap()
            };
            let before = router.clone().oneshot(graph_request()).await.unwrap();
            assert_eq!(before.status(), axum::http::StatusCode::OK);
            stop_admission_on_signal(operations, std::future::ready(result)).await;
            let after = router.oneshot(graph_request()).await.unwrap();
            assert_eq!(after.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
    }
}
