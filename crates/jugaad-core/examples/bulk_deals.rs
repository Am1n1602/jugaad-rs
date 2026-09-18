//! Fetches the current bulk deals report - unlike every other example in
//! this crate, this endpoint takes no date; NSE only serves the latest
//! snapshot.
//!
//!     cargo run -p jugaad-core --example bulk_deals

use jugaad_core::nse::NseArchives;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archives = NseArchives::new()?;
    let text = archives.bulk_deals_raw().await?;

    println!("header: {}", text.lines().next().unwrap_or_default());
    println!("{} deals", text.lines().count().saturating_sub(1));

    Ok(())
}
