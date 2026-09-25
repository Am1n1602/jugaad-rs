use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize};

use super::dates::deserialize_nse_date;
use super::http::{HttpClient, client_builder, with_retry};
use super::quote::{NEXTAPI_URL, deserialize_lenient_u64};
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";

/// One market segment's live open/closed status (Capital Market, Currency,
/// Commodity, Debt).
///
/// Confirmed live: Currency/Commodity/Debt's own `last`/`change` stay
/// `None` even while `status` reports `"Open"` - NSE just doesn't
/// populate them through this endpoint. Currency is the one exception in
/// practice: a separate row with `market: "currencyfuture"` (real USDINR
/// future data) shows up alongside it in the same response and comes
/// back as its own entry here - Commodity and Debt have no such
/// alternate row anywhere in this endpoint.
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

/// One F&O contract's live turnover snapshot, from either of NSE's two
/// top-20 leaderboards (`ranking`: `"value"` - ranked by premium
/// turnover, or `"volume"` - ranked by contracts traded). NSE returns
/// both leaderboards in one response; this crate flattens them into one
/// `Vec` tagged by `ranking`, matching the one-row-per-record convention
/// used elsewhere in this crate (see `BlockDealRow`).
#[derive(Debug, Serialize)]
pub struct EqDerivativeTurnoverRow {
    pub ranking: String,
    pub underlying: String,
    pub identifier: String,
    pub instrument_type: String,
    pub instrument: String,
    pub expiry: NaiveDate,
    // "Call"/"Put"/"-" - confirmed live, a different vocabulary than the
    // "CE"/"PE"/"XX" used by every other option_type field in this crate
    // (see DerivativeHistoryRow/LiveFoRow/DerivativeQuoteRow) - kept as a
    // raw string rather than forced into the shared `OptionType` enum,
    // which only models Call/Put and has no "not an option" sentinel.
    pub option_type: String,
    pub strike_price: f64,
    pub last_price: f64,
    pub percent_change: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub contracts_traded: u64,
    pub total_turnover: f64,
    pub premium_turnover: f64,
    pub open_interest: u64,
    pub underlying_value: f64,
}

#[derive(Debug, Deserialize)]
struct RawEqDerivativeTurnover {
    underlying: String,
    identifier: String,
    #[serde(rename = "instrumentType")]
    instrument_type: String,
    instrument: String,
    #[serde(rename = "expiryDate", deserialize_with = "deserialize_nse_date")]
    expiry: NaiveDate,
    #[serde(rename = "optionType")]
    option_type: String,
    #[serde(rename = "strikePrice")]
    strike_price: f64,
    #[serde(rename = "lastPrice")]
    last_price: f64,
    #[serde(rename = "pChange")]
    percent_change: f64,
    #[serde(rename = "openPrice")]
    open: f64,
    #[serde(rename = "highPrice")]
    high: f64,
    #[serde(rename = "lowPrice")]
    low: f64,
    // Confirmed live: count-like fields on NSE's derivatives endpoints
    // have repeatedly shown up as JSON floats for some contracts and
    // plain integers for others within the same response - see the
    // identical issue documented for DerivativeQuoteRow in quote.rs.
    #[serde(
        rename = "numberOfContractsTraded",
        deserialize_with = "deserialize_lenient_u64"
    )]
    contracts_traded: u64,
    #[serde(rename = "totalTurnover")]
    total_turnover: f64,
    #[serde(rename = "premiumTurnover")]
    premium_turnover: f64,
    #[serde(rename = "openInterest", deserialize_with = "deserialize_lenient_u64")]
    open_interest: u64,
    #[serde(rename = "underlyingValue")]
    underlying_value: f64,
}

impl RawEqDerivativeTurnover {
    fn into_row(self, ranking: &str) -> EqDerivativeTurnoverRow {
        EqDerivativeTurnoverRow {
            ranking: ranking.to_string(),
            underlying: self.underlying,
            identifier: self.identifier,
            instrument_type: self.instrument_type,
            instrument: self.instrument,
            expiry: self.expiry,
            option_type: self.option_type,
            strike_price: self.strike_price,
            last_price: self.last_price,
            percent_change: self.percent_change,
            open: self.open,
            high: self.high,
            low: self.low,
            contracts_traded: self.contracts_traded,
            total_turnover: self.total_turnover,
            premium_turnover: self.premium_turnover,
            open_interest: self.open_interest,
            underlying_value: self.underlying_value,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EqDerivativeTurnoverResponse {
    value: Vec<RawEqDerivativeTurnover>,
    volume: Vec<RawEqDerivativeTurnover>,
}

/// Which direction to fetch market movers for -
/// `live-analysis-variations`'s `index` query parameter. NSE spells the
/// losers value `"loosers"` on the wire (confirmed live); this crate's
/// public API uses the correct spelling and corrects it internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoverDirection {
    Gainers,
    Losers,
}

impl MoverDirection {
    fn as_query_param(self) -> &'static str {
        match self {
            MoverDirection::Gainers => "gainers",
            MoverDirection::Losers => "loosers",
        }
    }

    fn label(self) -> &'static str {
        match self {
            MoverDirection::Gainers => "gainers",
            MoverDirection::Losers => "losers",
        }
    }
}

