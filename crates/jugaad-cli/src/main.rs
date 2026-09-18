use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use jugaad_core::nse::{Instrument, NseArchives, NseHistory, NseIndexHistory, OptionType};

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

/// Instrument type as accepted on the command line - matched against
/// `--strike-price`/`--option-type` at runtime since clap can't express
/// jugaad_core's `Instrument` enum's "options carry a strike and type"
/// invariant directly through flags.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum InstrumentKind {
    FutIdx,
    FutStk,
    OptIdx,
    OptStk,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OptionTypeArg {
    Call,
    Put,
}

/// Builds a validated `Instrument` from the loose CLI flags, requiring
/// `strike_price`/`option_type` exactly when `kind` is an option type.
fn build_instrument(
    kind: InstrumentKind,
    strike_price: Option<f64>,
    option_type: Option<OptionTypeArg>,
) -> anyhow::Result<Instrument> {
    match kind {
        InstrumentKind::FutIdx => Ok(Instrument::FutIdx),
        InstrumentKind::FutStk => Ok(Instrument::FutStk),
        InstrumentKind::OptIdx | InstrumentKind::OptStk => {
            let strike_price = strike_price
                .ok_or_else(|| anyhow::anyhow!("--strike-price is required for options"))?;
            let option_type = match option_type
                .ok_or_else(|| anyhow::anyhow!("--option-type is required for options"))?
            {
                OptionTypeArg::Call => OptionType::Call,
                OptionTypeArg::Put => OptionType::Put,
            };
            Ok(match kind {
                InstrumentKind::OptIdx => Instrument::OptIdx {
                    strike_price,
                    option_type,
                },
                _ => Instrument::OptStk {
                    strike_price,
                    option_type,
                },
            })
        }
    }
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
    /// Download an index's daily OHLC history over a date range
    Index {
        /// Index name, e.g. "NIFTY 50" (quote it if it contains spaces)
        name: String,
        /// Start date (inclusive), e.g. 2024-08-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-08-31
        #[arg(short, long)]
        to: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_history")]
        output: PathBuf,
    },
    /// Download F&O (futures/options) daily price and open-interest history
    Derivatives {
        /// Symbol, e.g. NIFTY or RELIANCE
        symbol: String,
        /// Start date (inclusive), e.g. 2024-12-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-12-05
        #[arg(short, long)]
        to: NaiveDate,
        /// Contract expiry date, e.g. 2024-12-26
        #[arg(short, long)]
        expiry: NaiveDate,
        /// Instrument type
        #[arg(short, long, value_enum)]
        instrument: InstrumentKind,
        /// Strike price - required for optidx/optstk
        #[arg(short = 'p', long)]
        strike_price: Option<f64>,
        /// Call or put - required for optidx/optstk
        #[arg(long, value_enum)]
        option_type: Option<OptionTypeArg>,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/derivatives_history")]
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
        Command::Index {
            name,
            from,
            to,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let history = NseIndexHistory::new()?;
            let path = history.index_history_csv(&name, from, to, &output).await?;
            println!("Saved index history to {}", path.display());
        }
        Command::Derivatives {
            symbol,
            from,
            to,
            expiry,
            instrument,
            strike_price,
            option_type,
            output,
        } => {
            let instrument = build_instrument(instrument, strike_price, option_type)?;
            std::fs::create_dir_all(&output)?;
            let history = NseHistory::new()?;
            let path = history
                .derivatives_history_csv(&symbol, from, to, expiry, instrument, &output)
                .await?;
            println!("Saved derivatives history to {}", path.display());
        }
    }
    Ok(())
}
