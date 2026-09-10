use super::*;

#[tokio::test]
async fn occupied_control_address_rejects_admission_and_leaves_listener_alive() {
    let listener = bind_control(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();

    let error = bind_control(address).await.unwrap_err();

    assert!(error.contains("could not bind local control server"));
    assert!(error.contains(&address.to_string()));
    let connection = tokio::net::TcpStream::connect(address).await.unwrap();
    let (_, peer) = listener.accept().await.unwrap();
    assert_eq!(peer, connection.local_addr().unwrap());
}

#[tokio::test]
async fn dropping_admission_after_startup_failure_releases_the_address() {
    let listener = bind_control(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let failed = async move {
        let _listener = listener;
        Err::<(), _>("native preparation failed")
    }
    .await;

    assert!(failed.is_err());
    let replacement = bind_control(address).await.unwrap();
    assert_eq!(replacement.local_addr().unwrap(), address);
}