/// One stock's row in a top-gainers/top-losers list, scoped to one of
/// NSE's seven index/security buckets (`scope`): `NIFTY`, `BANKNIFTY`,
/// `NIFTYNEXT50`, `SecGtr20` ("Securities > Rs 20"), `SecLwr20`
/// ("Securities < Rs 20"), `FOSec` ("F&O Securities"), or `allSec` ("All
/// Securities") - NSE's own bucket keys, confirmed live via this
/// endpoint's own `legends` field, kept as-is rather than renamed.
///
/// This is the endpoint NSE's own "Top Gainers/Losers" page actually
/// calls - confirmed live by inspecting that page's network requests.
/// `NSELive.top_stocks`/`getTopTenStock` (the endpoint Python's
/// `top_stocks` uses) was tried first and reliably returns only its
/// `topGainers` field; every other field it claims to have came back
/// empty across repeated live checks with the market open, while this
/// endpoint had real data for both directions at the same moment. See
/// `docs/nse-findings.md`.
///
/// NSE returns all seven buckets in one response per direction;
/// `market_movers_raw` flattens both directions and all seven buckets
/// into one `Vec` tagged by `scope`/`direction`, matching the
/// one-row-per-record convention used elsewhere in this crate.
#[derive(Debug, Serialize)]
pub struct MarketMoverRow {
    pub scope: String,
    pub direction: String,
    pub symbol: String,
    pub series: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub last_price: f64,
    pub previous_close: f64,
    pub change: f64,
    pub percent_change: f64,
    pub traded_quantity: u64,
    pub turnover: f64,
    pub market_type: String,
    // "-" on the wire when there's no upcoming corporate action -
    // confirmed live (13 of 127 rows in one sample).
    pub ca_ex_date: Option<String>,
    pub ca_purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMoverRow {
    symbol: String,
    series: String,
    #[serde(rename = "open_price")]
    open: f64,
    #[serde(rename = "high_price")]
    high: f64,
    #[serde(rename = "low_price")]
    low: f64,
    ltp: f64,
    #[serde(rename = "prev_price")]
    previous_close: f64,
    #[serde(rename = "net_price")]
    change: f64,
    // Confirmed live: not a duplicate of `net_price` (an absolute price
    // change) despite matching in many rows - they diverge on others
    // (e.g. one sample: net_price 1.54 vs perChange 0.96), so both are
    // kept. NSE also spells this one in camelCase, unlike every other
    // field on this row.
    #[serde(rename = "perChange")]
    percent_change: f64,
    #[serde(rename = "trade_quantity")]
    traded_quantity: u64,
    turnover: f64,
    #[serde(rename = "market_type")]
    market_type: String,
    #[serde(rename = "ca_ex_dt", deserialize_with = "deserialize_dash_as_none")]
    ca_ex_date: Option<String>,
    #[serde(deserialize_with = "deserialize_dash_as_none")]
    ca_purpose: Option<String>,
}

impl RawMoverRow {
    fn into_row(self, scope: &str, direction: &str) -> MarketMoverRow {
        MarketMoverRow {
            scope: scope.to_string(),
            direction: direction.to_string(),
            symbol: self.symbol,
            series: self.series,
            open: self.open,
            high: self.high,
            low: self.low,
            last_price: self.ltp,
            previous_close: self.previous_close,
            change: self.change,
            percent_change: self.percent_change,
            traded_quantity: self.traded_quantity,
            turnover: self.turnover,
            market_type: self.market_type,
            ca_ex_date: self.ca_ex_date,
            ca_purpose: self.ca_purpose,
        }
    }
}

fn deserialize_dash_as_none<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(if raw == "-" { None } else { Some(raw) })
}

#[derive(Debug, Deserialize)]
struct MoverBucket {
    data: Vec<RawMoverRow>,
}

/// NSE returns all seven buckets in one response, keyed exactly as shown
/// in the response's own `legends` field - confirmed live.
#[derive(Debug, Deserialize)]
struct MoverResponse {
    #[serde(rename = "NIFTY")]
    nifty: MoverBucket,
    #[serde(rename = "BANKNIFTY")]
    bank_nifty: MoverBucket,
    #[serde(rename = "NIFTYNEXT50")]
    nifty_next_50: MoverBucket,
    #[serde(rename = "SecGtr20")]
    sec_gtr_20: MoverBucket,
    #[serde(rename = "SecLwr20")]
    sec_lwr_20: MoverBucket,
    #[serde(rename = "FOSec")]
    fo_sec: MoverBucket,
    #[serde(rename = "allSec")]
    all_sec: MoverBucket,
}

/// One stock in NSE's "Most Active Equities" leaderboard, ranked either
/// by traded value or by traded volume.
///
/// Drops `closePrice`, confirmed live to always be `0` across every row
/// checked (both rankings) - a dead placeholder, same treatment as other
/// confirmed-always-zero/null fields elsewhere in this crate.
#[derive(Debug, Serialize)]
pub struct MostActiveEquityRow {
    pub ranking: String,
    pub symbol: String,
    pub identifier: String,
    pub last_price: f64,
    pub percent_change: f64,
    pub quantity_traded: u64,
    pub total_traded_volume: u64,
    pub total_traded_value: f64,
    pub previous_close: f64,
    // "-" on the wire when there's no upcoming corporate action -
    // confirmed live, same convention as `MarketMoverRow::ca_ex_date`.
    pub ex_date: Option<String>,
    pub purpose: Option<String>,
    pub year_high: f64,
    pub year_low: f64,
    pub change: f64,
    pub open: f64,
    pub day_high: f64,
    pub day_low: f64,
    pub last_update_time: String,
}

