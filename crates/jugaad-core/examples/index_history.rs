//! Fetches historical OHLC data for an NSE index over a date range spanning
//! more than one month, to exercise the chunking/concurrency path.
//!
//!     cargo run -p jugaad-core --example index_history

use chrono::NaiveDate;
use jugaad_core::nse::NseIndexHistory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseIndexHistory::new()?;

    let from = NaiveDate::from_ymd_opt(2024, 6, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 8, 31).ok_or("invalid date")?;
    let mut rows = history.index_history_raw("NIFTY 50", from, to).await?;

    rows.sort_by_key(|row| row.date);

    println!(
        "Fetched {} rows for NIFTY 50 between {from} and {to}",
        rows.len()
    );
    if let Some(latest) = rows.last() {
        println!("Most recent: {} close={}", latest.date, latest.close);
    }

    Ok(())
}
