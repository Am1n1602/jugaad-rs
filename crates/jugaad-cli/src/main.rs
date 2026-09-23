use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use jugaad_core::nse::{
    ChartPeriod, IndexChartPeriod, Instrument, NseArchives, NseCorporateAnnouncements,
    NseCorporateResults, NseDailyReports, NseHistory, NseIndexHistory, NseLiveMarket, NseQuote,
    OptionChainKind, OptionType, ResultPeriod,
};

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

/// Option-chain flavor as accepted on the command line, mapped onto
/// jugaad_core's `OptionChainKind`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OptionChainKindArg {
    Index,
    Equity,
}

impl From<OptionChainKindArg> for OptionChainKind {
    fn from(kind: OptionChainKindArg) -> Self {
        match kind {
            OptionChainKindArg::Index => OptionChainKind::Index,
            OptionChainKindArg::Equity => OptionChainKind::Equity,
        }
    }
}

/// Financial-results period as accepted on the command line, mapped onto
/// jugaad_core's `ResultPeriod`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ResultPeriodArg {
    Annual,
    Quarterly,
}

impl From<ResultPeriodArg> for ResultPeriod {
    fn from(period: ResultPeriodArg) -> Self {
        match period {
            ResultPeriodArg::Annual => ResultPeriod::Annual,
            ResultPeriodArg::Quarterly => ResultPeriod::Quarterly,
        }
    }
}

/// Chart time window as accepted on the command line, mapped onto
/// jugaad_core's `ChartPeriod`. Renamed to NSE's own short period codes
/// (`1d`/`1w`/...) rather than clap's default kebab-case (`one-day`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ChartPeriodArg {
    #[value(name = "1d")]
    OneDay,
    #[value(name = "1w")]
    OneWeek,
    #[value(name = "1m")]
    OneMonth,
    #[value(name = "1y")]
    OneYear,
    #[value(name = "5y")]
    FiveYears,
}

impl From<ChartPeriodArg> for ChartPeriod {
    fn from(period: ChartPeriodArg) -> Self {
        match period {
            ChartPeriodArg::OneDay => ChartPeriod::OneDay,
            ChartPeriodArg::OneWeek => ChartPeriod::OneWeek,
            ChartPeriodArg::OneMonth => ChartPeriod::OneMonth,
            ChartPeriodArg::OneYear => ChartPeriod::OneYear,
            ChartPeriodArg::FiveYears => ChartPeriod::FiveYears,
        }
    }
}

/// Index chart time window as accepted on the command line, mapped onto
/// jugaad_core's `IndexChartPeriod`. A larger set than `ChartPeriodArg`
/// (stocks) - this endpoint also accepts `3m`/`6m`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum IndexChartPeriodArg {
    #[value(name = "1d")]
    OneDay,
    #[value(name = "1w")]
    OneWeek,
    #[value(name = "1m")]
    OneMonth,
    #[value(name = "3m")]
    ThreeMonths,
    #[value(name = "6m")]
    SixMonths,
    #[value(name = "1y")]
    OneYear,
    #[value(name = "5y")]
    FiveYears,
}