#[derive(Debug, Deserialize)]
struct RawMostActiveEquity {
    symbol: String,
    identifier: String,
    #[serde(rename = "lastPrice")]
    last_price: f64,
    #[serde(rename = "pChange")]
    percent_change: f64,
    #[serde(rename = "quantityTraded")]
    quantity_traded: u64,
    #[serde(rename = "totalTradedVolume")]
    total_traded_volume: u64,
    #[serde(rename = "totalTradedValue")]
    total_traded_value: f64,
    #[serde(rename = "previousClose")]
    previous_close: f64,
    #[serde(rename = "exDate", deserialize_with = "deserialize_dash_as_none")]
    ex_date: Option<String>,
    purpose: Option<String>,
    #[serde(rename = "yearHigh")]
    year_high: f64,
    #[serde(rename = "yearLow")]
    year_low: f64,
    change: f64,
    open: f64,
    #[serde(rename = "dayHigh")]
    day_high: f64,
    #[serde(rename = "dayLow")]
    day_low: f64,
    #[serde(rename = "lastUpdateTime")]
    last_update_time: String,
}

impl RawMostActiveEquity {
    fn into_row(self, ranking: &str) -> MostActiveEquityRow {
        MostActiveEquityRow {
            ranking: ranking.to_string(),
            symbol: self.symbol,
            identifier: self.identifier,
            last_price: self.last_price,
            percent_change: self.percent_change,
            quantity_traded: self.quantity_traded,
            total_traded_volume: self.total_traded_volume,
            total_traded_value: self.total_traded_value,
            previous_close: self.previous_close,
            ex_date: self.ex_date,
            purpose: self.purpose,
            year_high: self.year_high,
            year_low: self.year_low,
            change: self.change,
            open: self.open,
            day_high: self.day_high,
            day_low: self.day_low,
            last_update_time: self.last_update_time,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MostActiveEquityResponse {
    data: Vec<RawMostActiveEquity>,
}

/// One stock in NSE's "Volume Gainers" list - today's volume compared
/// against its 1-week and 2-week averages.
#[derive(Debug, Serialize, Deserialize)]
pub struct VolumeGainerRow {
    pub symbol: String,
    #[serde(rename(deserialize = "companyName"))]
    pub company_name: String,
    pub volume: u64,
    #[serde(rename(deserialize = "week1AvgVolume"))]
    pub week1_avg_volume: u64,
    #[serde(rename(deserialize = "week1volChange"))]
    pub week1_volume_change_pct: f64,
    #[serde(rename(deserialize = "week2AvgVolume"))]
    pub week2_avg_volume: u64,
    #[serde(rename(deserialize = "week2volChange"))]
    pub week2_volume_change_pct: f64,
    #[serde(rename(deserialize = "ltp"))]
    pub last_price: f64,
    #[serde(rename(deserialize = "pChange"))]
    pub percent_change: f64,
    pub turnover: f64,
}

#[derive(Debug, Deserialize)]
struct VolumeGainersResponse {
    data: Vec<VolumeGainerRow>,
}

/// One stock hitting a new 52-week high or low.
///
/// `company_name` corrects NSE's own field-name typo ("comapnyName", not
/// "companyName") - confirmed live, only this endpoint misspells it.
/// Drops the top-level `high`/`low` counts the raw response also carries:
/// both are just `data.len()`, confirmed live.
#[derive(Debug, Serialize)]
pub struct FiftyTwoWeekRow {
    pub direction: String,
    pub symbol: String,
    pub company_name: String,
    pub series: String,
    pub last_price: f64,
    pub change: f64,
    pub percent_change: f64,
    pub new_52_week_value: f64,
    pub previous_52_week_value: f64,
    pub previous_close: f64,
    // `None` for a recently-listed stock with no real previous 52-week
    // extreme yet - confirmed live, NSE sends "-" rather than omitting
    // the field or sending `null`.
    pub previous_52_week_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct RawFiftyTwoWeek {
    symbol: String,
    #[serde(rename = "comapnyName")]
    company_name: String,
    series: String,
    ltp: f64,
    change: f64,
    #[serde(rename = "pChange")]
    percent_change: f64,
    #[serde(rename = "new52WHL")]
    new_52_week_value: f64,
    #[serde(rename = "prev52WHL")]
    previous_52_week_value: f64,
    // Confirmed live: a JSON string here, unlike every numeric sibling on
    // this row, which are plain JSON numbers.
    #[serde(rename = "prevClose", deserialize_with = "deserialize_string_f64")]
    previous_close: f64,
    // "-" on the wire for a recently-listed stock with no real previous
    // 52-week extreme to compare against - confirmed live, always
    // alongside a `prev52WHL` of exactly `0` in that case.
    #[serde(rename = "prevHLDate", deserialize_with = "deserialize_nse_date_opt")]
    previous_52_week_date: Option<NaiveDate>,
}

impl RawFiftyTwoWeek {
    fn into_row(self, direction: &str) -> FiftyTwoWeekRow {
        FiftyTwoWeekRow {
            direction: direction.to_string(),
            symbol: self.symbol,
            company_name: self.company_name,
            series: self.series,
            last_price: self.ltp,
            change: self.change,
            percent_change: self.percent_change,
            new_52_week_value: self.new_52_week_value,
            previous_52_week_value: self.previous_52_week_value,
            previous_close: self.previous_close,
            previous_52_week_date: self.previous_52_week_date,
        }
    }
}

fn deserialize_string_f64<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(serde::de::Error::custom)
}

fn deserialize_nse_date_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw == "-" {
        return Ok(None);
    }
    NaiveDate::parse_from_str(&raw, "%d-%b-%Y")
        .map(Some)
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
struct FiftyTwoWeekResponse {
    data: Vec<RawFiftyTwoWeek>,
}

/// One large deal - a bulk deal, short deal, or block deal. NSE returns
/// three separate lists in one response, all with the identical row
/// shape; `large_deals_raw` flattens them into one `Vec` tagged by
/// `deal_type`, the same convention used for `BlockDealRow`/
/// `EqDerivativeTurnoverRow`/`MarketMoverRow`.
///
/// `client_name`/`buy_sell`/`weighted_avg_price` are all always `null`
/// for short deals specifically (133 of 133 rows checked) - short-sale
/// counterparty/side/price apparently isn't disclosed through this
/// endpoint, unlike bulk/block deals.
#[derive(Debug, Serialize)]
pub struct LargeDealRow {
    pub deal_type: String,
    pub symbol: String,
    pub company_name: String,
    pub client_name: Option<String>,
    pub buy_sell: Option<String>,
    pub quantity: u64,
    pub weighted_avg_price: Option<f64>,
    pub remarks: Option<String>,
    pub date: NaiveDate,
}

#[derive(Debug, Deserialize)]
struct RawLargeDeal {
    symbol: String,
    #[serde(rename = "name")]
    company_name: String,
    #[serde(rename = "clientName")]
    client_name: Option<String>,
    #[serde(rename = "buySell")]
    buy_sell: Option<String>,
    #[serde(deserialize_with = "deserialize_numeric_string_u64")]
    qty: u64,
    #[serde(
        rename = "watp",
        deserialize_with = "deserialize_numeric_string_f64_opt"
    )]
    weighted_avg_price: Option<f64>,
    // `null` or the literal `"-"` depending on the deal - both mean "no
    // remark", confirmed live.
    #[serde(deserialize_with = "deserialize_dash_or_null_as_none")]
    remarks: Option<String>,
    #[serde(deserialize_with = "deserialize_nse_date")]
    date: NaiveDate,
}

impl RawLargeDeal {
    fn into_row(self, deal_type: &str) -> LargeDealRow {
        LargeDealRow {
            deal_type: deal_type.to_string(),
            symbol: self.symbol,
            company_name: self.company_name,
            client_name: self.client_name,
            buy_sell: self.buy_sell,
            quantity: self.qty,
            weighted_avg_price: self.weighted_avg_price,
            remarks: self.remarks,
            date: self.date,
        }
    }
}

fn deserialize_numeric_string_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(serde::de::Error::custom)
}

