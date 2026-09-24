//! Manual smoke test for `jugaad-rpc`. Start the server first
//! (`cargo run -p jugaad-rpc`), then run this against it:
//!
//!     cargo run -p jugaad-rpc --example client -- SBIN

use jugaad_rpc::jugaad::jugaad_client::JugaadClient;
use jugaad_rpc::jugaad::{StockQuoteRequest, WatchStockQuoteRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let symbol = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "SBIN".to_string());
    let mut client = JugaadClient::connect("http://[::1]:50051").await?;

    let quote = client
        .get_stock_quote(StockQuoteRequest {
            symbol: symbol.clone(),
        })
        .await?
        .into_inner();
    println!("GetStockQuote: {quote:#?}");

    println!("\nWatchStockQuote (3 updates, 2s apart):");
    let mut stream = client
        .watch_stock_quote(WatchStockQuoteRequest {
            symbol,
            interval_seconds: 2,
        })
        .await?
        .into_inner();

    for _ in 0..3 {
        match stream.message().await? {
            Some(quote) => println!(
                "  {} last={} change={} at {}",
                quote.symbol, quote.last_price, quote.change, quote.last_update_time
            ),
            None => break,
        }
    }

    Ok(())
}
