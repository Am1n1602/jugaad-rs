use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};

use super::USER_AGENT;
use super::dates::deserialize_nse_date;
use super::quote::NEXTAPI_URL;
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";

/// One market segment's live open/closed status (Capital Market, Currency,
/// Commodity, Debt).
#[derive(Debug, Serialize)]
pub struct MarketSegmentStatus {
    pub market: String,
    pub status: String,
    pub trade_date: String,
    pub index: Option<String>,
    pub last: Option<String>,
    pub change: Option<String>,
    pub percent_change: Option<String>,
    pub status_message: String,
}

/// NSE's `marketState` array also carries a few unrelated blurbs (a
/// USD-adjusted NIFTY figure, a currency-futures pseudo row) that don't
/// name a `market` at all - confirmed live. Deserializing into this fully
/// optional shape first, then filtering/mapping into `MarketSegmentStatus`
/// in `market_status_raw`, tolerates that without lying about the shape.
#[derive(Debug, Deserialize)]
struct RawSegment {
    market: Option<String>,
    #[serde(rename = "marketStatus")]
    status: Option<String>,
    #[serde(rename = "tradeDate")]
    trade_date: Option<String>,
    index: Option<String>,
    #[serde(default, deserialize_with = "deserialize_flexible_number")]
    last: Option<String>,
    // Confirmed live: the four named segments call this "variation", but
    // the USD-adjusted NIFTY entry calls the same kind of value "change".
    #[serde(
        rename = "variation",
        alias = "change",
        default,
        deserialize_with = "deserialize_flexible_number"
    )]
    change: Option<String>,
    #[serde(
        rename = "percentChange",
        default,
        deserialize_with = "deserialize_flexible_number"
    )]
    percent_change: Option<String>,
    #[serde(rename = "marketStatusMessage")]
    status_message: Option<String>,
}

impl RawSegment {
    /// `None` for entries that don't name a `market` - see the module docs.
    fn into_named(self) -> Option<MarketSegmentStatus> {
        Some(MarketSegmentStatus {
            market: self.market?,
            status: self.status?,
            trade_date: self.trade_date?,
            index: self.index,
            last: self.last,
            change: self.change,
            percent_change: self.percent_change,
            status_message: self.status_message?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct MarketStatusResponse {
    #[serde(rename = "marketState")]
    market_state: Vec<RawSegment>,
}

/// `last`/`variation`/`change`/`percentChange` in `marketState` arrive as a
/// JSON number, a numeric JSON string, or an empty string ("not
/// applicable", e.g. `Currency` while closed)
/// depending on which segment. Normalized to `Option<String>` here rather
/// than parsed further, since these are display values, not something a
/// caller would compute on.
fn deserialize_flexible_number<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) if s.trim().is_empty() => Ok(None),
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Number(n) => Ok(Some(n.to_string())),
        other => Err(serde::de::Error::custom(format!(
            "expected a number or string, got {other}"
        ))),
    }
}

/// One index's live snapshot: current price/change, day range, 52-week
/// range, valuation ratios and market breadth (advances/declines).
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexSnapshotRow {
    #[serde(rename(deserialize = "key"))]
    pub category: String,
    #[serde(rename(deserialize = "index"))]
    pub name: String,
    #[serde(rename(deserialize = "indexSymbol"))]
    pub symbol: String,
    pub last: f64,
    #[serde(rename(deserialize = "variation"))]
    pub change: f64,
    #[serde(rename(deserialize = "percentChange"))]
    pub percent_change: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    #[serde(rename(deserialize = "previousClose"))]
    pub prev_close: f64,
    #[serde(rename(deserialize = "yearHigh"))]
    pub year_high: f64,
    #[serde(rename(deserialize = "yearLow"))]
    pub year_low: f64,
    // Empty string for indices with no meaningful P/E, e.g. NIFTY50 USD -
    // confirmed live.
    #[serde(
        rename(deserialize = "pe"),
        deserialize_with = "deserialize_string_f64_opt"
    )]
    pub pe: Option<f64>,
    #[serde(
        rename(deserialize = "pb"),
        deserialize_with = "deserialize_string_f64_opt"
    )]
    pub pb: Option<f64>,
    #[serde(
        rename(deserialize = "dy"),
        deserialize_with = "deserialize_string_f64_opt"
    )]
    pub div_yield: Option<f64>,
    // Absent entirely for indices with no derivatives rather than null or zero.
    #[serde(
        rename(deserialize = "advances"),
        default,
        deserialize_with = "deserialize_string_u32_opt"
    )]
    pub advances: Option<u32>,
    #[serde(
        rename(deserialize = "declines"),
        default,
        deserialize_with = "deserialize_string_u32_opt"
    )]
    pub declines: Option<u32>,
    #[serde(
        rename(deserialize = "unchanged"),
        default,
        deserialize_with = "deserialize_string_u32_opt"
    )]
    pub unchanged: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct IndexSnapshotResponse {
    data: Vec<IndexSnapshotRow>,
}