fn deserialize_numeric_string_f64_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|s| s.parse().map_err(serde::de::Error::custom))
        .transpose()
}

fn deserialize_dash_or_null_as_none<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|s| s != "-"))
}

#[derive(Debug, Deserialize)]
struct LargeDealResponse {
    #[serde(rename = "BULK_DEALS_DATA")]
    bulk: Vec<RawLargeDeal>,
    #[serde(rename = "SHORT_DEALS_DATA")]
    short: Vec<RawLargeDeal>,
    #[serde(rename = "BLOCK_DEALS_DATA")]
    block: Vec<RawLargeDeal>,
}

/// One trading holiday for one market segment. NSE returns a separate
/// list per segment (`CM`, `FO`, `CD`, `COM`, `CBM`, `CMOT`, `EGR`,
/// `IRD`, `MF`, `NDM`, `NTRP`, `SLBS` - confirmed live); `holiday_list_raw`
/// flattens all of them into one `Vec` tagged by `segment`, the same
/// convention as `MarketMoverRow`/`LargeDealRow`.
#[derive(Debug, Serialize)]
pub struct HolidayRow {
    pub segment: String,
    pub date: NaiveDate,
    pub week_day: String,
    pub description: String,
    // "Open"/"Closed" when populated - confirmed live, `null` for most
    // segments/dates, but real values seen for `COM`/`EGR`.
    pub morning_session: Option<String>,
    pub evening_session: Option<String>,
    pub serial_number: u32,
}

#[derive(Debug, Deserialize)]
struct RawHoliday {
    #[serde(rename = "tradingDate", deserialize_with = "deserialize_nse_date")]
    date: NaiveDate,
    #[serde(rename = "weekDay")]
    week_day: String,
    description: String,
    morning_session: Option<String>,
    evening_session: Option<String>,
    #[serde(rename = "Sr_no")]
    serial_number: u32,
}

impl RawHoliday {
    fn into_row(self, segment: &str) -> HolidayRow {
        HolidayRow {
            segment: segment.to_string(),
            date: self.date,
            week_day: self.week_day,
            description: self.description,
            morning_session: self.morning_session,
            evening_session: self.evening_session,
            serial_number: self.serial_number,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NseLiveMarket {
    client: HttpClient,
}

impl NseLiveMarket {
    pub fn new() -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client: with_retry(client),
        })
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

    /// Fetches NSE's two top-20 F&O turnover leaderboards (by premium
    /// turnover value and by contracts traded), flattened into one `Vec`
    /// tagged by `ranking`. Hardcodes `index=allcontracts` - the only
    /// value confirmed live and the default Python's `eq_derivative_turnover`
    /// uses; other values aren't verified, so no parameter is exposed here
    /// rather than guessing at what else might be valid.
    pub async fn eq_derivative_turnover_raw(&self) -> Result<Vec<EqDerivativeTurnoverRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/equity-stock"))
            .query(&[("index", "allcontracts")])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: EqDerivativeTurnoverResponse = response.json().await.map_err(|e| {
            Error::Parse(format!(
                "could not parse equity derivative turnover response: {e}"
            ))
        })?;

        let mut rows: Vec<EqDerivativeTurnoverRow> = parsed
            .value
            .into_iter()
            .map(|raw| raw.into_row("value"))
            .collect();
        rows.extend(parsed.volume.into_iter().map(|raw| raw.into_row("volume")));
        Ok(rows)
    }

