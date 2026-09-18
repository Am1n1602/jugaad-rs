//! Exercises the live-market endpoints: market open/closed status, a
//! snapshot of every index, market-wide turnover, and a live NIFTY
//! futures/options snapshot.
//!
//!     cargo run -p jugaad-core --example live_market

use jugaad_core::nse::NseLiveMarket;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let live = NseLiveMarket::new()?;

    let segments = live.market_status_raw().await?;
    println!("Market status:");
    for segment in &segments {
        println!(
            "  {}: {} ({})",
            segment.market, segment.status, segment.status_message
        );
    }

    let index_rows = live.index_snapshot_raw().await?;
    println!("\nIndex snapshot rows: {}", index_rows.len());
    if let Some(row) = index_rows.iter().find(|r| r.name == "NIFTY 50") {
        println!(
            "  {} last={} change={} pe={:?}",
            row.name, row.last, row.change, row.pe
        );
    }

    let turnover_rows = live.market_turnover_raw().await?;
    println!("\nMarket turnover:");
    for row in &turnover_rows {
        println!(
            "  {}: volume={} value={}",
            row.name.as_deref().unwrap_or("(unnamed)"),
            row.volume,
            row.value
        );
    }

    let fo_rows = live.live_fo_snapshot_raw().await?;
    println!("\nLive NIFTY F&O rows: {}", fo_rows.len());
    for row in &fo_rows {
        println!(
            "  {} last_price={} open_interest={}",
            row.contract, row.last_price, row.open_interest
        );
    }

    Ok(())
}
