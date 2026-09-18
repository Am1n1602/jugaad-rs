use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use jugaad_core::nse::{NseArchives, NseHistory};

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
    /// Print the current version
    Version,
    /// Download NSE's daily bhavcopy (whole-market OHLC data) for one date
    Bhavcopy {
        /// Trading date to fetch, e.g. 2023-09-27
        date: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/daily_bhavcopy")]
        output: PathBuf,
    },
    /// Download a stock's daily price/volume history over a date range
    Stock {
        /// Stock symbol, e.g. SBIN or TCS
        symbol: String,
        /// Start date (inclusive), e.g. 2024-08-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-08-31
        #[arg(short, long)]
        to: NaiveDate,
        /// NSE series - "EQ" for ordinary equity shares
        #[arg(short, long, default_value = "EQ")]
        series: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/stock_history")]
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
        Command::Stock {
            symbol,
            from,
            to,
            series,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let history = NseHistory::new()?;
            let path = history
                .stock_history_csv(&symbol, from, to, &series, &output)
                .await?;
            println!("Saved stock history to {}", path.display());
        }
    }
    Ok(())
}
