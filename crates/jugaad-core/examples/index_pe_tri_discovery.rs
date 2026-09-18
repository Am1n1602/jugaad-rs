//! Exercises the new index endpoints: P/E, Total Return Index, and the
//! three discovery endpoints for browsing available index names.
//!
//!     cargo run -p jugaad-core --example index_pe_tri_discovery

use chrono::NaiveDate;
use jugaad_core::nse::NseIndexHistory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseIndexHistory::new()?;

    let from = NaiveDate::from_ymd_opt(2024, 8, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 8, 5).ok_or("invalid date")?;

    let pe_rows = history.index_pe_history_raw("NIFTY 50", from, to).await?;
    println!("P/E rows: {}", pe_rows.len());
    if let Some(row) = pe_rows.first() {
        println!(
            "  {} pe={} pb={} div_yield={}",
            row.date, row.pe, row.pb, row.div_yield
        );
    }

    let tri_rows = history
        .index_tri_history_raw("NIFTY 50", "NIFTY 50", from, to)
        .await?;
    println!("TRI rows: {}", tri_rows.len());
    if let Some(row) = tri_rows.first() {
        println!(
            "  {} total_returns_index={} ntr_value={}",
            row.date, row.total_returns_index, row.ntr_value
        );
    }

    let types = history.index_type_list().await?;
    println!("Index types: {types:?}");

    let subtypes = history
        .index_subtype_list("Equity", "Historical Index Data")
        .await?;
    println!("Equity subtypes: {subtypes:?}");

    let names = history
        .index_name_list("Broad Market Indices", "Historical Index Data")
        .await?;
    println!(
        "Broad Market Indices names (first 5): {:?}",
        &names[..5.min(names.len())]
    );

    Ok(())
}
