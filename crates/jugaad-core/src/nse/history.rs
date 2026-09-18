use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Datelike, NaiveDate};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use super::USER_AGENT;
use super::dates::{break_into_month_chunks, deserialize_nse_date};
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";

// NSE's API isn't documented or guaranteed reliable over long date ranges, so
// long requests get split into calendar-month chunks fetched separately. This
// caps how many of those chunks are in flight at once, to stay polite to NSE.
const MAX_CONCURRENT_REQUESTS: usize = 2;

/// One trading day's OHLC, volume and delivery data for a symbol.
///
/// `rename(deserialize = "...")` is used instead of a plain `rename` so NSE's
/// cryptic field names are only used when *reading* the API response -
/// writing this struct out (e.g. to CSV) uses the clean field names below.
#[derive(Debug, Serialize, Deserialize)]
pub struct StockHistoryRow {
    #[serde(rename(deserialize = "CH_SYMBOL"))]
    pub symbol: String,
    #[serde(rename(deserialize = "CH_SERIES"))]
    pub series: String,
    #[serde(
        rename(deserialize = "mTIMESTAMP"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub date: NaiveDate,
    #[serde(rename(deserialize = "CH_OPENING_PRICE"))]
    pub open: f64,
    #[serde(rename(deserialize = "CH_TRADE_HIGH_PRICE"))]
    pub high: f64,
    #[serde(rename(deserialize = "CH_TRADE_LOW_PRICE"))]
    pub low: f64,
    #[serde(rename(deserialize = "CH_PREVIOUS_CLS_PRICE"))]
    pub prev_close: f64,
    #[serde(rename(deserialize = "CH_LAST_TRADED_PRICE"))]
    pub ltp: f64,
    #[serde(rename(deserialize = "CH_CLOSING_PRICE"))]
    pub close: f64,
    #[serde(rename(deserialize = "VWAP"))]
    pub vwap: f64,
    #[serde(rename(deserialize = "CH_TOT_TRADED_QTY"))]
    pub volume: u64,
    #[serde(rename(deserialize = "CH_TOT_TRADED_VAL"))]
    pub value: f64,
    #[serde(rename(deserialize = "CH_TOTAL_TRADES"))]
    pub trades: Option<u64>,
    #[serde(rename(deserialize = "COP_DELIV_QTY"))]
    pub delivery_qty: Option<u64>,
    #[serde(rename(deserialize = "COP_DELIV_PERC"))]
    pub delivery_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct StockHistoryResponse {
    data: Vec<StockHistoryRow>,
}

/// Whether an option is a call or a put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionType {
    Call,
    Put,
}

impl OptionType {
    fn as_query_param(self) -> &'static str {
        match self {
            OptionType::Call => "CE",
            OptionType::Put => "PE",
        }
    }
}

/// Which F&O contract to fetch history for.
///
/// Python's version takes a bare `instrument_type` string plus optional
/// `strike_price`/`option_type` arguments, checked for consistency at
/// runtime (raising if you ask for an option without a strike price). This
/// enum makes that mistake impossible to construct in the first place - an
/// `OptIdx`/`OptStk` value always carries its strike price and option type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instrument {
    FutIdx,
    FutStk,
    OptIdx {
        strike_price: f64,
        option_type: OptionType,
    },
    OptStk {
        strike_price: f64,
        option_type: OptionType,
    },
}