fn deserialize_string_f64_opt<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    raw.parse().map(Some).map_err(serde::de::Error::custom)
}

fn deserialize_string_u32_opt<'de, D>(deserializer: D) -> std::result::Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map(Some).map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
struct TurnoverFigures {
    volume: u64,
    value: f64,
    #[serde(rename = "openInterest")]
    open_interest: u64,
}

#[derive(Debug, Deserialize)]
struct RawTurnoverEntry {
    name: Option<String>,
    yesterday: TurnoverFigures,
}

#[derive(Debug, Deserialize)]
struct MarketTurnoverResponse {
    data: Vec<RawTurnoverEntry>,
}

#[derive(Debug, Serialize)]
pub struct MarketTurnoverRow {
    pub name: Option<String>,
    pub volume: u64,
    pub value: f64,
    pub open_interest: u64,
}

impl From<RawTurnoverEntry> for MarketTurnoverRow {
    fn from(raw: RawTurnoverEntry) -> Self {
        MarketTurnoverRow {
            name: raw.name,
            volume: raw.yesterday.volume,
            value: raw.yesterday.value,
            open_interest: raw.yesterday.open_interest,
        }
    }
}

/// One contract's live price/open-interest snapshot from the NIFTY
/// index-futures/options bucket - see `live_fo_snapshot_raw`.
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveFoRow {
    pub underlying: String,
    pub identifier: String,
    #[serde(rename(deserialize = "instrumentType"))]
    pub instrument_type: String,
    pub instrument: String,
    pub contract: String,
    #[serde(
        rename(deserialize = "expiryDate"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub expiry: NaiveDate,
    // "-" for futures, which aren't options.
    #[serde(rename(deserialize = "optionType"))]
    pub option_type: String,
    #[serde(rename(deserialize = "strikePrice"))]
    pub strike_price: f64,
    #[serde(rename(deserialize = "lastPrice"))]
    pub last_price: f64,
    pub change: f64,
    #[serde(rename(deserialize = "pChange"))]
    pub percent_change: f64,
    #[serde(rename(deserialize = "openPrice"))]
    pub open: f64,
    #[serde(rename(deserialize = "highPrice"))]
    pub high: f64,
    #[serde(rename(deserialize = "lowPrice"))]
    pub low: f64,
    #[serde(rename(deserialize = "closePrice"))]
    pub close_price: f64,
    pub volume: u64,
    // "totalTurnover"/"value"/"premiumTurnOver" are always identical -
    // confirmed live - so only one is kept.
    #[serde(rename(deserialize = "totalTurnover"))]
    pub turnover: f64,
    #[serde(rename(deserialize = "underlyingValue"))]
    pub underlying_value: f64,
    #[serde(rename(deserialize = "openInterest"))]
    pub open_interest: u64,
    #[serde(rename(deserialize = "noOfTrades"))]
    pub trades: u64,
}

#[derive(Debug, Deserialize)]
struct LiveFoResponse {
    data: Vec<LiveFoRow>,
}

/// One block deal within a trading session (`"session1"`, the pre-open
/// negotiated-deal window, or `"session2"`, the mid-day window). NSE's own
/// response keeps these as two separate lists; `block_deal_session_raw`
/// flattens them into one `Vec` tagged by `session`, matching the
/// one-row-per-record convention used elsewhere in this crate.
#[derive(Debug, Serialize)]
pub struct BlockDealRow {
    pub session: String,
    pub identifier: String,
    pub symbol: String,
    pub series: String,
    pub market_type: String,
    pub change: f64,
    pub percent_change: f64,
    pub last_price: f64,
    pub open: f64,
    pub day_high: f64,
    pub day_low: f64,
    pub previous_close: f64,
    pub average_price: f64,
    pub total_traded_volume: u64,
    pub total_traded_value: f64,
    pub total_buy_quantity: u64,
    pub total_sell_quantity: u64,
    // Always null in every entry seen live - kept as `Option` rather than
    // dropped, in case they populate for deals this crate hasn't observed
    // yet (e.g. ones tied to a corporate action).
    pub status: Option<String>,
    pub ex_date: Option<String>,
    pub purpose: Option<String>,
    pub last_update_time: String,
}

#[derive(Debug, Deserialize)]
struct RawBlockDeal {
    identifier: String,
    symbol: String,
    series: String,
    #[serde(rename = "marketType")]
    market_type: String,
    change: f64,
    // Duplicate of `pChange` (same value, confirmed live) - only one is
    // kept, same convention as elsewhere in this crate.
    #[serde(rename = "pChange")]
    percent_change: f64,
    #[serde(rename = "lastPrice")]
    last_price: f64,
    open: f64,
    #[serde(rename = "dayHigh")]
    day_high: f64,
    #[serde(rename = "dayLow")]
    day_low: f64,
    #[serde(rename = "previousClose")]
    previous_close: f64,
    #[serde(rename = "averagePrice")]
    average_price: f64,
    #[serde(rename = "totalTradedVolume")]
    total_traded_volume: u64,
    #[serde(rename = "totalTradedValue")]
    total_traded_value: f64,
    #[serde(rename = "totalBuyQuantity")]
    total_buy_quantity: u64,
    #[serde(rename = "totalSellQuantity")]
    total_sell_quantity: u64,
    status: Option<String>,
    #[serde(rename = "exDate")]
    ex_date: Option<String>,
    purpose: Option<String>,
    #[serde(rename = "lastUpdateTime")]
    last_update_time: String,
}

impl RawBlockDeal {
    fn into_row(self, session: &str) -> BlockDealRow {
        BlockDealRow {
            session: session.to_string(),
            identifier: self.identifier,
            symbol: self.symbol,
            series: self.series,
            market_type: self.market_type,
            change: self.change,
            percent_change: self.percent_change,
            last_price: self.last_price,
            open: self.open,
            day_high: self.day_high,
            day_low: self.day_low,
            previous_close: self.previous_close,
            average_price: self.average_price,
            total_traded_volume: self.total_traded_volume,
            total_traded_value: self.total_traded_value,
            total_buy_quantity: self.total_buy_quantity,
            total_sell_quantity: self.total_sell_quantity,
            status: self.status,
            ex_date: self.ex_date,
            purpose: self.purpose,
            last_update_time: self.last_update_time,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawBlockDealData {
    session1: Vec<RawBlockDeal>,
    session2: Vec<RawBlockDeal>,
}

#[derive(Debug, Deserialize)]
struct BlockDealSessionResponse {
    data: RawBlockDealData,
}

#[derive(Debug, Clone)]
pub struct NseLiveMarket {
    client: Client,
}

impl NseLiveMarket {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    /// Fetches whether each market segment (Capital Market, Currency,
    /// Commodity, Debt) is currently open, with a same-day snapshot value
    /// where NSE provides one.
    pub async fn market_status_raw(&self) -> Result<Vec<MarketSegmentStatus>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/marketStatus"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: MarketStatusResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse market status response: {e}")))?;

        Ok(parsed
            .market_state
            .into_iter()
            .filter_map(RawSegment::into_named)
            .collect())
    }

    /// Fetches market status the same way as `market_status_raw`, then
    /// writes it as a CSV file to `path` exactly. Like `bulk_deals_save`,
    /// this takes a full file path rather than a destination directory
    /// (there's no date/range to derive a filename from) and always
    /// overwrites, since the data is a live snapshot that changes intraday.
    pub async fn market_status_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.market_status_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches a live snapshot of every NSE index.
    pub async fn index_snapshot_raw(&self) -> Result<Vec<IndexSnapshotRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/allIndices"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: IndexSnapshotResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse index snapshot response: {e}")))?;

        Ok(parsed.data)
    }

    /// Fetches the index snapshot the same way as `index_snapshot_raw`, then
    /// writes it as a CSV file to `path` exactly - see `market_status_csv`
    /// for why this takes a full file path and always overwrites.
    pub async fn index_snapshot_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.index_snapshot_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches market-wide turnover (volume/value/open interest) by segment
    /// as of the last completed trading session.
    pub async fn market_turnover_raw(&self) -> Result<Vec<MarketTurnoverRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/market-turnover"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: MarketTurnoverResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse market turnover response: {e}")))?;

        Ok(parsed
            .data
            .into_iter()
            .map(MarketTurnoverRow::from)
            .collect())
    }

    /// Fetches turnover the same way as `market_turnover_raw`, then writes
    /// it as a CSV file to `path` exactly - see `market_status_csv` for why
    /// this takes a full file path and always overwrites.
    pub async fn market_turnover_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.market_turnover_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches a live snapshot of NIFTY index futures/options. Takes no
    /// parameter: confirmed live, `index=nse50_fut` is the only value this
    /// endpoint accepts - every other bucket NSE's own UI seems to
    /// reference (e.g. `banknifty_fut`, `niftyit_fut`) returns a 500.
    pub async fn live_fo_snapshot_raw(&self) -> Result<Vec<LiveFoRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/liveEquity-derivatives"))
            .query(&[("index", "nse50_fut")])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: LiveFoResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse live F&O response: {e}")))?;

        Ok(parsed.data)
    }

    /// Fetches the F&O snapshot the same way as `live_fo_snapshot_raw`, then
    /// writes it as a CSV file to `path` exactly - see `market_status_csv`
    /// for why this takes a full file path and always overwrites.
    pub async fn live_fo_snapshot_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.live_fo_snapshot_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches today's block deals across both trading sessions
    /// (pre-open and mid-day). Hits the same generic NextApi endpoint
    /// `NseQuote` uses, with `functionName=getBlockDealSession`.
    pub async fn block_deal_session_raw(&self) -> Result<Vec<BlockDealRow>> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[("functionName", "getBlockDealSession")])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: BlockDealSessionResponse = response.json().await.map_err(|e| {
            Error::Parse(format!("could not parse block deal session response: {e}"))
        })?;

        let mut rows = Vec::new();
        rows.extend(
            parsed
                .data
                .session1
                .into_iter()
                .map(|raw| raw.into_row("session1")),
        );
        rows.extend(
            parsed
                .data
                .session2
                .into_iter()
                .map(|raw| raw.into_row("session2")),
        );
        Ok(rows)
    }

    /// Fetches block deals the same way as `block_deal_session_raw`, then
    /// writes them as a CSV file to `path` exactly - see
    /// `market_status_csv` for why this takes a full file path and always
    /// overwrites.
    pub async fn block_deal_session_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.block_deal_session_raw().await?;
        write_csv(&rows, path)
    }
}

