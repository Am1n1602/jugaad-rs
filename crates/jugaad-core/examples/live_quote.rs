//! Exercises the per-symbol live-quote endpoints: a stock's live quote
//! (with order book), a symbol's F&O contracts, a single index's live
//! value, and option chains (index/equity/currency).
//!
//!     cargo run -p jugaad-core --example live_quote

use jugaad_core::nse::{ChartPeriod, NseQuote, OptionChainKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let quote = NseQuote::new()?;

    let stock = quote.stock_quote_raw("SBIN").await?;
    println!(
        "SBIN: last={} change={} volume={}",
        stock.last_price, stock.change, stock.total_traded_volume
    );
    println!(
        "  best bid: {} x {}  best ask: {} x {}",
        stock.order_book.levels[0].buy_price,
        stock.order_book.levels[0].buy_quantity,
        stock.order_book.levels[0].sell_price,
        stock.order_book.levels[0].sell_quantity
    );

    let chart = quote
        .stock_chart_data_raw("SBIN", ChartPeriod::OneDay)
        .await?;
    println!(
        "\nSBIN intraday chart: {} points, close={}",
        chart.points.len(),
        chart.close_price
    );
    if let Some(point) = chart.points.last() {
        println!(
            "  latest: {} price={} session={}",
            point.timestamp, point.price, point.session
        );
    }

    let fo_rows = quote.derivative_quote_raw("SBIN").await?;
    println!("\nSBIN F&O contracts: {}", fo_rows.len());
    if let Some(row) = fo_rows.first() {
        println!(
            "  {} {} strike={} last={}",
            row.instrument_type, row.expiry, row.strike_price, row.last_price
        );
    }

    let index = quote.index_quote_raw("NIFTY 50").await?;
    println!(
        "\nNIFTY 50: last={} change={} volume={}",
        index.last, index.change, index.total_traded_volume
    );

    let chain = quote
        .option_chain_raw("NIFTY", OptionChainKind::Index, None)
        .await?;
    println!("\nNIFTY option chain rows: {}", chain.len());
    if let Some(row) = chain.iter().find(|r| r.call.is_some() && r.put.is_some()) {
        println!(
            "  strike={} expiry={} call_oi={} put_oi={}",
            row.strike_price,
            row.expiry,
            row.call.as_ref().map(|c| c.open_interest).unwrap_or(0),
            row.put.as_ref().map(|p| p.open_interest).unwrap_or(0)
        );
    }

    let currency_chain = quote.currency_option_chain_raw("USDINR").await?;
    println!("\nUSDINR option chain rows: {}", currency_chain.len());

    Ok(())
}