impl Instrument {
    fn as_query_param(self) -> &'static str {
        match self {
            Instrument::FutIdx => "FUTIDX",
            Instrument::FutStk => "FUTSTK",
            Instrument::OptIdx { .. } => "OPTIDX",
            Instrument::OptStk { .. } => "OPTSTK",
        }
    }

    fn strike_and_option(self) -> Option<(f64, OptionType)> {
        match self {
            Instrument::FutIdx | Instrument::FutStk => None,
            Instrument::OptIdx {
                strike_price,
                option_type,
            }
            | Instrument::OptStk {
                strike_price,
                option_type,
            } => Some((strike_price, option_type)),
        }
    }

    // Used to build a unique CSV filename - futures need nothing extra, but
    // options need the strike/type too, or different contracts for the same
    // symbol/expiry/range would collide on the same file.
    fn filename_suffix(self) -> String {
        match self.strike_and_option() {
            None => self.as_query_param().to_string(),
            Some((strike_price, option_type)) => format!(
                "{}-{strike_price}-{}",
                self.as_query_param(),
                option_type.as_query_param()
            ),
        }
    }
}

/// One trading day's price/open-interest data for a single F&O contract
/// (futures or options, index or stock).
///
/// Like `StockHistoryRow`, `rename(deserialize = "...")` keeps NSE's raw
/// field names to reading the API response only - writing this out (e.g.
/// to CSV) uses the clean field names below.
#[derive(Debug, Serialize, Deserialize)]
pub struct DerivativeHistoryRow {
    #[serde(rename(deserialize = "FH_INSTRUMENT"))]
    pub instrument: String,
    #[serde(rename(deserialize = "FH_SYMBOL"))]
    pub symbol: String,
    #[serde(
        rename(deserialize = "FH_EXPIRY_DT"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub expiry: NaiveDate,
    // 0 for futures, which don't have a strike price.
    #[serde(rename(deserialize = "FH_STRIKE_PRICE"))]
    pub strike_price: f64,
    // "XX" for futures, which aren't options.
    #[serde(rename(deserialize = "FH_OPTION_TYPE"))]
    pub option_type: String,
    #[serde(
        rename(deserialize = "FH_TIMESTAMP"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub date: NaiveDate,
    #[serde(rename(deserialize = "FH_OPENING_PRICE"))]
    pub open: f64,
    #[serde(rename(deserialize = "FH_TRADE_HIGH_PRICE"))]
    pub high: f64,
    #[serde(rename(deserialize = "FH_TRADE_LOW_PRICE"))]
    pub low: f64,
    #[serde(rename(deserialize = "FH_CLOSING_PRICE"))]
    pub close: f64,
    #[serde(rename(deserialize = "FH_LAST_TRADED_PRICE"))]
    pub ltp: f64,
    #[serde(rename(deserialize = "FH_PREV_CLS"))]
    pub prev_close: f64,
    #[serde(rename(deserialize = "FH_SETTLE_PRICE"))]
    pub settle_price: f64,
    #[serde(rename(deserialize = "FH_TOT_TRADED_QTY"))]
    pub volume: u64,
    #[serde(rename(deserialize = "FH_TOT_TRADED_VAL"))]
    pub value: f64,
    #[serde(rename(deserialize = "FH_OPEN_INT"))]
    pub open_interest: u64,
    // Can be negative - open interest can shrink day over day.
    #[serde(rename(deserialize = "FH_CHANGE_IN_OI"))]
    pub change_in_oi: i64,
    #[serde(rename(deserialize = "FH_MARKET_LOT"))]
    pub market_lot: u64,
    // Null for index instruments (NIFTY, ...); populated for stock ones.
    #[serde(rename(deserialize = "FH_UNDERLYING_VALUE"))]
    pub underlying_value: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DerivativeHistoryResponse {
    data: Vec<DerivativeHistoryRow>,
}

#[derive(Debug, Clone)]
pub struct NseHistory {
    client: Client,
    // Tracks whether the cookie warm-up GET has run yet. `&self` methods
    // can't normally mutate fields, but an atomic can be flipped through a
    // shared reference safely - see `fetch_chunk` below. Wrapped in `Arc` so
    // clones of `NseHistory` (one per concurrent task) share one flag rather
    // than each thinking it's the first to warm up.
    warmed_up: Arc<AtomicBool>,
}

impl NseHistory {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_store(true)
            .build()?;
        Ok(Self {
            client,
            warmed_up: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Fetches daily price/volume history for `symbol` between `from_date`
    /// and `to_date` (inclusive). `series` is usually `"EQ"` for ordinary
    /// equity shares. Returns an empty `Vec` if the range has no trading
    /// days or the symbol doesn't exist - NSE's API answers both cases the
    /// same way, so there's no reliable way to tell them apart here.
    ///
    /// Long ranges are split into calendar-month chunks and fetched
    /// concurrently (bounded by `MAX_CONCURRENT_REQUESTS`), since NSE's API
    /// isn't reliable over long spans.
    pub async fn stock_history_raw(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        series: &str,
    ) -> Result<Vec<StockHistoryRow>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        let mut tasks = Vec::new();
        for (chunk_start, chunk_end) in break_into_month_chunks(from_date, to_date) {
            let history = self.clone();
            let symbol = symbol.to_string();
            let series = series.to_string();
            let semaphore = Arc::clone(&semaphore);

            tasks.push(tokio::spawn(async move {
                // Held until this task finishes, so at most
                // MAX_CONCURRENT_REQUESTS tasks run their fetch at once.
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|e| Error::Parse(format!("semaphore closed unexpectedly: {e}")))?;
                history
                    .fetch_chunk(&symbol, chunk_start, chunk_end, &series)
                    .await
            }));
        }

        // Awaiting in the order the chunks were pushed keeps rows in
        // chronological order, even though they were fetched concurrently.
        let mut rows = Vec::new();
        for task in tasks {
            rows.extend(task.await??);
        }

        Ok(rows)
    }

    /// Fetches history the same way as `stock_history_raw`, then writes it
    /// as a CSV file into `dest` (a directory - the filename is derived from
    /// the symbol, date range and series). Returns the path written.
    pub async fn stock_history_csv(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        series: &str,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .stock_history_raw(symbol, from_date, to_date, series)
            .await?;

        let file_name = format!("{symbol}-{from_date}-{to_date}-{series}.csv");
        let path = dest.join(file_name);

        let mut writer = csv::Writer::from_path(&path)?;
        for row in &rows {
            writer.serialize(row)?;
        }
        writer.flush()?;

        Ok(path)
    }

    /// Fetches one chunk directly from NSE in a single request. Callers
    /// should use `stock_history_raw`, which chunks long ranges for you.
    async fn fetch_chunk(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        series: &str,
    ) -> Result<Vec<StockHistoryRow>> {
        // The data API rejects requests that don't carry cookies from a real
        // page visit first. `swap` sets the flag and hands back the prior
        // value in one atomic step, so only the first caller does this.
        if !self.warmed_up.swap(true, Ordering::Relaxed) {
            self.client
                .get(format!("{BASE_URL}/report-detail/eq_security"))
                .send()
                .await?;
        }

        // NSE's API wants "ALL" rather than "EQ" to mean the default equity
        // series. Important: "ALL" really does mean every series active for
        // that symbol/date (not just EQ) - see `filter_by_series` below for
        // why the response still needs filtering afterward.
        let series_param = if series == "EQ" { "ALL" } else { series };
        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();

        let response = self
            .client
            .get(format!(
                "{BASE_URL}/api/historicalOR/generateSecurityWiseHistoricalData"
            ))
            .query(&[
                ("symbol", symbol),
                ("from", from_str.as_str()),
                ("to", to_str.as_str()),
                ("type", "priceVolumeDeliverable"),
                ("series", series_param),
            ])
            .header("Referer", format!("{BASE_URL}/report-detail/eq_security"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: StockHistoryResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse stock history response: {e}")))?;

        Ok(filter_by_series(parsed.data, series))
    }

    /// Fetches daily price/open-interest history for one F&O contract:
    /// `symbol` (e.g. `"NIFTY"` or `"RELIANCE"`) at `expiry_date`, between
    /// `from_date` and `to_date` (inclusive). `instrument` picks futures vs.
    /// options and, for options, carries the strike price and call/put.
    ///
    /// Same empty-`Vec`-on-no-match and concurrent month-chunking behavior
    /// as `stock_history_raw`.
    pub async fn derivatives_history_raw(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        expiry_date: NaiveDate,
        instrument: Instrument,
    ) -> Result<Vec<DerivativeHistoryRow>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        let mut tasks = Vec::new();
        for (chunk_start, chunk_end) in break_into_month_chunks(from_date, to_date) {
            let history = self.clone();
            let symbol = symbol.to_string();
            let semaphore = Arc::clone(&semaphore);

            tasks.push(tokio::spawn(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|e| Error::Parse(format!("semaphore closed unexpectedly: {e}")))?;
                history
                    .fetch_derivatives_chunk(
                        &symbol,
                        chunk_start,
                        chunk_end,
                        expiry_date,
                        instrument,
                    )
                    .await
            }));
        }

        let mut rows = Vec::new();
        for task in tasks {
            rows.extend(task.await??);
        }

        Ok(rows)
    }

    /// Fetches history the same way as `derivatives_history_raw`, then
    /// writes it as a CSV file into `dest` (a directory - the filename is
    /// derived from the symbol, date range, expiry and contract details).
    /// Returns the path written.
    pub async fn derivatives_history_csv(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        expiry_date: NaiveDate,
        instrument: Instrument,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .derivatives_history_raw(symbol, from_date, to_date, expiry_date, instrument)
            .await?;

        let file_name = format!(
            "{symbol}-{from_date}-{to_date}-{expiry_date}-{}.csv",
            instrument.filename_suffix()
        );
        let path = dest.join(file_name);

        let mut writer = csv::Writer::from_path(&path)?;
        for row in &rows {
            writer.serialize(row)?;
        }
        writer.flush()?;

        Ok(path)
    }

    /// Fetches one chunk directly from NSE in a single request. Callers
    /// should use `derivatives_history_raw`, which chunks long ranges.
    async fn fetch_derivatives_chunk(
        &self,
        symbol: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        expiry_date: NaiveDate,
        instrument: Instrument,
    ) -> Result<Vec<DerivativeHistoryRow>> {
        if !self.warmed_up.swap(true, Ordering::Relaxed) {
            self.client
                .get(format!("{BASE_URL}/report-detail/eq_security"))
                .send()
                .await?;
        }

        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();
        // NSE wants the expiry uppercased, e.g. "26-DEC-2024".
        let expiry_str = expiry_date.format("%d-%b-%Y").to_string().to_uppercase();
        let year_str = from_date.year().to_string();

        let mut query = vec![
            ("symbol", symbol),
            ("from", from_str.as_str()),
            ("to", to_str.as_str()),
            ("expiryDate", expiry_str.as_str()),
            ("instrumentType", instrument.as_query_param()),
            ("year", year_str.as_str()),
        ];

        let strike_str;
        if let Some((strike_price, option_type)) = instrument.strike_and_option() {
            strike_str = format!("{strike_price:.2}");
            query.push(("strikePrice", strike_str.as_str()));
            query.push(("optionType", option_type.as_query_param()));
        }

        let response = self
            .client
            .get(format!("{BASE_URL}/api/historicalOR/foCPV"))
            .query(&query)
            .header("Referer", format!("{BASE_URL}/report-detail/eq_security"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: DerivativeHistoryResponse = response.json().await.map_err(|e| {
            Error::Parse(format!("could not parse derivatives history response: {e}"))
        })?;

        Ok(parsed.data)
    }
}

/// Keeps only rows matching `series`, unless `series` is `"ALL"` (a request
/// for every series, passed straight through).
///
/// This exists because of a real NSE quirk: asking for the default `"EQ"`
/// series requires querying NSE with `"ALL"` instead (see `fetch_chunk`), but
/// `"ALL"` returns every series NSE has for that symbol/date - not just EQ.
/// For example, SBIN on 2024-08-19 has both an `"EQ"` row and a `"T0"` row (a
/// same-day-settlement trial NSE ran on some symbols that year) - without
/// this filter, a caller asking for `"EQ"` would silently get both.
fn filter_by_series(data: Vec<StockHistoryRow>, series: &str) -> Vec<StockHistoryRow> {
    if series == "ALL" {
        return data;
    }
    data.into_iter()
        .filter(|row| row.series == series)
        .collect()
}

// Panicking via `.unwrap()` on a failed assertion is the normal, intended
// way for a test to fail - the workspace-wide `unwrap_used` lint is aimed at
// production code paths, not test code, hence the blanket allow below.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // Real response captured from NSE's historicalOR API for SBIN.
    const SAMPLE_RESPONSE: &str = r#"{"data":[{"CH_SYMBOL":"SBIN","CH_SERIES":"EQ",
        "mTIMESTAMP":"05-Aug-2024","CH_PREVIOUS_CLS_PRICE":847.85,"CH_OPENING_PRICE":830,
        "CH_TRADE_HIGH_PRICE":831.35,"CH_TRADE_LOW_PRICE":800,"CH_LAST_TRADED_PRICE":810,
        "CH_CLOSING_PRICE":811.65,"VWAP":815.37,"CH_TOT_TRADED_QTY":27676951,
        "CH_TOT_TRADED_VAL":22567000834,"CH_TOTAL_TRADES":539085,
        "CH_TIMESTAMP":"2024-08-04T18:30:00.000Z","COP_DELIV_QTY":12248505,
        "COP_DELIV_PERC":44.26}]}"#;

