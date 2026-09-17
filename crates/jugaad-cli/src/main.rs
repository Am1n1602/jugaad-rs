use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use jugaad_core::nse::NseArchives;

#[derive(Debug, Parser)]
#[command(
    name = "jugaad",
    version,
    about = "Rust implication of jugaad-data i.e Indian market data downloader"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    /// Download NSE's daily bhavcopy (whole-market OHLC data) for one date
    Bhavcopy {
        /// Trading date to fetch, e.g. 2023-09-27
        date: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/daily_bhavcopy")]
        output: PathBuf,
    },
}

#[tokio::main]

async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Version => println!("jugaad-core {}", jugaad_core::version()),
        Command::Bhavcopy { date, output } => {
            std::fs::create_dir_all(&output)?;
            let archives = NseArchives::new()?;
            let path = archives.bhavcopy_save(date, &output).await?;
            println!("Saved bhavcopy to {}", path.display());
        }
    }
    Ok(())
}