/// Shared by every `_csv` method in this module (and in `quote.rs`):
/// serializes `rows` to `path`, overwriting anything already there.
pub(super) fn write_csv<T: Serialize>(rows: &[T], path: &Path) -> Result<PathBuf> {
    let mut writer = csv::Writer::from_path(path)?;
    for row in rows {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(path.to_path_buf())
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

    // Real response shape captured from NSE's marketStatus API - trimmed to
    // the marketState array, which is all this module models. Includes both
    // "variation" (Capital Market) and "change" (NIFTY50 USD) key names,
    // and both an open and a closed segment.
    const SAMPLE_MARKET_STATUS: &str = r#"{"marketState":[
        {"market":"Capital Market","marketStatus":"Closed","tradeDate":"18-Sep-2026 15:30",
         "index":"NIFTY 50","last":23346.4,"variation":75.80000000000291,"percentChange":0.33,
         "marketStatusMessage":"Normal Market has Closed"},
        {"market":"Commodity","marketStatus":"Open","tradeDate":"18-Sep-2026",
         "index":"","last":"","variation":"","percentChange":"","marketStatusMessage":"Market is Open"},
        {"index":"NIFTY50 USD","last":8435.8,"change":31.090000000000146,"percentChange":0.37,
         "timestamp":"18-Sep-2026 15:39"},
        {"market":"currencyfuture","marketStatus":"Closed","tradeDate":"18-Sep-2026",
         "index":"","last":"95.9600","variation":"","percentChange":"","marketStatusMessage":"Market is Closed",
         "expiryDate":"28-Sep-2026","underlying":"USDINR"}
    ]}"#;

    #[test]
    fn market_status_keeps_only_entries_that_name_a_market() {
        let parsed: MarketStatusResponse = serde_json::from_str(SAMPLE_MARKET_STATUS).unwrap();
        let segments: Vec<MarketSegmentStatus> = parsed
            .market_state
            .into_iter()
            .filter_map(RawSegment::into_named)
            .collect();

        // The NIFTY50 USD entry (no "market" key) is dropped.
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].market, "Capital Market");
        assert_eq!(segments[0].change.as_deref(), Some("75.80000000000291"));
    }

    #[test]
    fn market_status_treats_empty_string_fields_as_none() {
        let parsed: MarketStatusResponse = serde_json::from_str(SAMPLE_MARKET_STATUS).unwrap();
        let segments: Vec<MarketSegmentStatus> = parsed
            .market_state
            .into_iter()
            .filter_map(RawSegment::into_named)
            .collect();

        let commodity = segments.iter().find(|s| s.market == "Commodity").unwrap();
        assert_eq!(commodity.last, None);
        assert_eq!(commodity.change, None);
        assert_eq!(commodity.percent_change, None);
    }

    #[test]
    fn market_status_reads_a_numeric_string_last_value() {
        let parsed: MarketStatusResponse = serde_json::from_str(SAMPLE_MARKET_STATUS).unwrap();
        let segments: Vec<MarketSegmentStatus> = parsed
            .market_state
            .into_iter()
            .filter_map(RawSegment::into_named)
            .collect();

        let currency_future = segments
            .iter()
            .find(|s| s.market == "currencyfuture")
            .unwrap();
        assert_eq!(currency_future.last.as_deref(), Some("95.9600"));
    }

    // Real response captured from NSE's allIndices API for NIFTY 50.
    const SAMPLE_INDEX_SNAPSHOT: &str = r#"{"data":[
        {"key":"INDICES ELIGIBLE IN DERIVATIVES","index":"NIFTY 50","indexSymbol":"NIFTY 50",
         "last":23346.4,"variation":75.8,"percentChange":0.33,"open":23334.7,"high":23389.15,
         "low":23286.6,"previousClose":23270.6,"yearHigh":26373.2,"yearLow":22182.55,
         "indicativeClose":0,"pe":"19.74","pb":"2.82","dy":"1.21","declines":"24","advances":"26",
         "unchanged":"0"}
    ]}"#;

    #[test]
    fn deserializes_real_index_snapshot_shape() {
        let parsed: IndexSnapshotResponse = serde_json::from_str(SAMPLE_INDEX_SNAPSHOT).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.name, "NIFTY 50");
        assert_eq!(row.last, 23346.4);
        assert_eq!(row.pe, Some(19.74));
        assert_eq!(row.advances, Some(26));
    }

    // Real response captured for NIFTY50 USD - a strategy index with no
    // meaningful P/E/P/B/dividend yield and no derivatives (so no
    // advances/declines/unchanged at all).
    const SAMPLE_INDEX_SNAPSHOT_NO_PE_NO_BREADTH: &str = r#"{"data":[
        {"key":"STRATEGY INDICES","index":"NIFTY50 USD","indexSymbol":"NIFTY50 USD","last":8435.8,
         "variation":31.09,"percentChange":0.37,"open":8404.7,"high":8483.2,"low":8404.05,
         "previousClose":8404.71,"yearHigh":0,"yearLow":0,"indicativeClose":0,"pe":"","pb":"","dy":""}
    ]}"#;

    #[test]
    fn index_snapshot_treats_empty_pe_as_none_and_missing_breadth_as_none() {
        let parsed: IndexSnapshotResponse =
            serde_json::from_str(SAMPLE_INDEX_SNAPSHOT_NO_PE_NO_BREADTH).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.pe, None);
        assert_eq!(row.pb, None);
        assert_eq!(row.div_yield, None);
        assert_eq!(row.advances, None);
        assert_eq!(row.declines, None);
        assert_eq!(row.unchanged, None);
    }

    #[test]
    fn index_snapshot_serializing_uses_clean_field_names() {
        let parsed: IndexSnapshotResponse = serde_json::from_str(SAMPLE_INDEX_SNAPSHOT).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&parsed.data[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "category,name,symbol,last,change,percent_change,open,high,low,prev_close,year_high,year_low,pe,pb,div_yield,advances,declines,unchanged"
        );
    }

    // Real response captured from NSE's market-turnover API, trimmed to two
    // segments - one normal, one with a null name (a real NSE data quirk).
    const SAMPLE_MARKET_TURNOVER: &str = r#"{"data":[
        {"name":"Equities","yesterday":{"volume":3528797247,"value":1107796241968.77,"openInterest":0},"today":{}},
        {"name":null,"yesterday":{"volume":0,"value":3688914932.47,"openInterest":0},"today":{}}
    ]}"#;

    #[test]
    fn deserializes_real_market_turnover_shape() {
        let parsed: MarketTurnoverResponse = serde_json::from_str(SAMPLE_MARKET_TURNOVER).unwrap();
        let rows: Vec<MarketTurnoverRow> = parsed
            .data
            .into_iter()
            .map(MarketTurnoverRow::from)
            .collect();

        assert_eq!(rows[0].name.as_deref(), Some("Equities"));
        assert_eq!(rows[0].volume, 3_528_797_247);
        assert_eq!(rows[1].name, None);
        assert_eq!(rows[1].value, 3_688_914_932.47);
    }

    // Real response captured from NSE's liveEquity-derivatives API for
    // index=nse50_fut.
    const SAMPLE_LIVE_FO: &str = r#"{"data":[
        {"underlying":"NIFTY","identifier":"FUTIDXNIFTY29-09-2026XX0.00","instrumentType":"FUTIDX",
         "instrument":"Index Futures","contract":"NIFTY 29-Sep-2026","expiryDate":"29-Sep-2026",
         "optionType":"-","strikePrice":0,"lastPrice":23380,"change":46.6,"pChange":0.2,
         "openPrice":23379,"highPrice":23420,"lowPrice":23312.6,"closePrice":23378.5,
         "volume":1508520,"totalTurnover":35250627718.8,"value":35250627718.8,
         "premiumTurnOver":35250627718.8,"underlyingValue":23346.4,"openInterest":268025,
         "noOfTrades":23208}
    ]}"#;

    #[test]
    fn deserializes_real_live_fo_shape() {
        let parsed: LiveFoResponse = serde_json::from_str(SAMPLE_LIVE_FO).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.underlying, "NIFTY");
        assert_eq!(row.instrument_type, "FUTIDX");
        assert_eq!(row.expiry, date(2026, 9, 29));
        assert_eq!(row.turnover, 35_250_627_718.8);
        assert_eq!(row.open_interest, 268_025);
    }

    #[test]
    fn live_fo_serializing_uses_clean_field_names() {
        let parsed: LiveFoResponse = serde_json::from_str(SAMPLE_LIVE_FO).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&parsed.data[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "underlying,identifier,instrument_type,instrument,contract,expiry,option_type,strike_price,last_price,change,percent_change,open,high,low,close_price,volume,turnover,underlying_value,open_interest,trades"
        );
    }

    // Real response captured from NSE's NextApi getBlockDealSession -
    // session1 empty (confirmed live: this window hadn't happened yet
    // that day), session2 with two deals.
    const SAMPLE_BLOCK_DEAL_SESSION: &str = r#"{"data":{"session1":[],"session2":[
        {"identifier":"ENTEROBLO","symbol":"ENTERO","series":"BL","marketType":"O","change":28.4,
         "lastPrice":1695,"totalTradedVolume":1390000,"status":null,"open":1695,"dayHigh":1695,
         "dayLow":1695,"previousClose":1666.6,"averagePrice":1695,"totalBuyQuantity":0,
         "totalSellQuantity":0,"onlineIndex":0,"lastUpdateTime":"18-Sep-2026 14:06:38",
         "totalTradedValue":2356050000,"exDate":null,"purpose":null,"PChange":1.7,"pChange":1.7},
        {"identifier":"TMCVBLO","symbol":"TMCV","series":"BL","marketType":"O","change":1.9,
         "lastPrice":438,"totalTradedVolume":706512,"status":null,"open":438,"dayHigh":438,
         "dayLow":438,"previousClose":436.1,"averagePrice":438,"totalBuyQuantity":0,
         "totalSellQuantity":0,"onlineIndex":0,"lastUpdateTime":"18-Sep-2026 14:06:53",
         "totalTradedValue":309452256,"exDate":null,"purpose":null,"PChange":0.44,"pChange":0.44}
    ]}}"#;

    #[test]
    fn deserializes_real_block_deal_session_shape() {
        let parsed: BlockDealSessionResponse =
            serde_json::from_str(SAMPLE_BLOCK_DEAL_SESSION).unwrap();

        assert!(parsed.data.session1.is_empty());
        assert_eq!(parsed.data.session2.len(), 2);
        assert_eq!(parsed.data.session2[0].symbol, "ENTERO");
        assert_eq!(parsed.data.session2[0].percent_change, 1.7);
        assert_eq!(parsed.data.session2[0].status, None);
    }

    #[test]
    fn block_deal_rows_are_tagged_with_their_session() {
        let parsed: BlockDealSessionResponse =
            serde_json::from_str(SAMPLE_BLOCK_DEAL_SESSION).unwrap();

        let mut rows: Vec<BlockDealRow> = parsed
            .data
            .session1
            .into_iter()
            .map(|raw| raw.into_row("session1"))
            .collect();
        rows.extend(
            parsed
                .data
                .session2
                .into_iter()
                .map(|raw| raw.into_row("session2")),
        );

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.session == "session2"));
        assert_eq!(rows[1].symbol, "TMCV");
        assert_eq!(rows[1].last_price, 438.0);
    }

    #[test]
    fn block_deal_serializing_uses_clean_field_names() {
        let parsed: BlockDealSessionResponse =
            serde_json::from_str(SAMPLE_BLOCK_DEAL_SESSION).unwrap();
        let row = parsed
            .data
            .session2
            .into_iter()
            .next()
            .unwrap()
            .into_row("session2");

        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&row).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "session,identifier,symbol,series,market_type,change,percent_change,last_price,open,day_high,day_low,previous_close,average_price,total_traded_volume,total_traded_value,total_buy_quantity,total_sell_quantity,status,ex_date,purpose,last_update_time"
        );
    }
}