    /// Fetches the turnover leaderboards the same way as
    /// `eq_derivative_turnover_raw`, then writes them as a CSV file to
    /// `path` exactly - see `market_status_csv` for why this takes a full
    /// file path and always overwrites.
    pub async fn eq_derivative_turnover_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.eq_derivative_turnover_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches NSE's top-gainers/top-losers lists across all seven of its
    /// index/security buckets, flattened into one `Vec` tagged by
    /// `scope`/`direction`. Two requests under the hood (one per
    /// direction - NSE's endpoint returns all seven buckets for a single
    /// direction per call, not both directions at once).
    pub async fn market_movers_raw(&self) -> Result<Vec<MarketMoverRow>> {
        let mut rows = Vec::new();

        for direction in [MoverDirection::Gainers, MoverDirection::Losers] {
            let response = self
                .client
                .get(format!("{BASE_URL}/api/live-analysis-variations"))
                .query(&[("index", direction.as_query_param())])
                .send()
                .await?;

            match response.status() {
                StatusCode::OK => {}
                StatusCode::FORBIDDEN => return Err(Error::Blocked),
                status => return Err(Error::UnexpectedStatus(status)),
            }

            let parsed: MoverResponse = response.json().await.map_err(|e| {
                Error::Parse(format!("could not parse market movers response: {e}"))
            })?;

            let direction_label = direction.label();
            for (bucket, scope) in [
                (parsed.nifty, "NIFTY"),
                (parsed.bank_nifty, "BANKNIFTY"),
                (parsed.nifty_next_50, "NIFTYNEXT50"),
                (parsed.sec_gtr_20, "SecGtr20"),
                (parsed.sec_lwr_20, "SecLwr20"),
                (parsed.fo_sec, "FOSec"),
                (parsed.all_sec, "allSec"),
            ] {
                rows.extend(
                    bucket
                        .data
                        .into_iter()
                        .map(|raw| raw.into_row(scope, direction_label)),
                );
            }
        }

        Ok(rows)
    }

    /// Fetches market movers the same way as `market_movers_raw`, then
    /// writes them as a CSV file to `path` exactly - see `market_status_csv`
    /// for why this takes a full file path and always overwrites.
    pub async fn market_movers_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.market_movers_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches NSE's "Most Active Equities" leaderboards - top 20 by
    /// traded value and top 20 by traded volume - flattened into one
    /// `Vec` tagged by `ranking`. Two requests under the hood, one per
    /// ranking.
    pub async fn most_active_equities_raw(&self) -> Result<Vec<MostActiveEquityRow>> {
        let mut rows = Vec::new();

        for ranking in ["value", "volume"] {
            let response = self
                .client
                .get(format!(
                    "{BASE_URL}/api/live-analysis-most-active-securities"
                ))
                .query(&[("index", ranking)])
                .send()
                .await?;

            match response.status() {
                StatusCode::OK => {}
                StatusCode::FORBIDDEN => return Err(Error::Blocked),
                status => return Err(Error::UnexpectedStatus(status)),
            }

            let parsed: MostActiveEquityResponse = response.json().await.map_err(|e| {
                Error::Parse(format!(
                    "could not parse most active equities response: {e}"
                ))
            })?;

            rows.extend(parsed.data.into_iter().map(|raw| raw.into_row(ranking)));
        }

        Ok(rows)
    }

