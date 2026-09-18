//! Lists and downloads NSE's generic daily reports by file key.
//!
//!     cargo run -p jugaad-core --example daily_reports

use jugaad_core::nse::NseDailyReports;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reports = NseDailyReports::new()?;

    let summaries = reports.list_available_reports("CM").await?;
    println!("{} report types available for CM", summaries.len());
    for summary in summaries.iter().take(5) {
        println!(
            "  {} ({}): {} date(s)",
            summary.file_key,
            summary.display_name,
            summary.dates.len()
        );
    }

    let bytes = reports.download_report_raw("CM-BULK-DEAL", "CM").await?;
    println!(
        "CM-BULK-DEAL: {} bytes, starts with: {:?}",
        bytes.len(),
        String::from_utf8_lossy(&bytes[..bytes.len().min(60)])
    );

    Ok(())
}
