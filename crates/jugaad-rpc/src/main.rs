use jugaad_rpc::{JugaadServer, JugaadService};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Binds all interfaces, not just loopback - required for this to be
    // reachable from outside a Docker container even with a published
    // port. Override with JUGAAD_RPC_ADDR for a different bind address.
    let addr = std::env::var("JUGAAD_RPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let addr = addr.parse()?;

    // Opt-in: a parent process (the Python package) holds our stdin pipe open
    // and exits us when it closes, so a hard-killed parent can't orphan the
    // server. Must stay opt-in - Docker's stdin is /dev/null (instant EOF).
    if std::env::var("JUGAAD_RPC_EXIT_ON_STDIN_CLOSE").is_ok_and(|v| v == "1") {
        std::thread::spawn(|| {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            std::process::exit(0);
        });
    }

    let service = JugaadService::new()?;
    println!("jugaad-rpc listening on {addr}");

    Server::builder()
        .add_service(JugaadServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