    /// Fetches the leaderboards the same way as `most_active_equities_raw`,
    /// then writes them as a CSV file to `path` exactly - see
    /// `market_status_csv` for why this takes a full file path and always
    /// overwrites.
    pub async fn most_active_equities_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.most_active_equities_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches NSE's "Volume Gainers" list - stocks trading well above
    /// their recent average volume.
    pub async fn volume_gainers_raw(&self) -> Result<Vec<VolumeGainerRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/live-analysis-volume-gainers"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: VolumeGainersResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse volume gainers response: {e}")))?;

        Ok(parsed.data)
    }

    /// Fetches volume gainers the same way as `volume_gainers_raw`, then
    /// writes them as a CSV file to `path` exactly - see
    /// `market_status_csv`.
    pub async fn volume_gainers_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.volume_gainers_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches stocks hitting a new 52-week high or low, flattened into
    /// one `Vec` tagged by `direction`. Two requests under the hood - NSE
    /// serves highs and lows from two entirely separate endpoints, not
    /// one endpoint with a direction parameter.
    pub async fn fifty_two_week_raw(&self) -> Result<Vec<FiftyTwoWeekRow>> {
        let mut rows = Vec::new();

        for (direction, path) in [
            ("high", "live-analysis-data-52weekhighstock"),
            ("low", "live-analysis-data-52weeklowstock"),
        ] {
            let response = self
                .client
                .get(format!("{BASE_URL}/api/{path}"))
                .send()
                .await?;

            match response.status() {
                StatusCode::OK => {}
                StatusCode::FORBIDDEN => return Err(Error::Blocked),
                status => return Err(Error::UnexpectedStatus(status)),
            }

            let parsed: FiftyTwoWeekResponse = response.json().await.map_err(|e| {
                Error::Parse(format!("could not parse 52-week {direction} response: {e}"))
            })?;

            rows.extend(parsed.data.into_iter().map(|raw| raw.into_row(direction)));
        }

        Ok(rows)
    }

    /// Fetches 52-week highs/lows the same way as `fifty_two_week_raw`,
    /// then writes them as a CSV file to `path` exactly - see
    /// `market_status_csv`.
    pub async fn fifty_two_week_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.fifty_two_week_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches today's large deals - bulk, short, and block deals -
    /// flattened into one `Vec` tagged by `deal_type` (`"bulk"`,
    /// `"short"`, `"block"`). A different, simpler view of deals than
    /// `block_deal_session_raw`/`NseArchives::bulk_deals_raw` (client
    /// identities and buy/sell side instead of live OHLC-style pricing),
    /// and the only source in this crate for short deals specifically.
    pub async fn large_deals_raw(&self) -> Result<Vec<LargeDealRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/snapshot-capital-market-largedeal"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: LargeDealResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse large deals response: {e}")))?;

        let mut rows: Vec<LargeDealRow> = parsed
            .bulk
            .into_iter()
            .map(|raw| raw.into_row("bulk"))
            .collect();
        rows.extend(parsed.short.into_iter().map(|raw| raw.into_row("short")));
        rows.extend(parsed.block.into_iter().map(|raw| raw.into_row("block")));
        Ok(rows)
    }

    /// Fetches large deals the same way as `large_deals_raw`, then writes
    /// them as a CSV file to `path` exactly - see `market_status_csv`.
    pub async fn large_deals_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.large_deals_raw().await?;
        write_csv(&rows, path)
    }

    /// Fetches NSE's trading holiday calendar across every market segment
    /// it publishes one for, flattened into one `Vec` tagged by `segment`.
    pub async fn holiday_list_raw(&self) -> Result<Vec<HolidayRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/holiday-master"))
            .query(&[("type", "trading")])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: std::collections::HashMap<String, Vec<RawHoliday>> = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse holiday list response: {e}")))?;

        Ok(parsed
            .into_iter()
            .flat_map(|(segment, holidays)| {
                holidays.into_iter().map(move |raw| raw.into_row(&segment))
            })
            .collect())
    }

    /// Fetches the holiday calendar the same way as `holiday_list_raw`,
    /// then writes it as a CSV file to `path` exactly - see
    /// `market_status_csv`.
    pub async fn holiday_list_csv(&self, path: &Path) -> Result<PathBuf> {
        let rows = self.holiday_list_raw().await?;
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

/// Shared by every `download_*_raw` method that fetches an arbitrary file
/// by URL (XBRL/HTML filings in `corporate_results.rs`, PDF attachments
/// in `corporate_announcements.rs`): downloads whatever bytes are at
/// `url`, unchanged - the caller decides what format to expect.
pub(super) async fn download_bytes(client: &HttpClient, url: &str) -> Result<Vec<u8>> {
    let response = client.get(url).send().await?;

    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Err(Error::NotFound(format!("no file at '{url}'"))),
        StatusCode::FORBIDDEN => return Err(Error::Blocked),
        status => return Err(Error::UnexpectedStatus(status)),
    }

    Ok(response.bytes().await?.to_vec())
}

