//! Fetches a stock's daily price history and prints date, close and volume
//! for every trading day in the range.
//!
//!     cargo run -p jugaad-core --example stock_history_table

use chrono::NaiveDate;
use jugaad_core::nse::NseHistory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseHistory::new()?;

    let from = NaiveDate::from_ymd_opt(2024, 6, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 8, 31).ok_or("invalid date")?;
    let mut rows = history.stock_history_raw("SBIN", from, to, "EQ").await?;

    // Rows come back grouped by month chunk (each chunk newest-first
    // internally), not globally sorted - sort by date so the table below
    // reads chronologically from oldest to newest.
    rows.sort_by_key(|row| row.date);

    println!("{:<12} {:>10} {:>12}", "DATE", "CLOSE", "VOLUME");
    for row in &rows {
        println!("{:<12} {:>10.2} {:>12}", row.date, row.close, row.volume);
    }
    println!("{} rows", rows.len());

    Ok(())
}
