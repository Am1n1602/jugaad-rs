//! Fetches F&O price history: NIFTY index futures, and a NIFTY call option
//! for comparison.
//!
//!     cargo run -p jugaad-core --example derivatives_history

use chrono::NaiveDate;
use jugaad_core::nse::{Instrument, NseHistory, OptionType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseHistory::new()?;

    let from = NaiveDate::from_ymd_opt(2024, 12, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 12, 5).ok_or("invalid date")?;
    let expiry = NaiveDate::from_ymd_opt(2024, 12, 26).ok_or("invalid date")?;

    let futures = history
        .derivatives_history_raw("NIFTY", from, to, expiry, Instrument::FutIdx)
        .await?;
    println!("NIFTY futures ({expiry} expiry): {} rows", futures.len());
    if let Some(row) = futures.first() {
        println!(
            "  {} close={} open_interest={} change_in_oi={}",
            row.date, row.close, row.open_interest, row.change_in_oi
        );
    }

    let call = history
        .derivatives_history_raw(
            "NIFTY",
            from,
            to,
            expiry,
            Instrument::OptIdx {
                strike_price: 24000.0,
                option_type: OptionType::Call,
            },
        )
        .await?;
    println!("NIFTY 24000 CE ({expiry} expiry): {} rows", call.len());
    if let Some(row) = call.first() {
        println!("  {} close={}", row.date, row.close);
    }

    Ok(())
}
