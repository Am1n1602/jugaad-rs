//! Fetches bhavcopy for dates before AND after the 2024-07-08 UDiff cutover,
//! showing `bhavcopy_raw` automatically picking the right format for each.
//!
//!     cargo run -p jugaad-core --example old_bhavcopy

use chrono::NaiveDate;
use jugaad_core::nse::NseArchives;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archives = NseArchives::new()?;

    let old_date = NaiveDate::from_ymd_opt(2020, 1, 1).ok_or("invalid date")?;
    let old_text = archives.bhavcopy_raw(old_date).await?;
    println!(
        "{old_date} (pre-cutover) header: {}",
        old_text.lines().next().unwrap_or_default()
    );
    println!(
        "{old_date}: {} rows",
        old_text.lines().count().saturating_sub(1)
    );

    let new_date = NaiveDate::from_ymd_opt(2024, 8, 1).ok_or("invalid date")?;
    let new_text = archives.bhavcopy_raw(new_date).await?;
    println!(
        "{new_date} (post-cutover) header: {}",
        new_text.lines().next().unwrap_or_default()
    );
    println!(
        "{new_date}: {} rows",
        new_text.lines().count().saturating_sub(1)
    );

    Ok(())
}