impl From<IndexChartPeriodArg> for IndexChartPeriod {
    fn from(period: IndexChartPeriodArg) -> Self {
        match period {
            IndexChartPeriodArg::OneDay => IndexChartPeriod::OneDay,
            IndexChartPeriodArg::OneWeek => IndexChartPeriod::OneWeek,
            IndexChartPeriodArg::OneMonth => IndexChartPeriod::OneMonth,
            IndexChartPeriodArg::ThreeMonths => IndexChartPeriod::ThreeMonths,
            IndexChartPeriodArg::SixMonths => IndexChartPeriod::SixMonths,
            IndexChartPeriodArg::OneYear => IndexChartPeriod::OneYear,
            IndexChartPeriodArg::FiveYears => IndexChartPeriod::FiveYears,
        }
    }
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
    /// Download NSE's daily F&O (derivatives) bhavcopy for one date
    BhavcopyFo {
        /// Trading date to fetch, e.g. 2023-09-27
        date: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/fo_bhavcopy")]
        output: PathBuf,
    },
    /// Download NSE's "full" bhavcopy (every series, with delivery data)
    FullBhavcopy {
        /// Trading date to fetch, e.g. 2023-09-27
        date: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/full_bhavcopy")]
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
    /// Download NSE's current bulk deals report (no date - always the latest)
    BulkDeals {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/bulk_deals.csv")]
        output: PathBuf,
    },
    /// List NSE's available daily reports for a segment (e.g. bulk deals,
    /// volatility, block deals, and 35+ others) - only today's and
    /// yesterday's files are ever available through this API
    ListDailyReports {
        /// Market segment, e.g. "CM" (capital market) or "FO" (derivatives)
        #[arg(short, long, default_value = "CM")]
        segment: String,
    },
    /// Download one of NSE's daily reports by file key (see list-daily-reports)
    DailyReport {
        /// File key, e.g. "CM-BULK-DEAL" (see list-daily-reports for options)
        file_key: String,
        /// Market segment, e.g. "CM" (capital market) or "FO" (derivatives)
        #[arg(short, long, default_value = "CM")]
        segment: String,
        /// Directory to save the file into
        #[arg(short, long, default_value = "data/nse/daily_reports")]
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
    /// Download an index's daily P/E, P/B and dividend yield over a date range
    IndexPe {
        /// Index name, e.g. "NIFTY 50" (quote it if it contains spaces)
        name: String,
        /// Start date (inclusive), e.g. 2024-08-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-08-31
        #[arg(short, long)]
        to: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_pe_history")]
        output: PathBuf,
    },
    /// Download an index's daily Total Return Index values over a date range
    IndexTri {
        /// Index name, e.g. "NIFTY 50" (quote it if it contains spaces).
        /// For most indices this is also the display name; for "strategy"
        /// indices it's a short internal code - pass --index-name for those.
        name: String,
        /// Start date (inclusive), e.g. 2024-08-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-08-31
        #[arg(short, long)]
        to: NaiveDate,
        /// Display name, if different from `name` (strategy indices only) -
        /// defaults to `name`
        #[arg(long)]
        index_name: Option<String>,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_tri_history")]
        output: PathBuf,
    },
    /// List the top-level index categories (e.g. "Equity", "Fixed Income")
    IndexTypes,
    /// List index sub-categories for a category and group
    IndexSubtypes {
        /// e.g. "Equity", "Fixed Income", "Multi Asset" (see index-types)
        #[arg(long)]
        index_type: String,
        /// e.g. "Historical Index Data", "Total returns Index Values ",
        /// "P/E, P/B & Div.Yield values"
        #[arg(long)]
        index_group: String,
    },
    /// List index names for a sub-category and group
    IndexNames {
        /// e.g. "Broad Market Indices" (see index-subtypes)
        #[arg(long)]
        index_type: String,
        /// e.g. "Historical Index Data", "Total returns Index Values ",
        /// "P/E, P/B & Div.Yield values"
        #[arg(long)]
        index_group: String,
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
    /// Show whether each market segment is currently open
    MarketStatus {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/market_status.csv")]
        output: PathBuf,
    },
    /// Download a live snapshot of every NSE index
    IndexSnapshot {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/index_snapshot.csv")]
        output: PathBuf,
    },
    /// Download market-wide turnover (volume/value/open interest) by segment
    MarketTurnover {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/market_turnover.csv")]
        output: PathBuf,
    },
    /// Download a live snapshot of NIFTY index futures/options
    LiveFo {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/live_fo.csv")]
        output: PathBuf,
    },
    /// Download today's block deals (pre-open and mid-day sessions)
    BlockDealSession {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/block_deal_session.csv")]
        output: PathBuf,
    },
    /// Download NSE's top-20 F&O turnover leaderboards (by value and by volume)
    EqDerivativeTurnover {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/eq_derivative_turnover.csv")]
        output: PathBuf,
    },
    /// Download top gainers/losers across all 7 index/security scopes
    MarketMovers {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/market_movers.csv")]
        output: PathBuf,
    },
    /// Download NSE's "Most Active Equities" leaderboards (by traded
    /// value and by traded volume)
    MostActiveEquities {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/most_active_equities.csv")]
        output: PathBuf,
    },
    /// Download NSE's "Volume Gainers" list (stocks trading well above
    /// their recent average volume)
    VolumeGainers {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/volume_gainers.csv")]
        output: PathBuf,
    },
    /// Download stocks hitting a new 52-week high or low
    FiftyTwoWeek {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/fifty_two_week.csv")]
        output: PathBuf,
    },
    /// Download today's large deals (bulk, short, and block deals)
    LargeDeals {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/large_deals.csv")]
        output: PathBuf,
    },
    /// Download a stock's live quote (price, order book depth, volume)
    StockQuote {
        /// Stock symbol, e.g. SBIN or TCS
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/stock_quote")]
        output: PathBuf,
    },
    /// Download a stock's intraday or historical price chart
    StockChart {
        /// Stock symbol, e.g. SBIN or TCS
        symbol: String,
        /// Time window: 1d, 1w, 1m, 1y, or 5y
        #[arg(short, long, value_enum, default_value = "1d")]
        period: ChartPeriodArg,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/stock_chart")]
        output: PathBuf,
    },
    /// Download every F&O contract (all expiries/strikes) for a symbol
    DerivativeQuote {
        /// Symbol, e.g. NIFTY or RELIANCE
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/derivative_quote")]
        output: PathBuf,
    },
    /// Download a single index's live value, volume and turnover
    IndexQuote {
        /// Index name, e.g. "NIFTY 50" (quote it if it contains spaces)
        name: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_quote")]
        output: PathBuf,
    },
    /// Download an index or equity's option chain
    OptionChain {
        /// Symbol, e.g. NIFTY (index) or SBIN (equity)
        symbol: String,
        /// Whether `symbol` is an index or an equity
        #[arg(short, long, value_enum)]
        kind: OptionChainKindArg,
        /// Expiry date, e.g. 2026-09-22 - defaults to the nearest expiry
        #[arg(short, long)]
        expiry: Option<NaiveDate>,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/option_chain")]
        output: PathBuf,
    },
    /// Download a currency pair's option chain
    CurrencyOptionChain {
        /// Currency pair symbol, e.g. USDINR
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/currency_option_chain")]
        output: PathBuf,
    },
    /// Download a symbol's financial-results filings - only the
    /// "equities" and "sme" segments are supported
    FinancialResults {
        /// Symbol, e.g. TCS or SBIN
        symbol: String,
        /// Listed-entity segment - only "equities" and "sme" are supported
        #[arg(short, long, default_value = "equities")]
        segment: String,
        /// Annual or quarterly filings
        #[arg(short, long, value_enum)]
        period: ResultPeriodArg,
        /// Start date (inclusive), e.g. 2018-01-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2024-12-31
        #[arg(short, long)]
        to: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/financial_results")]
        output: PathBuf,
    },
    /// Download a filing's raw XBRL document (get the URL from
    /// financial-results' xbrl_url column)
    DownloadXbrl {
        /// XBRL URL
        url: String,
        /// Directory to save the file into
        #[arg(short, long, default_value = "data/nse/financial_results")]
        output: PathBuf,
    },
    /// Download a filing's raw HTML detail page (get the URL from
    /// financial-results' result_detailed_data_link column) - only
    /// available for older filings that have no real XBRL
    DownloadResultHtml {
        /// HTML detail-page URL
        url: String,
        /// Directory to save the file into
        #[arg(short, long, default_value = "data/nse/financial_results")]
        output: PathBuf,
    },
    /// Download a symbol's corporate announcements (board meetings,
    /// credit ratings, press releases, and similar exchange disclosures)
    CorporateAnnouncements {
        /// Listed-entity segment: equities, sme, debt, mf, invitsreits,
        /// or municipalBond
        #[arg(short, long, default_value = "equities")]
        segment: String,
        /// Symbol to filter to, e.g. TCS or SBIN - omit for every symbol
        /// in the segment
        #[arg(long)]
        symbol: Option<String>,
        /// Start date (inclusive), e.g. 2026-01-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2026-09-21
        #[arg(short, long)]
        to: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/corporate_announcements")]
        output: PathBuf,
    },
    /// Download Social Stock Exchange (registered social enterprise)
    /// announcements - a different response shape than
    /// corporate-announcements, so its own command
    SseAnnouncements {
        /// Symbol to filter to, e.g. "EF-SE" - omit for every SSE entity
        #[arg(long)]
        symbol: Option<String>,
        /// Start date (inclusive), e.g. 2026-01-01
        #[arg(short, long)]
        from: NaiveDate,
        /// End date (inclusive), e.g. 2026-09-21
        #[arg(short, long)]
        to: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/sse_announcements")]
        output: PathBuf,
    },
    /// Download an announcement's raw attachment (get the URL from
    /// corporate-announcements' or sse-announcements' attachment_url
    /// column) - a PDF in every case seen so far, saved as whatever file
    /// type NSE actually serves
    DownloadAnnouncementAttachment {
        /// Attachment URL
        url: String,
        /// Directory to save the file into
        #[arg(short, long, default_value = "data/nse/corporate_announcements")]
        output: PathBuf,
    },
    /// List NSE's trading holidays across every market segment
    HolidayList {
        /// File path to save the CSV to
        #[arg(short, long, default_value = "data/nse/holiday_list.csv")]
        output: PathBuf,
    },
    /// Download NSE's whole-market index bhavcopy (daily closing snapshot
    /// of every index) for one date
    IndexBhavcopy {
        /// Date, e.g. 2026-09-22
        date: NaiveDate,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_bhavcopy")]
        output: PathBuf,
    },
    /// Download a symbol's SEBI registration details
    RegDetails {
        /// Symbol, e.g. SBIN or TCS
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/reg_details")]
        output: PathBuf,
    },
    /// List the names of every index a symbol is a constituent of
    IndexList {
        /// Symbol, e.g. SBIN or TCS
        symbol: String,
    },
    /// Download a symbol's static metadata (eligibility flags, series, ISIN)
    SymbolMeta {
        /// Symbol, e.g. SBIN or TCS
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/symbol_meta")]
        output: PathBuf,
    },
    /// Download the company name behind a symbol
    SymbolName {
        /// Symbol, e.g. SBIN or TCS
        symbol: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/symbol_name")]
        output: PathBuf,
    },
    /// Download a symbol's price change over several trailing windows
    /// alongside its benchmark index's change over the same windows
    YearwiseData {
        /// Symbol, e.g. SBIN or TCS
        symbol: String,
        /// Series, e.g. EQ
        #[arg(short, long, default_value = "EQ")]
        series: String,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/yearwise_data")]
        output: PathBuf,
    },
    /// Download an index's intraday or historical price chart
    IndexChart {
        /// Index name, e.g. "NIFTY 50" (quote it if it contains spaces)
        name: String,
        /// Time window: 1d, 1w, 1m, 3m, 6m, 1y, or 5y
        #[arg(short, long, value_enum, default_value = "1d")]
        period: IndexChartPeriodArg,
        /// Directory to save the CSV into
        #[arg(short, long, default_value = "data/nse/index_chart")]
        output: PathBuf,
    },
}

// clap's derive macro generates recursive command-matching code sized to
// the number of subcommands; in an unoptimized debug build (no inlining
// or tail-call elimination), a CLI this size can overflow the default
// ~1MB Windows main-thread stack before even reaching `Cli::parse()` -
// confirmed live: `jugaad version` (no args at all) crashed with
// STATUS_STACK_OVERFLOW once the subcommand count grew past ~30, and
// only in debug builds (`--release` never reproduced it). Running
// everything on an explicitly larger stack sidesteps the limit rather
// than relying on release-only builds or shrinking the CLI.
fn main() -> anyhow::Result<()> {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(run_cli)?
        .join()
        .map_err(|panic| anyhow::anyhow!("jugaad worker thread panicked: {panic:?}"))?
}

fn run_cli() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => println!("jugaad-core {}", jugaad_core::version()),
        Command::Bhavcopy { date, output } => {
            std::fs::create_dir_all(&output)?;
            let archives = NseArchives::new()?;
            let path = archives.bhavcopy_save(date, &output).await?;
            println!("Saved bhavcopy to {}", path.display());
        }
        Command::BhavcopyFo { date, output } => {
            std::fs::create_dir_all(&output)?;
            let archives = NseArchives::new()?;
            let path = archives.bhavcopy_fo_save(date, &output).await?;
            println!("Saved F&O bhavcopy to {}", path.display());
        }
        Command::FullBhavcopy { date, output } => {
            std::fs::create_dir_all(&output)?;
            let archives = NseArchives::new()?;
            let path = archives.full_bhavcopy_save(date, &output).await?;
            println!("Saved full bhavcopy to {}", path.display());
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
        Command::BulkDeals { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let archives = NseArchives::new()?;
            let path = archives.bulk_deals_save(&output).await?;
            println!("Saved bulk deals to {}", path.display());
        }
        Command::ListDailyReports { segment } => {
            let reports = NseDailyReports::new()?;
            for summary in reports.list_available_reports(&segment).await? {
                println!("{} ({})", summary.file_key, summary.display_name);
                for date in &summary.dates {
                    println!(
                        "  {}  {}  {}",
                        date.trading_date, date.file_size, date.file_name
                    );
                }
            }
        }
        Command::DailyReport {
            file_key,
            segment,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let reports = NseDailyReports::new()?;
            let path = reports
                .download_report_save(&file_key, &segment, &output)
                .await?;
            println!("Saved {file_key} to {}", path.display());
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
        Command::IndexPe {
            name,
            from,
            to,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let history = NseIndexHistory::new()?;
            let path = history
                .index_pe_history_csv(&name, from, to, &output)
                .await?;
            println!("Saved index P/E history to {}", path.display());
        }
        Command::IndexTri {
            name,
            from,
            to,
            index_name,
            output,
        } => {
            let index_name = index_name.as_deref().unwrap_or(&name);
            std::fs::create_dir_all(&output)?;
            let history = NseIndexHistory::new()?;
            let path = history
                .index_tri_history_csv(&name, index_name, from, to, &output)
                .await?;
            println!("Saved index TRI history to {}", path.display());
        }
        Command::IndexTypes => {
            let history = NseIndexHistory::new()?;
            for index_type in history.index_type_list().await? {
                println!("{index_type}");
            }
        }
        Command::IndexSubtypes {
            index_type,
            index_group,
        } => {
            let history = NseIndexHistory::new()?;
            for subtype in history
                .index_subtype_list(&index_type, &index_group)
                .await?
            {
                println!("{subtype}");
            }
        }
        Command::IndexNames {
            index_type,
            index_group,
        } => {
            let history = NseIndexHistory::new()?;
            for name in history.index_name_list(&index_type, &index_group).await? {
                println!("{name}");
            }
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
        Command::MarketStatus { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.market_status_csv(&output).await?;
            println!("Saved market status to {}", path.display());
        }
        Command::IndexSnapshot { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.index_snapshot_csv(&output).await?;
            println!("Saved index snapshot to {}", path.display());
        }
        Command::MarketTurnover { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.market_turnover_csv(&output).await?;
            println!("Saved market turnover to {}", path.display());
        }
        Command::LiveFo { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.live_fo_snapshot_csv(&output).await?;
            println!("Saved live F&O snapshot to {}", path.display());
        }
        Command::BlockDealSession { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.block_deal_session_csv(&output).await?;
            println!("Saved block deal session to {}", path.display());
        }
        Command::MarketMovers { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.market_movers_csv(&output).await?;
            println!("Saved market movers to {}", path.display());
        }
        Command::MostActiveEquities { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.most_active_equities_csv(&output).await?;
            println!("Saved most active equities to {}", path.display());
        }
        Command::VolumeGainers { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.volume_gainers_csv(&output).await?;
            println!("Saved volume gainers to {}", path.display());
        }
        Command::FiftyTwoWeek { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.fifty_two_week_csv(&output).await?;
            println!("Saved 52-week highs/lows to {}", path.display());
        }
        Command::LargeDeals { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.large_deals_csv(&output).await?;
            println!("Saved large deals to {}", path.display());
        }
        Command::EqDerivativeTurnover { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.eq_derivative_turnover_csv(&output).await?;
            println!("Saved equity derivative turnover to {}", path.display());
        }
        Command::StockQuote { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.stock_quote_csv(&symbol, &output).await?;
            println!("Saved stock quote to {}", path.display());
        }
        Command::StockChart {
            symbol,
            period,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote
                .stock_chart_data_csv(&symbol, period.into(), &output)
                .await?;
            println!("Saved stock chart data to {}", path.display());
        }
        Command::DerivativeQuote { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.derivative_quote_csv(&symbol, &output).await?;
            println!("Saved derivative quote to {}", path.display());
        }
        Command::IndexQuote { name, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.index_quote_csv(&name, &output).await?;
            println!("Saved index quote to {}", path.display());
        }
        Command::OptionChain {
            symbol,
            kind,
            expiry,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote
                .option_chain_csv(&symbol, kind.into(), expiry, &output)
                .await?;
            println!("Saved option chain to {}", path.display());
        }
        Command::CurrencyOptionChain { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.currency_option_chain_csv(&symbol, &output).await?;
            println!("Saved currency option chain to {}", path.display());
        }
        Command::FinancialResults {
            symbol,
            segment,
            period,
            from,
            to,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateResults::new()?;
            let path = client
                .financial_results_csv(&segment, &symbol, period.into(), from, to, &output)
                .await?;
            println!("Saved financial results to {}", path.display());
        }
        Command::DownloadXbrl { url, output } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateResults::new()?;
            let path = client.download_xbrl_save(&url, &output).await?;
            println!("Saved XBRL document to {}", path.display());
        }
        Command::DownloadResultHtml { url, output } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateResults::new()?;
            let path = client.download_result_html_save(&url, &output).await?;
            println!("Saved result detail page to {}", path.display());
        }
        Command::CorporateAnnouncements {
            segment,
            symbol,
            from,
            to,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateAnnouncements::new()?;
            let path = client
                .corporate_announcements_csv(&segment, symbol.as_deref(), from, to, &output)
                .await?;
            println!("Saved corporate announcements to {}", path.display());
        }
        Command::SseAnnouncements {
            symbol,
            from,
            to,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateAnnouncements::new()?;
            let path = client
                .sse_announcements_csv(symbol.as_deref(), from, to, &output)
                .await?;
            println!("Saved SSE announcements to {}", path.display());
        }
        Command::DownloadAnnouncementAttachment { url, output } => {
            std::fs::create_dir_all(&output)?;
            let client = NseCorporateAnnouncements::new()?;
            let path = client.download_attachment_save(&url, &output).await?;
            println!("Saved attachment to {}", path.display());
        }
        Command::HolidayList { output } => {
            if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let live = NseLiveMarket::new()?;
            let path = live.holiday_list_csv(&output).await?;
            println!("Saved holiday list to {}", path.display());
        }
        Command::IndexBhavcopy { date, output } => {
            std::fs::create_dir_all(&output)?;
            let history = NseIndexHistory::new()?;
            let path = history.index_bhavcopy_save(date, &output).await?;
            println!("Saved index bhavcopy to {}", path.display());
        }
        Command::RegDetails { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.reg_details_csv(&symbol, &output).await?;
            println!("Saved reg details to {}", path.display());
        }
        Command::IndexList { symbol } => {
            let quote = NseQuote::new()?;
            for name in quote.index_list_raw(&symbol).await? {
                println!("{name}");
            }
        }
        Command::SymbolMeta { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.symbol_meta_csv(&symbol, &output).await?;
            println!("Saved symbol meta to {}", path.display());
        }
        Command::SymbolName { symbol, output } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.symbol_name_csv(&symbol, &output).await?;
            println!("Saved symbol name to {}", path.display());
        }
        Command::YearwiseData {
            symbol,
            series,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote.yearwise_data_csv(&symbol, &series, &output).await?;
            println!("Saved yearwise data to {}", path.display());
        }
        Command::IndexChart {
            name,
            period,
            output,
        } => {
            std::fs::create_dir_all(&output)?;
            let quote = NseQuote::new()?;
            let path = quote
                .index_chart_data_csv(&name, period.into(), &output)
                .await?;
            println!("Saved index chart data to {}", path.display());
        }
    }
    Ok(())
}
