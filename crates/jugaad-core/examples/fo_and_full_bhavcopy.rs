//! Fetches F&O bhavcopy (old + UDiff era) and full bhavcopy.
//!
//!     cargo run -p jugaad-core --example fo_and_full_bhavcopy

use chrono::NaiveDate;
use jugaad_core::nse::NseArchives;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archives = NseArchives::new()?;

    let old_date = NaiveDate::from_ymd_opt(2020, 1, 1).ok_or("invalid date")?;
    let fo_old = archives.bhavcopy_fo_raw(old_date).await?;
    println!(
        "F&O bhavcopy {old_date} (pre-UDiff): {} rows, header: {}",
        fo_old.lines().count().saturating_sub(1),
        fo_old.lines().next().unwrap_or_default()
    );

    let new_date = NaiveDate::from_ymd_opt(2024, 8, 1).ok_or("invalid date")?;
    let fo_new = archives.bhavcopy_fo_raw(new_date).await?;
    println!(
        "F&O bhavcopy {new_date} (UDiff): {} rows, header: {}",
        fo_new.lines().count().saturating_sub(1),
        fo_new.lines().next().unwrap_or_default()
    );

    let full = archives.full_bhavcopy_raw(new_date).await?;
    println!(
        "Full bhavcopy {new_date}: {} rows, header: {}",
        full.lines().count().saturating_sub(1),
        full.lines().next().unwrap_or_default()
    );

    Ok(())
}
