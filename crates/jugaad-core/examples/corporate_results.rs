//! Exercises the corporate financial-results endpoint: lists TCS's annual
//! filings (spanning both the placeholder-XBRL era and the real-XBRL
//! era), then downloads the raw XBRL/HTML for the oldest and newest ones.
//!
//!     cargo run -p jugaad-core --example corporate_results

use chrono::NaiveDate;
use jugaad_core::nse::{NseCorporateResults, ResultPeriod};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = NseCorporateResults::new()?;

    let from = NaiveDate::from_ymd_opt(2012, 1, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2026, 9, 19).ok_or("invalid date")?;

    let mut rows = client
        .financial_results_raw("equities", "TCS", ResultPeriod::Annual, from, to)
        .await?;
    rows.sort_by_key(|r| r.from_date);

    println!("TCS annual filings: {}", rows.len());
    for row in &rows {
        println!(
            "  {} to {}  consolidated={:?} audited={:?}  xbrl={}",
            row.from_date,
            row.to_date,
            row.consolidated,
            row.audited,
            row.xbrl_url.is_some()
        );
    }

    if let Some(link) = rows
        .first()
        .and_then(|r| r.result_detailed_data_link.as_deref())
    {
        let bytes = client.download_result_html_raw(link).await?;
        println!(
            "\nOldest filing's HTML fallback page: {} bytes",
            bytes.len()
        );
    }

    if let Some(xbrl) = rows.last().and_then(|r| r.xbrl_url.as_deref()) {
        let bytes = client.download_xbrl_raw(xbrl).await?;
        println!("Newest filing's XBRL document: {} bytes", bytes.len());
    }

    Ok(())
}
