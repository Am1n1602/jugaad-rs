pub mod archives;
mod dates;
pub mod history;
pub mod index;

pub use archives::NseArchives;
pub use history::{NseHistory, StockHistoryRow};
pub use index::{IndexHistoryRow, NseIndexHistory};

// NSE blocks requests that don't look like they came from a browser.
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
