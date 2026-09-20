pub mod archives;
pub mod corporate_results;
pub mod daily_reports;
mod dates;
pub mod history;
pub mod index;
pub mod live;
pub mod quote;

pub use archives::NseArchives;
pub use corporate_results::{
    AuditStatus, ConsolidationBasis, FinancialResultRow, NseCorporateResults, ResultPeriod,
};
pub use daily_reports::{NseDailyReports, ReportDate, ReportSummary};
pub use history::{DerivativeHistoryRow, Instrument, NseHistory, OptionType, StockHistoryRow};
pub use index::{IndexHistoryRow, IndexPeRow, IndexTriRow, NseIndexHistory};
pub use live::{
    BlockDealRow, EqDerivativeTurnoverRow, IndexSnapshotRow, LiveFoRow, MarketSegmentStatus,
    MarketTurnoverRow, NseLiveMarket,
};
pub use quote::{
    CurrencyOptionChainRow, CurrencyOptionLeg, DerivativeQuoteRow, IndexQuote, NseQuote,
    OptionChainKind, OptionChainRow, OptionLeg, OrderBook, OrderBookLevel, StockQuote,
};

// NSE blocks requests that don't look like they came from a browser.
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
