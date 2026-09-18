use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::Semaphore;

use super::USER_AGENT;
use super::dates::break_into_month_chunks;
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

// Convert 16-Feb-2024 format to a naivedate format
fn deserialize_nse_date<'de, D>(deserializer: D) -> std::result::Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&raw, "%d-%b-%Y").map_err(serde::de::Error::custom)
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
}