/// The last path segment of a URL, used as a filename for every
/// `download_*_save` method that saves a file under NSE's own name for
/// it rather than building one from the caller's own arguments.
pub(super) fn filename_from_url(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
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
    const SAMPLE_MARKET_STATUS: &str =
        include_str!("../../tests/fixtures/live/sample_market_status.json");

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
    const SAMPLE_INDEX_SNAPSHOT: &str =
        include_str!("../../tests/fixtures/live/sample_index_snapshot.json");

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
    const SAMPLE_INDEX_SNAPSHOT_NO_PE_NO_BREADTH: &str =
        include_str!("../../tests/fixtures/live/sample_index_snapshot_no_pe_no_breadth.json");

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
    const SAMPLE_MARKET_TURNOVER: &str =
        include_str!("../../tests/fixtures/live/sample_market_turnover.json");

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
    const SAMPLE_LIVE_FO: &str = include_str!("../../tests/fixtures/live/sample_live_fo.json");

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
    const SAMPLE_BLOCK_DEAL_SESSION: &str =
        include_str!("../../tests/fixtures/live/sample_block_deal_session.json");

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

    // Real response captured from NSE's equity-stock API
    // (index=allcontracts), trimmed to one row per leaderboard. Note
    // "Call"/"Put" (not "CE"/"PE") for option_type.
    const SAMPLE_EQ_DERIVATIVE_TURNOVER: &str =
        include_str!("../../tests/fixtures/live/sample_eq_derivative_turnover.json");

    #[test]
    fn deserializes_real_eq_derivative_turnover_shape() {
        let parsed: EqDerivativeTurnoverResponse =
            serde_json::from_str(SAMPLE_EQ_DERIVATIVE_TURNOVER).unwrap();

        assert_eq!(parsed.value.len(), 1);
        assert_eq!(parsed.value[0].option_type, "Call");
        assert_eq!(parsed.value[0].expiry, date(2026, 9, 22));
        assert_eq!(parsed.volume[0].option_type, "-");
    }

    #[test]
    fn eq_derivative_turnover_rows_are_tagged_with_their_ranking() {
        let parsed: EqDerivativeTurnoverResponse =
            serde_json::from_str(SAMPLE_EQ_DERIVATIVE_TURNOVER).unwrap();

        let mut rows: Vec<EqDerivativeTurnoverRow> = parsed
            .value
            .into_iter()
            .map(|raw| raw.into_row("value"))
            .collect();
        rows.extend(parsed.volume.into_iter().map(|raw| raw.into_row("volume")));

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ranking, "value");
        assert_eq!(rows[0].underlying, "NIFTY");
        assert_eq!(rows[1].ranking, "volume");
        assert_eq!(rows[1].underlying, "HDFCBANK");
    }

    // Confirmed live: contractsTraded/openInterest can be JSON floats for
    // some contracts within the same response - same issue as
    // DerivativeQuoteRow in quote.rs.
    #[test]
    fn eq_derivative_turnover_accepts_float_formatted_counts() {
        const FLOAT_COUNTS: &str = include_str!("../../tests/fixtures/live/float_counts.json");
        let parsed: EqDerivativeTurnoverResponse = serde_json::from_str(FLOAT_COUNTS).unwrap();

        assert_eq!(parsed.value[0].contracts_traded, 5_628_311);
        assert_eq!(parsed.value[0].open_interest, 114_149);
    }

    #[test]
    fn eq_derivative_turnover_serializing_uses_clean_field_names() {
        let parsed: EqDerivativeTurnoverResponse =
            serde_json::from_str(SAMPLE_EQ_DERIVATIVE_TURNOVER).unwrap();
        let row = parsed.value.into_iter().next().unwrap().into_row("value");

        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&row).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "ranking,underlying,identifier,instrument_type,instrument,expiry,option_type,strike_price,last_price,percent_change,open,high,low,contracts_traded,total_turnover,premium_turnover,open_interest,underlying_value"
        );
    }

    // Real response shape captured live from
    // `live-analysis-variations?index=gainers`, trimmed to two of the
    // seven buckets and one row each. `NIFTYNEXT50`/`SecGtr20`/`SecLwr20`/
    // `FOSec`/`allSec` are structurally identical to `NIFTY`/`BANKNIFTY`
    // shown here.
    const SAMPLE_MARKET_MOVERS: &str =
        include_str!("../../tests/fixtures/live/sample_market_movers.json");

    #[test]
    fn deserializes_real_market_movers_shape() {
        let parsed: MoverResponse = serde_json::from_str(SAMPLE_MARKET_MOVERS).unwrap();

        let nifty_row = parsed
            .nifty
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_row("NIFTY", "gainers");
        assert_eq!(nifty_row.symbol, "HDFCLIFE");
        assert_eq!(nifty_row.last_price, 562.0);
        assert_eq!(nifty_row.change, 2.01);
        assert_eq!(nifty_row.percent_change, 2.01);
        assert_eq!(nifty_row.ca_ex_date, Some("19-Jun-2026".to_string()));
        assert_eq!(
            nifty_row.ca_purpose,
            Some("Dividend - Rs 2.10 Per Share".to_string())
        );

        let bank_row = parsed
            .bank_nifty
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_row("BANKNIFTY", "gainers");
        assert_eq!(bank_row.ca_ex_date, None);
        assert_eq!(bank_row.ca_purpose, None);
    }

    // Confirmed live: `net_price` and `perChange` diverge on some rows
    // (e.g. NIFTYNEXT50's BAJAJHLDNG: net_price 1.54 vs perChange 0.96), so
    // both fields are kept rather than treated as a duplicate.
    #[test]
    fn market_mover_change_and_percent_change_are_not_always_equal() {
        const DIVERGENT: &str = include_str!("../../tests/fixtures/live/divergent.json");
        let bucket: MoverBucket = serde_json::from_str(DIVERGENT).unwrap();
        let row = bucket
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_row("NIFTYNEXT50", "gainers");

        assert_eq!(row.change, 1.54);
        assert_eq!(row.percent_change, 0.96);
    }

    // Real response shape captured live from
    // `live-analysis-most-active-securities?index=value`.
    const SAMPLE_MOST_ACTIVE: &str =
        include_str!("../../tests/fixtures/live/sample_most_active.json");

    #[test]
    fn deserializes_real_most_active_equity_shape() {
        let parsed: MostActiveEquityResponse = serde_json::from_str(SAMPLE_MOST_ACTIVE).unwrap();
        let row = parsed.data.into_iter().next().unwrap().into_row("value");

        assert_eq!(row.symbol, "SSRETAIL");
        assert_eq!(row.ex_date, None);
        assert_eq!(row.purpose, None);
        assert_eq!(row.ranking, "value");
    }

    // Real response shape captured live from `live-analysis-volume-gainers`.
    const SAMPLE_VOLUME_GAINER: &str =
        include_str!("../../tests/fixtures/live/sample_volume_gainer.json");

    #[test]
    fn deserializes_real_volume_gainer_shape() {
        let parsed: VolumeGainersResponse = serde_json::from_str(SAMPLE_VOLUME_GAINER).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.symbol, "LOYALTEX");
        assert_eq!(row.volume, 43_719);
        assert_eq!(row.week1_avg_volume, 138);
    }

    // Real response shape captured live from
    // `live-analysis-data-52weekhighstock` - note "comapnyName" (NSE's own
    // typo) and `prevClose` sent as a string unlike its numeric siblings.
    const SAMPLE_52W_HIGH: &str = include_str!("../../tests/fixtures/live/sample_52w_high.json");

    #[test]
    fn deserializes_real_52_week_high_shape() {
        let parsed: FiftyTwoWeekResponse = serde_json::from_str(SAMPLE_52W_HIGH).unwrap();
        let row = parsed.data.into_iter().next().unwrap().into_row("high");

        assert_eq!(row.symbol, "AARTIPHARM");
        assert_eq!(row.company_name, "Aarti Pharmalabs Limited");
        assert_eq!(row.previous_close, 883.75);
        assert_eq!(row.previous_52_week_date, Some(date(2026, 8, 11)));
        assert_eq!(row.direction, "high");
    }

    // Real response shape captured live for a recently-listed stock with
    // no real previous 52-week extreme - `prevHLDate` sent as the literal
    // string "-" instead of a date, alongside a `prev52WHL` of exactly 0.
    // This exact response broke deserialization the first time this was
    // tested live (the field was originally modeled as a required date).
    const SAMPLE_52W_NEWLY_LISTED: &str =
        include_str!("../../tests/fixtures/live/sample_52w_newly_listed.json");

    #[test]
    fn fifty_two_week_treats_dash_previous_date_as_none() {
        let parsed: FiftyTwoWeekResponse = serde_json::from_str(SAMPLE_52W_NEWLY_LISTED).unwrap();
        let row = parsed.data.into_iter().next().unwrap().into_row("low");

        assert_eq!(row.previous_52_week_date, None);
        assert_eq!(row.previous_52_week_value, 0.0);
    }

    // Full real 52-week high/low responses, captured live specifically
    // because the small hand-picked samples above only demonstrate the
    // "-" placeholder date bug on one row each. Deserializing every row
    // in both full responses is the actual regression guard - it also
    // covers whatever real variability the two single-row samples don't
    // happen to contain.
    const LARGE_52W_HIGH: &str = include_str!("../../tests/fixtures/live/large_52w_high.json");
    const LARGE_52W_LOW: &str = include_str!("../../tests/fixtures/live/large_52w_low.json");

    #[test]
    fn deserializes_full_real_52_week_high_and_low_responses() {
        let high: FiftyTwoWeekResponse = serde_json::from_str(LARGE_52W_HIGH).unwrap();
        let low: FiftyTwoWeekResponse = serde_json::from_str(LARGE_52W_LOW).unwrap();

        assert_eq!(high.data.len(), 93);
        assert_eq!(low.data.len(), 97);

        // Both full responses are known (confirmed at fixture-capture time)
        // to contain rows with the "-" placeholder date - the exact
        // real-world condition `fifty_two_week_treats_dash_previous_date_as_none`
        // guards against on a single row, now checked across the whole set.
        let high_dashes = high
            .data
            .into_iter()
            .map(|r| r.into_row("high"))
            .filter(|r| r.previous_52_week_date.is_none())
            .count();
        let low_dashes = low
            .data
            .into_iter()
            .map(|r| r.into_row("low"))
            .filter(|r| r.previous_52_week_date.is_none())
            .count();
        assert_eq!(high_dashes, 3);
        assert_eq!(low_dashes, 3);
    }

    // Real response shape captured live from
    // `snapshot-capital-market-largedeal` - one row from each of the three
    // lists, including a short deal (buySell/clientName/remarks/watp all
    // null - confirmed live to always be null for short deals).
    const SAMPLE_LARGE_DEAL: &str =
        include_str!("../../tests/fixtures/live/sample_large_deal.json");

    #[test]
    fn deserializes_real_large_deal_shape_and_flattens_all_three_lists() {
        let parsed: LargeDealResponse = serde_json::from_str(SAMPLE_LARGE_DEAL).unwrap();

        let bulk = &parsed.bulk[0];
        assert_eq!(bulk.remarks, None); // "-" normalized to None
        assert_eq!(bulk.weighted_avg_price, Some(4.62));
        assert_eq!(bulk.qty, 2_277_241);

        let short = &parsed.short[0];
        assert_eq!(short.buy_sell, None);
        assert_eq!(short.client_name, None);
        assert_eq!(short.weighted_avg_price, None);
        assert_eq!(short.qty, 5);

        let block = &parsed.block[0];
        assert_eq!(block.weighted_avg_price, Some(360.0));

        let rows: Vec<LargeDealRow> = parsed
            .bulk
            .into_iter()
            .map(|r| r.into_row("bulk"))
            .chain(parsed.short.into_iter().map(|r| r.into_row("short")))
            .chain(parsed.block.into_iter().map(|r| r.into_row("block")))
            .collect();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|r| r.deal_type == "bulk"));
        assert!(rows.iter().any(|r| r.deal_type == "short"));
        assert!(rows.iter().any(|r| r.deal_type == "block"));
    }

    // Real response shape captured live from `holiday-master?type=trading` -
    // trimmed to two segments, one holiday each (one with real session
    // values, one with the more common null/null).
    const SAMPLE_HOLIDAYS: &str = include_str!("../../tests/fixtures/live/sample_holidays.json");

    #[test]
    fn deserializes_real_holiday_list_shape_and_flattens_segments() {
        let parsed: std::collections::HashMap<String, Vec<RawHoliday>> =
            serde_json::from_str(SAMPLE_HOLIDAYS).unwrap();
        let rows: Vec<HolidayRow> = parsed
            .into_iter()
            .flat_map(|(segment, holidays)| {
                holidays.into_iter().map(move |raw| raw.into_row(&segment))
            })
            .collect();

        assert_eq!(rows.len(), 2);
        let cm = rows.iter().find(|r| r.segment == "CM").unwrap();
        assert_eq!(cm.date, date(2026, 1, 26));
        assert_eq!(cm.morning_session, None);

        let com = rows.iter().find(|r| r.segment == "COM").unwrap();
        assert_eq!(com.morning_session, Some("Open".to_string()));
        assert_eq!(com.evening_session, Some("Closed".to_string()));
    }
}
