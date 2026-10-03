use jugaad_rpc::{JugaadServer, JugaadService};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Binds all interfaces, not just loopback - required for this to be
    // reachable from outside a Docker container even with a published
    // port. Override with JUGAAD_RPC_ADDR for a different bind address.
    let addr = std::env::var("JUGAAD_RPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let addr = addr.parse()?;
    let service = JugaadService::new()?;
    println!("jugaad-rpc listening on {addr}");

    Server::builder()
        .add_service(JugaadServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