    #[test]
    fn deserializes_real_nse_response_shape() {
        let parsed: StockHistoryResponse = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.symbol, "SBIN");
        assert_eq!(row.date, date(2024, 8, 5));
        assert_eq!(row.close, 811.65);
        assert_eq!(row.trades, Some(539_085));
    }

    // Older records (and some series) don't have this field at all - NSE
    // sends it back as JSON `null` rather than omitting the key.
    const SAMPLE_RESPONSE_MISSING_TRADES: &str = r#"{"data":[{"CH_SYMBOL":"SBIN","CH_SERIES":"EQ",
        "mTIMESTAMP":"05-Jan-2010","CH_PREVIOUS_CLS_PRICE":2291.2,"CH_OPENING_PRICE":2308,
        "CH_TRADE_HIGH_PRICE":2310,"CH_TRADE_LOW_PRICE":2280.1,"CH_LAST_TRADED_PRICE":2294,
        "CH_CLOSING_PRICE":2292.05,"VWAP":2292.78,"CH_TOT_TRADED_QTY":1161374,
        "CH_TOT_TRADED_VAL":2662775673.75,"CH_TOTAL_TRADES":null,
        "CH_TIMESTAMP":"2010-01-04T18:30:00.000Z","COP_DELIV_QTY":522468,
        "COP_DELIV_PERC":44.99}]}"#;

    #[test]
    fn missing_optional_fields_deserialize_to_none() {
        let parsed: StockHistoryResponse =
            serde_json::from_str(SAMPLE_RESPONSE_MISSING_TRADES).unwrap();
        assert_eq!(parsed.data[0].trades, None);
    }

    #[test]
    fn serializing_uses_clean_field_names_not_nse_names() {
        let parsed: StockHistoryResponse = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&parsed.data[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "symbol,series,date,open,high,low,prev_close,ltp,close,vwap,volume,value,trades,delivery_qty,delivery_pct"
        );
    }

    // Real response captured from NSE for SBIN on 2024-08-19, a day it had
    // both a "T0" (same-day-settlement trial) row and a normal "EQ" row.
    const SAMPLE_RESPONSE_MULTIPLE_SERIES: &str = r#"{"data":[
        {"CH_SYMBOL":"SBIN","CH_SERIES":"T0","mTIMESTAMP":"19-Aug-2024",
        "CH_PREVIOUS_CLS_PRICE":812.1,"CH_OPENING_PRICE":820,"CH_TRADE_HIGH_PRICE":820,
        "CH_TRADE_LOW_PRICE":820,"CH_LAST_TRADED_PRICE":820,"CH_CLOSING_PRICE":813.7,
        "VWAP":820,"CH_TOT_TRADED_QTY":1,"CH_TOT_TRADED_VAL":820,"CH_TOTAL_TRADES":1,
        "CH_TIMESTAMP":"2024-08-18T18:30:00.000Z","COP_DELIV_QTY":1,"COP_DELIV_PERC":100},
        {"CH_SYMBOL":"SBIN","CH_SERIES":"EQ","mTIMESTAMP":"19-Aug-2024",
        "CH_PREVIOUS_CLS_PRICE":812.1,"CH_OPENING_PRICE":815,"CH_TRADE_HIGH_PRICE":825.4,
        "CH_TRADE_LOW_PRICE":812.6,"CH_LAST_TRADED_PRICE":814.6,"CH_CLOSING_PRICE":813.7,
        "VWAP":817.66,"CH_TOT_TRADED_QTY":10151482,"CH_TOT_TRADED_VAL":8300440391.9,
        "CH_TOTAL_TRADES":174502,"CH_TIMESTAMP":"2024-08-18T18:30:00.000Z",
        "COP_DELIV_QTY":2595511,"COP_DELIV_PERC":25.57}]}"#;

    #[test]
    fn filter_by_series_keeps_only_the_requested_series() {
        let parsed: StockHistoryResponse =
            serde_json::from_str(SAMPLE_RESPONSE_MULTIPLE_SERIES).unwrap();

        let eq_only = filter_by_series(parsed.data, "EQ");
        assert_eq!(eq_only.len(), 1);
        assert_eq!(eq_only[0].series, "EQ");
        assert_eq!(eq_only[0].volume, 10_151_482);
    }

    #[test]
    fn filter_by_series_all_passes_everything_through() {
        let parsed: StockHistoryResponse =
            serde_json::from_str(SAMPLE_RESPONSE_MULTIPLE_SERIES).unwrap();

        assert_eq!(filter_by_series(parsed.data, "ALL").len(), 2);
    }

    #[test]
    fn instrument_query_params_match_what_nse_expects() {
        assert_eq!(Instrument::FutIdx.as_query_param(), "FUTIDX");
        assert_eq!(Instrument::FutStk.as_query_param(), "FUTSTK");
        assert_eq!(Instrument::FutIdx.strike_and_option(), None);

        let call = Instrument::OptIdx {
            strike_price: 24000.0,
            option_type: OptionType::Call,
        };
        assert_eq!(call.as_query_param(), "OPTIDX");
        assert_eq!(call.strike_and_option(), Some((24000.0, OptionType::Call)));
        assert_eq!(OptionType::Call.as_query_param(), "CE");
        assert_eq!(OptionType::Put.as_query_param(), "PE");
    }

    // Real response captured from NSE for NIFTY index futures (FUTIDX).
    const SAMPLE_FUTIDX_RESPONSE: &str = r#"{"data":[{"FH_INSTRUMENT":"FUTIDX",
        "FH_SYMBOL":"NIFTY","FH_EXPIRY_DT":"26-Dec-2024","FH_STRIKE_PRICE":0,
        "FH_OPTION_TYPE":"XX","FH_MARKET_TYPE":"N","FH_OPENING_PRICE":24597,
        "FH_TRADE_HIGH_PRICE":24930,"FH_TRADE_LOW_PRICE":24396,"FH_CLOSING_PRICE":24764.35,
        "FH_LAST_TRADED_PRICE":24775.35,"FH_PREV_CLS":24561.7,"FH_SETTLE_PRICE":24764.35,
        "FH_TOT_TRADED_QTY":12468000,"FH_TOT_TRADED_VAL":3078866.77,"FH_OPEN_INT":11174400,
        "FH_CHANGE_IN_OI":-376650,"FH_MARKET_LOT":25,"FH_TIMESTAMP":"05-Dec-2024",
        "FH_TIMESTAMP_ORDER":"2024-12-04T18:30:00.000Z","FH_UNDERLYING_VALUE":null,
        "CALCULATED_PREMIUM_VAL":3078866.77}]}"#;

    #[test]
    fn deserializes_real_futures_response_shape() {
        let parsed: DerivativeHistoryResponse =
            serde_json::from_str(SAMPLE_FUTIDX_RESPONSE).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.instrument, "FUTIDX");
        assert_eq!(row.symbol, "NIFTY");
        assert_eq!(row.expiry, date(2024, 12, 26));
        assert_eq!(row.date, date(2024, 12, 5));
        assert_eq!(row.strike_price, 0.0);
        assert_eq!(row.option_type, "XX");
        // Open interest fell that day - must deserialize into a signed type.
        assert_eq!(row.change_in_oi, -376_650);
        assert_eq!(row.underlying_value, None);
    }

    // Real response captured from NSE for RELIANCE stock futures (FUTSTK).
    const SAMPLE_FUTSTK_RESPONSE: &str = r#"{"data":[{"FH_INSTRUMENT":"FUTSTK",
        "FH_SYMBOL":"RELIANCE","FH_EXPIRY_DT":"26-Dec-2024","FH_STRIKE_PRICE":0,
        "FH_OPTION_TYPE":"XX","FH_MARKET_TYPE":"N","FH_OPENING_PRICE":1319.75,
        "FH_TRADE_HIGH_PRICE":1334.7,"FH_TRADE_LOW_PRICE":1310.8,"FH_CLOSING_PRICE":1326.75,
        "FH_LAST_TRADED_PRICE":1325.55,"FH_PREV_CLS":1315.35,"FH_SETTLE_PRICE":1326.75,
        "FH_TOT_TRADED_QTY":32970000,"FH_TOT_TRADED_VAL":436858.24,"FH_OPEN_INT":160645000,
        "FH_CHANGE_IN_OI":-4710000,"FH_MARKET_LOT":500,"FH_TIMESTAMP":"05-Dec-2024",
        "FH_TIMESTAMP_ORDER":"2024-12-04T18:30:00.000Z","FH_UNDERLYING_VALUE":1322.05,
        "CALCULATED_PREMIUM_VAL":436858.24}]}"#;

    #[test]
    fn underlying_value_is_populated_for_stock_instruments() {
        let parsed: DerivativeHistoryResponse =
            serde_json::from_str(SAMPLE_FUTSTK_RESPONSE).unwrap();

        assert_eq!(parsed.data[0].underlying_value, Some(1322.05));
    }

    #[test]
    fn derivatives_serializing_uses_clean_field_names_not_nse_names() {
        let parsed: DerivativeHistoryResponse =
            serde_json::from_str(SAMPLE_FUTIDX_RESPONSE).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&parsed.data[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "instrument,symbol,expiry,strike_price,option_type,date,open,high,low,close,ltp,prev_close,settle_price,volume,value,open_interest,change_in_oi,market_lot,underlying_value"
        );
    }

    #[test]
    fn instrument_filename_suffix_disambiguates_option_contracts() {
        assert_eq!(Instrument::FutIdx.filename_suffix(), "FUTIDX");
        assert_eq!(
            Instrument::OptIdx {
                strike_price: 24000.0,
                option_type: OptionType::Call
            }
            .filename_suffix(),
            "OPTIDX-24000-CE"
        );
    }
}
