use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // LAN access is opt-in. The initial server cannot leave loopback.
    let address = SocketAddr::from(([127, 0, 0, 1], 19_341));
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("MixRelay control server listening on http://{address}");
    axum::serve(listener, mixrelay_control_server::router()).await?;
    Ok(())
}
