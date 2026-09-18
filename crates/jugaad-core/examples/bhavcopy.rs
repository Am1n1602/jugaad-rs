//! Fetches NSE's daily bhavcopy (whole-market OHLC data) for one trading day.
//!
//!     cargo run -p jugaad-core --example bhavcopy

use chrono::NaiveDate;
use jugaad_core::nse::NseArchives;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archives = NseArchives::new()?;

    let date = NaiveDate::from_ymd_opt(2024, 8, 1).ok_or("invalid date")?;
    let csv_text = archives.bhavcopy_raw(date).await?;

    let row_count = csv_text.lines().count().saturating_sub(1); // minus header
    println!("Fetched bhavcopy for {date}: {row_count} rows");
    println!("{}", csv_text.lines().next().unwrap_or_default());

    Ok(())
}
