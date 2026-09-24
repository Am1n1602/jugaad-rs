use jugaad_rpc::{JugaadServer, JugaadService};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = "[::1]:50051".parse()?;
    let service = JugaadService::new()?;
    println!("jugaad-rpc listening on {addr}");

    Server::builder()
        .add_service(JugaadServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
