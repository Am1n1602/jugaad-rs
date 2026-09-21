//! Exercises the live-market endpoints: market open/closed status, a
//! snapshot of every index, market-wide turnover, a live NIFTY
//! futures/options snapshot, today's block deals, and the F&O turnover
//! leaderboards.
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
            "  {}: {} ({}) last={:?} change={:?}",
            segment.market, segment.status, segment.status_message, segment.last, segment.change
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

    let block_deals = live.block_deal_session_raw().await?;
    println!("\nBlock deals today: {}", block_deals.len());
    for row in &block_deals {
        println!(
            "  [{}] {} last_price={} volume={}",
            row.session, row.symbol, row.last_price, row.total_traded_volume
        );
    }

    let turnover_leaders = live.eq_derivative_turnover_raw().await?;
    println!(
        "\nF&O turnover leaderboard rows: {}",
        turnover_leaders.len()
    );
    for row in turnover_leaders.iter().take(3) {
        println!(
            "  [{}] {} {} last_price={}",
            row.ranking, row.underlying, row.instrument, row.last_price
        );
    }

    Ok(())
}
