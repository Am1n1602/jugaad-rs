//! Fetches a stock's daily price history over a date range spanning more
//! than one month, to show the chunking/concurrency path in action.
//!
//!     cargo run -p jugaad-core --example stock_history

use chrono::NaiveDate;
use jugaad_core::nse::NseHistory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseHistory::new()?;

    let from = NaiveDate::from_ymd_opt(2024, 6, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 8, 31).ok_or("invalid date")?;
    let rows = history.stock_history_raw("SBIN", from, to, "EQ").await?;

    println!(
        "Fetched {} rows for SBIN between {from} and {to}",
        rows.len()
    );

    // Rows aren't globally sorted: each month's chunk comes back newest-first
    // from NSE, but chunks are concatenated in chronological chunk order, so
    // finding the actual most recent day means checking every row, not just
    // taking the first one.
    if let Some(latest) = rows.iter().max_by_key(|row| row.date) {
        println!(
            "Most recent: {} close={} volume={}",
            latest.date, latest.close, latest.volume
        );
    }

    Ok(())
}
