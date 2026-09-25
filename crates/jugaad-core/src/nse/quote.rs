//! Per-symbol/per-contract live data from nseindia.com: a stock's live
//! quote (with order book depth), a symbol's full F&O contract list, a
//! single index's live value, and option chains (index/equity/currency).

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::dates::deserialize_nse_date;
use super::http::{HttpClient, client_builder, with_retry};
use super::live::write_csv;
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";
// Also used by `NseLiveMarket::block_deal_session_raw` in live.rs - both
// hit the same generic NextApi endpoint, just with different
// `functionName` values.
pub(super) const NEXTAPI_URL: &str = "https://www.nseindia.com/api/NextApi/apiClient/GetQuoteApi";
// A sibling NextApi endpoint, one path segment shorter than
// `NEXTAPI_URL` (no `/GetQuoteApi` suffix) - confirmed live, used only
// by index chart data.
const NEXTAPI_APICLIENT_URL: &str = "https://www.nseindia.com/api/NextApi/apiClient";

/// One price level of an order book's bid/ask depth.
#[derive(Debug, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub buy_price: f64,
    pub buy_quantity: u64,
    pub sell_price: f64,
    pub sell_quantity: u64,
}

/// A stock's order book: 5 levels of bid/ask depth, best price first.
#[derive(Debug, Serialize)]
pub struct OrderBook {
    pub levels: [OrderBookLevel; 5],
    pub total_buy_quantity: u64,
    pub total_sell_quantity: u64,
}

/// A stock's live quote: price/change, day and 52-week range, traded
/// volume/value/delivery, and order book depth.
///
/// NSE's real response nests this across 5 sub-objects with roughly 90
/// fields total, most of it debt-security/compliance data that's null for
/// ordinary equities (the same endpoint shape is reused for equities,
/// ETFs and debt securities alike). This keeps the fields that make sense
/// for a live equity quote and drops the rest - see `RawEquityResponse`.
#[derive(Debug, Serialize)]
pub struct StockQuote {
    pub symbol: String,
    pub company_name: String,
    pub series: String,
    pub open: f64,
    pub day_high: f64,
    pub day_low: f64,
    pub previous_close: f64,
    pub last_price: f64,
    pub change: f64,
    pub percent_change: f64,
    pub year_high: f64,
    pub year_low: f64,
    pub total_traded_volume: u64,
    pub total_traded_value: f64,
    // Null for some series (e.g. freshly listed or non-equity instruments).
    pub total_market_cap: Option<f64>,
    pub face_value: f64,
    pub delivery_quantity: Option<u64>,
    pub delivery_pct: Option<f64>,
    pub order_book: OrderBook,
    pub last_update_time: String,
}

#[derive(Debug, Deserialize)]
struct RawOrderBook {
    #[serde(rename = "buyPrice1")]
    buy_price_1: f64,
    #[serde(rename = "buyQuantity1")]
    buy_quantity_1: u64,
    #[serde(rename = "buyPrice2")]
    buy_price_2: f64,
    #[serde(rename = "buyQuantity2")]
    buy_quantity_2: u64,
    #[serde(rename = "buyPrice3")]
    buy_price_3: f64,
    #[serde(rename = "buyQuantity3")]
    buy_quantity_3: u64,
    #[serde(rename = "buyPrice4")]
    buy_price_4: f64,
    #[serde(rename = "buyQuantity4")]
    buy_quantity_4: u64,
    #[serde(rename = "buyPrice5")]
    buy_price_5: f64,
    #[serde(rename = "buyQuantity5")]
    buy_quantity_5: u64,
    #[serde(rename = "sellPrice1")]
    sell_price_1: f64,
    #[serde(rename = "sellQuantity1")]
    sell_quantity_1: u64,
    #[serde(rename = "sellPrice2")]
    sell_price_2: f64,
    #[serde(rename = "sellQuantity2")]
    sell_quantity_2: u64,
    #[serde(rename = "sellPrice3")]
    sell_price_3: f64,
    #[serde(rename = "sellQuantity3")]
    sell_quantity_3: u64,
    #[serde(rename = "sellPrice4")]
    sell_price_4: f64,
    #[serde(rename = "sellQuantity4")]
    sell_quantity_4: u64,
    #[serde(rename = "sellPrice5")]
    sell_price_5: f64,
    #[serde(rename = "sellQuantity5")]
    sell_quantity_5: u64,
    #[serde(rename = "totalBuyQuantity")]
    total_buy_quantity: u64,
    #[serde(rename = "totalSellQuantity")]
    total_sell_quantity: u64,
}

impl From<RawOrderBook> for OrderBook {
    fn from(r: RawOrderBook) -> Self {
        OrderBook {
            levels: [
                OrderBookLevel {
                    buy_price: r.buy_price_1,
                    buy_quantity: r.buy_quantity_1,
                    sell_price: r.sell_price_1,
                    sell_quantity: r.sell_quantity_1,
                },
                OrderBookLevel {
                    buy_price: r.buy_price_2,
                    buy_quantity: r.buy_quantity_2,
                    sell_price: r.sell_price_2,
                    sell_quantity: r.sell_quantity_2,
                },
                OrderBookLevel {
                    buy_price: r.buy_price_3,
                    buy_quantity: r.buy_quantity_3,
                    sell_price: r.sell_price_3,
                    sell_quantity: r.sell_quantity_3,
                },
                OrderBookLevel {
                    buy_price: r.buy_price_4,
                    buy_quantity: r.buy_quantity_4,
                    sell_price: r.sell_price_4,
                    sell_quantity: r.sell_quantity_4,
                },
                OrderBookLevel {
                    buy_price: r.buy_price_5,
                    buy_quantity: r.buy_quantity_5,
                    sell_price: r.sell_price_5,
                    sell_quantity: r.sell_quantity_5,
                },
            ],
            total_buy_quantity: r.total_buy_quantity,
            total_sell_quantity: r.total_sell_quantity,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawMetaData {
    symbol: String,
    #[serde(rename = "companyName")]
    company_name: String,
    series: String,
    open: f64,
    #[serde(rename = "dayHigh")]
    day_high: f64,
    #[serde(rename = "dayLow")]
    day_low: f64,
    #[serde(rename = "previousClose")]
    previous_close: f64,
    change: f64,
    #[serde(rename = "pChange")]
    percent_change: f64,
}

#[derive(Debug, Deserialize)]
struct RawTradeInfo {
    #[serde(rename = "totalTradedVolume")]
    total_traded_volume: u64,
    #[serde(rename = "totalTradedValue")]
    total_traded_value: f64,
    #[serde(rename = "totalMarketCap")]
    total_market_cap: Option<f64>,
    #[serde(rename = "faceValue")]
    face_value: f64,
    #[serde(rename = "deliveryquantity")]
    delivery_quantity: Option<u64>,
    #[serde(rename = "deliveryToTradedQuantity")]
    delivery_pct: Option<f64>,
    #[serde(rename = "lastPrice")]
    last_price: f64,
}

#[derive(Debug, Deserialize)]
struct RawPriceInfo {
    #[serde(rename = "yearHigh")]
    year_high: f64,
    #[serde(rename = "yearLow")]
    year_low: f64,
}

/// The raw shape NSE actually sends: everything nested under one entry of
/// `equityResponse`. Kept private and mapped into the flat, curated public
/// `StockQuote` via `From` below - see `StockQuote`'s docs for why.
#[derive(Debug, Deserialize)]
struct RawEquityResponse {
    #[serde(rename = "orderBook")]
    order_book: RawOrderBook,
    #[serde(rename = "metaData")]
    meta_data: RawMetaData,
    #[serde(rename = "tradeInfo")]
    trade_info: RawTradeInfo,
    #[serde(rename = "priceInfo")]
    price_info: RawPriceInfo,
    #[serde(rename = "lastUpdateTime")]
    last_update_time: String,
}

impl From<RawEquityResponse> for StockQuote {
    fn from(r: RawEquityResponse) -> Self {
        StockQuote {
            symbol: r.meta_data.symbol,
            company_name: r.meta_data.company_name,
            series: r.meta_data.series,
            open: r.meta_data.open,
            day_high: r.meta_data.day_high,
            day_low: r.meta_data.day_low,
            previous_close: r.meta_data.previous_close,
            last_price: r.trade_info.last_price,
            change: r.meta_data.change,
            percent_change: r.meta_data.percent_change,
            year_high: r.price_info.year_high,
            year_low: r.price_info.year_low,
            total_traded_volume: r.trade_info.total_traded_volume,
            total_traded_value: r.trade_info.total_traded_value,
            total_market_cap: r.trade_info.total_market_cap,
            face_value: r.trade_info.face_value,
            delivery_quantity: r.trade_info.delivery_quantity,
            delivery_pct: r.trade_info.delivery_pct,
            order_book: r.order_book.into(),
            last_update_time: r.last_update_time,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StockQuoteResponse {
    #[serde(rename = "equityResponse")]
    equity_response: Vec<RawEquityResponse>,
}

/// `StockQuote` flattened into one CSV row: the 5-level order book has no
/// natural CSV representation as a nested array, so each level gets its
/// own numbered columns instead (`buy_price_1`, `buy_price_2`, ...).
#[derive(Debug, Serialize)]
struct StockQuoteCsvRow<'a> {
    symbol: &'a str,
    company_name: &'a str,
    series: &'a str,
    open: f64,
    day_high: f64,
    day_low: f64,
    previous_close: f64,
    last_price: f64,
    change: f64,
    percent_change: f64,
    year_high: f64,
    year_low: f64,
    total_traded_volume: u64,
    total_traded_value: f64,
    total_market_cap: Option<f64>,
    face_value: f64,
    delivery_quantity: Option<u64>,
    delivery_pct: Option<f64>,
    buy_price_1: f64,
    buy_quantity_1: u64,
    sell_price_1: f64,
    sell_quantity_1: u64,
    buy_price_2: f64,
    buy_quantity_2: u64,
    sell_price_2: f64,
    sell_quantity_2: u64,
    buy_price_3: f64,
    buy_quantity_3: u64,
    sell_price_3: f64,
    sell_quantity_3: u64,
    buy_price_4: f64,
    buy_quantity_4: u64,
    sell_price_4: f64,
    sell_quantity_4: u64,
    buy_price_5: f64,
    buy_quantity_5: u64,
    sell_price_5: f64,
    sell_quantity_5: u64,
    total_buy_quantity: u64,
    total_sell_quantity: u64,
    last_update_time: &'a str,
}

impl<'a> From<&'a StockQuote> for StockQuoteCsvRow<'a> {
    fn from(q: &'a StockQuote) -> Self {
        let levels = &q.order_book.levels;
        StockQuoteCsvRow {
            symbol: &q.symbol,
            company_name: &q.company_name,
            series: &q.series,
            open: q.open,
            day_high: q.day_high,
            day_low: q.day_low,
            previous_close: q.previous_close,
            last_price: q.last_price,
            change: q.change,
            percent_change: q.percent_change,
            year_high: q.year_high,
            year_low: q.year_low,
            total_traded_volume: q.total_traded_volume,
            total_traded_value: q.total_traded_value,
            total_market_cap: q.total_market_cap,
            face_value: q.face_value,
            delivery_quantity: q.delivery_quantity,
            delivery_pct: q.delivery_pct,
            buy_price_1: levels[0].buy_price,
            buy_quantity_1: levels[0].buy_quantity,
            sell_price_1: levels[0].sell_price,
            sell_quantity_1: levels[0].sell_quantity,
            buy_price_2: levels[1].buy_price,
            buy_quantity_2: levels[1].buy_quantity,
            sell_price_2: levels[1].sell_price,
            sell_quantity_2: levels[1].sell_quantity,
            buy_price_3: levels[2].buy_price,
            buy_quantity_3: levels[2].buy_quantity,
            sell_price_3: levels[2].sell_price,
            sell_quantity_3: levels[2].sell_quantity,
            buy_price_4: levels[3].buy_price,
            buy_quantity_4: levels[3].buy_quantity,
            sell_price_4: levels[3].sell_price,
            sell_quantity_4: levels[3].sell_quantity,
            buy_price_5: levels[4].buy_price,
            buy_quantity_5: levels[4].buy_quantity,
            sell_price_5: levels[4].sell_price,
            sell_quantity_5: levels[4].sell_quantity,
            total_buy_quantity: q.order_book.total_buy_quantity,
            total_sell_quantity: q.order_book.total_sell_quantity,
            last_update_time: &q.last_update_time,
        }
    }
}

/// One F&O contract's live price/open-interest snapshot, as returned for
/// every contract (all expiries, all strikes, futures and options alike)
/// of a single underlying symbol - see `NseQuote::derivative_quote_raw`.
///
/// Same futures-use-sentinel-values shape as `DerivativeHistoryRow`/
/// `LiveFoRow` elsewhere in this crate (`option_type` is `"XX"` and
/// `strike_price` is `0` for futures rows).
#[derive(Debug, Serialize, Deserialize)]
pub struct DerivativeQuoteRow {
    pub underlying: String,
    pub identifier: String,
    #[serde(rename(deserialize = "instrumentType"))]
    pub instrument_type: String,
    #[serde(
        rename(deserialize = "expiryDate"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub expiry: NaiveDate,
    #[serde(rename(deserialize = "optionType"))]
    pub option_type: String,
    // Confirmed live: NSE pads this as a string (e.g. `"   23300.00"`),
    // unlike every other strike-price field in this crate, which are
    // plain JSON numbers.
    #[serde(
        rename(deserialize = "strikePrice"),
        deserialize_with = "deserialize_trimmed_f64"
    )]
    pub strike_price: f64,
    #[serde(rename(deserialize = "lastPrice"))]
    pub last_price: f64,
    pub change: f64,
    #[serde(rename(deserialize = "pchange"))]
    pub percent_change: f64,
    #[serde(rename(deserialize = "openPrice"))]
    pub open: f64,
    #[serde(rename(deserialize = "highPrice"))]
    pub high: f64,
    #[serde(rename(deserialize = "lowPrice"))]
    pub low: f64,
    #[serde(rename(deserialize = "prevClose"))]
    pub prev_close: f64,
    #[serde(rename(deserialize = "closePrice"))]
    pub close_price: f64,
    #[serde(rename(deserialize = "totalTradedVolume"))]
    pub volume: u64,
    #[serde(rename(deserialize = "totalTurnover"))]
    pub turnover: f64,
    #[serde(rename(deserialize = "underlyingValue"))]
    pub underlying_value: f64,
    // Confirmed live: NSE formats this as a JSON float (e.g. `90670.0`)
    // for some contracts and a plain integer for others - a strict `u64`
    // fails to deserialize the float form, so this goes through `f64`
    // first and rounds.
    #[serde(
        rename(deserialize = "openInterest"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub open_interest: u64,
    // Same float-or-integer inconsistency as `open_interest`.
    #[serde(
        rename(deserialize = "changeinOpenInterest"),
        deserialize_with = "deserialize_lenient_i64"
    )]
    pub change_in_open_interest: i64,
    #[serde(rename(deserialize = "pchangeinOpenInterest"))]
    pub percent_change_in_open_interest: f64,
}

#[derive(Debug, Deserialize)]
struct DerivativeQuoteResponse {
    data: Vec<DerivativeQuoteRow>,
}

fn deserialize_trimmed_f64<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.trim().parse().map_err(serde::de::Error::custom)
}

// Also used by `NseLiveMarket::eq_derivative_turnover_raw` in live.rs -
// count-like fields on NSE's derivatives endpoints have repeatedly shown
// up as JSON floats for some contracts and plain integers for others
// within the same response.
pub(super) fn deserialize_lenient_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(f64::deserialize(deserializer)?.round() as u64)
}

fn deserialize_lenient_i64<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(f64::deserialize(deserializer)?.round() as i64)
}

/// Which time window to fetch a stock's price chart for.
///
/// Confirmed live: NSE's `getSymbolChartData` NextApi function only
/// accepts these five values - the other period buttons NSE's own chart
/// shows (`"3M"`, `"6M"`, `"3Y"`, `"ALL"`) make the endpoint return a 500
/// Java `NullPointerException` instead of data or a clean error, so this
/// stays an enum rather than a free-form string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartPeriod {
    OneDay,
    OneWeek,
    OneMonth,
    OneYear,
    FiveYears,
}

impl ChartPeriod {
    fn as_query_param(self) -> &'static str {
        match self {
            ChartPeriod::OneDay => "1D",
            ChartPeriod::OneWeek => "1W",
            ChartPeriod::OneMonth => "1M",
            ChartPeriod::OneYear => "1Y",
            ChartPeriod::FiveYears => "5Y",
        }
    }

    // Used to build a CSV filename - lowercase, matching this crate's other
    // filename conventions rather than the API's own uppercase query value.
    fn label(self) -> &'static str {
        match self {
            ChartPeriod::OneDay => "1d",
            ChartPeriod::OneWeek => "1w",
            ChartPeriod::OneMonth => "1m",
            ChartPeriod::OneYear => "1y",
            ChartPeriod::FiveYears => "5y",
        }
    }
}

/// One point on a stock's price chart.
///
/// `session`/`change`/`percent_change` are only meaningfully populated for
/// `ChartPeriod::OneDay` - confirmed live, the weekly/monthly/yearly
/// windows always report `session: "NM"` with `change`/`percent_change`
/// as `None`.
#[derive(Debug, Serialize)]
pub struct ChartDataPoint {
    // IST wall-clock time, not UTC - see `ist_timestamp_from_millis`.
    pub timestamp: NaiveDateTime,
    pub price: f64,
    pub session: String,
    pub change: Option<f64>,
    pub percent_change: Option<f64>,
}

/// A stock's intraday or historical price chart -
/// `NseQuote::stock_chart_data_raw`.
#[derive(Debug, Serialize)]
pub struct ChartData {
    pub identifier: String,
    pub name: String,
    pub close_price: f64,
    pub points: Vec<ChartDataPoint>,
}

/// One `grapthData` point, deserialized straight from its 5-element JSON
/// array: `(timestamp_millis, price, session, change, percent_change)`.
type RawChartDataPoint = (i64, f64, String, Option<String>, Option<String>);

/// The raw shape NSE actually sends. `grapthData` is NSE's own misspelling
/// of "graphData", confirmed live - kept as-is here since it's just the
/// wire name, not the public field name.
#[derive(Debug, Deserialize)]
struct RawChartData {
    identifier: String,
    name: String,
    #[serde(rename = "grapthData")]
    graph_data: Vec<RawChartDataPoint>,
    #[serde(rename = "closePrice")]
    close_price: f64,
}

// Confirmed live by comparing a freshly-fetched point's timestamp against
// `stock_quote_raw`'s `last_update_time` (genuine IST) at the same moment:
// decoding the epoch as a real UTC instant read ~5:30 ahead of the actual
// UTC clock - NSE built the epoch from IST wall-clock digits as if they
// were UTC, the same root cause as the `CH_TIMESTAMP` bug documented in
// `dates.rs`. This reads the UTC digits back out as the IST wall-clock
// value they actually represent, rather than trusting the instant.
fn ist_timestamp_from_millis(millis: i64) -> Option<NaiveDateTime> {
    Some(DateTime::from_timestamp_millis(millis)?.naive_utc())
}

fn chart_data_point_from_tuple(row: RawChartDataPoint) -> Result<ChartDataPoint> {
    let (millis, price, session, change, percent_change) = row;
    let timestamp = ist_timestamp_from_millis(millis)
        .ok_or_else(|| Error::Parse(format!("invalid chart data timestamp: {millis}")))?;
    let change = change
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|e| Error::Parse(format!("could not parse chart data change: {e}")))?;
    let percent_change = percent_change
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|e| Error::Parse(format!("could not parse chart data percent_change: {e}")))?;
    Ok(ChartDataPoint {
        timestamp,
        price,
        session,
        change,
        percent_change,
    })
}

/// One `ChartDataPoint` flattened for CSV, with the chart's `identifier`/
/// `name`/`close_price` repeated on every row since there's no natural
/// single-row summary alongside a time series.
#[derive(Debug, Serialize)]
struct ChartDataCsvRow<'a> {
    identifier: &'a str,
    name: &'a str,
    close_price: f64,
    timestamp: NaiveDateTime,
    price: f64,
    session: &'a str,
    change: Option<f64>,
    percent_change: Option<f64>,
}

fn chart_data_csv_rows(data: &ChartData) -> Vec<ChartDataCsvRow<'_>> {
    data.points
        .iter()
        .map(|p| ChartDataCsvRow {
            identifier: &data.identifier,
            name: &data.name,
            close_price: data.close_price,
            timestamp: p.timestamp,
            price: p.price,
            session: &p.session,
            change: p.change,
            percent_change: p.percent_change,
        })
        .collect()
}

/// Which time window to fetch an index's price chart for.
///
/// A different, larger set of confirmed-valid values than `ChartPeriod`
/// (stocks) - confirmed live, this endpoint accepts `3M`/`6M` (which
/// return a 500 for `stock_chart_data_raw`) but, like stocks, rejects
/// `3Y`/`ALL` (HTTP 404, same envelope as an unknown index name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexChartPeriod {
    OneDay,
    OneWeek,
    OneMonth,
    ThreeMonths,
    SixMonths,
    OneYear,
    FiveYears,
}

impl IndexChartPeriod {
    fn as_query_param(self) -> &'static str {
        match self {
            IndexChartPeriod::OneDay => "1D",
            IndexChartPeriod::OneWeek => "1W",
            IndexChartPeriod::OneMonth => "1M",
            IndexChartPeriod::ThreeMonths => "3M",
            IndexChartPeriod::SixMonths => "6M",
            IndexChartPeriod::OneYear => "1Y",
            IndexChartPeriod::FiveYears => "5Y",
        }
    }

    // Used to build a CSV filename - lowercase, matching this crate's other
    // filename conventions rather than the API's own uppercase query value.
    fn label(self) -> &'static str {
        match self {
            IndexChartPeriod::OneDay => "1d",
            IndexChartPeriod::OneWeek => "1w",
            IndexChartPeriod::OneMonth => "1m",
            IndexChartPeriod::ThreeMonths => "3m",
            IndexChartPeriod::SixMonths => "6m",
            IndexChartPeriod::OneYear => "1y",
            IndexChartPeriod::FiveYears => "5y",
        }
    }
}

/// One point on an index's price chart.
///
/// Unlike `ChartDataPoint` (stocks), `change`/`percent_change` are
/// always real JSON numbers here, never a string or `null` - confirmed
/// live, every non-1D window sends a literal `0`/`0` instead of omitting
/// them, so there's no clean "not applicable" signal to model as `None`
/// without silently losing a genuinely-flat data point.
#[derive(Debug, Serialize)]
pub struct IndexChartDataPoint {
    // IST wall-clock time, not UTC - confirmed live to have the
    // identical bug as `ChartDataPoint` - see `ist_timestamp_from_millis`.
    pub timestamp: NaiveDateTime,
    pub price: f64,
    pub session: String,
    pub change: f64,
    pub percent_change: f64,
}

/// An index's intraday or historical price chart -
/// `NseQuote::index_chart_data_raw`.
#[derive(Debug, Serialize)]
pub struct IndexChartData {
    pub identifier: String,
    pub name: String,
    pub close_price: f64,
    pub points: Vec<IndexChartDataPoint>,
}

/// One `IndexChartDataPoint` flattened for CSV, mirroring `ChartDataCsvRow`
/// (stocks) - `change`/`percent_change` stay plain `f64` here, matching
/// `IndexChartDataPoint`'s own fields.
#[derive(Debug, Serialize)]
struct IndexChartDataCsvRow<'a> {
    identifier: &'a str,
    name: &'a str,
    close_price: f64,
    timestamp: NaiveDateTime,
    price: f64,
    session: &'a str,
    change: f64,
    percent_change: f64,
}

fn index_chart_data_csv_rows(data: &IndexChartData) -> Vec<IndexChartDataCsvRow<'_>> {
    data.points
        .iter()
        .map(|p| IndexChartDataCsvRow {
            identifier: &data.identifier,
            name: &data.name,
            close_price: data.close_price,
            timestamp: p.timestamp,
            price: p.price,
            session: &p.session,
            change: p.change,
            percent_change: p.percent_change,
        })
        .collect()
}

/// One `grapthData` point for an index chart - see `RawChartDataPoint`
/// (stocks) for why NSE's own misspelling is kept as the wire name.
type RawIndexChartDataPoint = (i64, f64, String, f64, f64);

#[derive(Debug, Deserialize)]
struct RawIndexChartData {
    identifier: String,
    name: String,
    #[serde(rename = "grapthData")]
    graph_data: Vec<RawIndexChartDataPoint>,
    #[serde(rename = "closePrice")]
    close_price: f64,
}

#[derive(Debug, Deserialize)]
struct IndexChartResponse {
    data: RawIndexChartData,
}

fn index_chart_data_point_from_tuple(row: RawIndexChartDataPoint) -> Result<IndexChartDataPoint> {
    let (millis, price, session, change, percent_change) = row;
    let timestamp = ist_timestamp_from_millis(millis)
        .ok_or_else(|| Error::Parse(format!("invalid index chart timestamp: {millis}")))?;
    Ok(IndexChartDataPoint {
        timestamp,
        price,
        session,
        change,
        percent_change,
    })
}

/// A single index's live value, volume and turnover.
///
/// Distinct from `IndexSnapshotRow` (`NseLiveMarket::index_snapshot_raw`,
/// backed by `allIndices`): this adds `total_traded_volume`/
/// `total_traded_value`/`last_update_time` that `allIndices` doesn't have,
/// but drops P/E/P/B/dividend-yield and market breadth - a genuinely
/// different endpoint, not a filtered view of the same data.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexQuote {
    #[serde(rename(deserialize = "symbol"))]
    pub name: String,
    #[serde(rename(deserialize = "lastPrice"))]
    pub last: f64,
    pub change: f64,
    #[serde(rename(deserialize = "pChange"))]
    pub percent_change: f64,
    pub open: f64,
    #[serde(rename(deserialize = "dayHigh"))]
    pub day_high: f64,
    #[serde(rename(deserialize = "dayLow"))]
    pub day_low: f64,
    #[serde(rename(deserialize = "previousClose"))]
    pub previous_close: f64,
    #[serde(rename(deserialize = "yearHigh"))]
    pub year_high: f64,
    #[serde(rename(deserialize = "yearLow"))]
    pub year_low: f64,
    #[serde(rename(deserialize = "totalTradedVolume"))]
    pub total_traded_volume: u64,
    #[serde(rename(deserialize = "totalTradedValue"))]
    pub total_traded_value: f64,
    #[serde(rename(deserialize = "lastUpdateTime"))]
    pub last_update_time: String,
}

#[derive(Debug, Deserialize)]
struct IndexQuoteResponse {
    data: Vec<IndexQuote>,
}

/// Which option-chain flavor to fetch - `index_option_chain`/
/// `equities_option_chain` in Python, confirmed live to be the same
/// endpoint and response shape, differing only by this query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionChainKind {
    Index,
    Equity,
}

impl OptionChainKind {
    fn as_query_param(self) -> &'static str {
        match self {
            OptionChainKind::Index => "Indices",
            OptionChainKind::Equity => "Equity",
        }
    }

    // Used to build a CSV filename - lowercase, filesystem-friendly,
    // distinct from the API's own casing in `as_query_param`.
    fn label(self) -> &'static str {
        match self {
            OptionChainKind::Index => "index",
            OptionChainKind::Equity => "equity",
        }
    }
}

/// One leg (call or put) of an option-chain row.
///
/// Drops a duplicate `PChange`/`pChange` pair (identical values, confirmed
/// live) and an `optionType` field that's always `null` in this response -
/// NSE encodes call/put via which of `CE`/`PE` is populated instead.
#[derive(Debug, Serialize, Deserialize)]
pub struct OptionLeg {
    // `None` when there's no real contract for this strike/side - NSE
    // still sends a `CE`/`PE` object either way (every other field zeroed
    // out), rather than omitting the key. Confirmed live on deep SBIN
    // equity strikes.
    pub identifier: Option<String>,
    #[serde(rename(deserialize = "lastPrice"))]
    pub last_price: f64,
    pub change: f64,
    #[serde(rename(deserialize = "pChange"))]
    pub percent_change: f64,
    // Same float-or-integer inconsistency `DerivativeQuoteRow` already
    // guards against - confirmed live on the real NIFTY index chain (128
    // strikes), where a plain `u64`/`i64` fails to deserialize a
    // fractional value like `48608.769230769234` on this field. Applied
    // to every count-like field on this leg defensively, not just the one
    // that happened to trip in that sample.
    #[serde(
        rename(deserialize = "openInterest"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub open_interest: u64,
    #[serde(
        rename(deserialize = "changeinOpenInterest"),
        deserialize_with = "deserialize_lenient_i64"
    )]
    pub change_in_open_interest: i64,
    #[serde(rename(deserialize = "pchangeinOpenInterest"))]
    pub percent_change_in_open_interest: f64,
    #[serde(
        rename(deserialize = "totalTradedVolume"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_traded_volume: u64,
    #[serde(rename(deserialize = "impliedVolatility"))]
    pub implied_volatility: f64,
    #[serde(rename(deserialize = "buyPrice1"))]
    pub buy_price: f64,
    #[serde(
        rename(deserialize = "buyQuantity1"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub buy_quantity: u64,
    #[serde(rename(deserialize = "sellPrice1"))]
    pub sell_price: f64,
    #[serde(
        rename(deserialize = "sellQuantity1"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub sell_quantity: u64,
    #[serde(
        rename(deserialize = "totalBuyQuantity"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_buy_quantity: u64,
    #[serde(
        rename(deserialize = "totalSellQuantity"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_sell_quantity: u64,
    #[serde(rename(deserialize = "underlyingValue"))]
    pub underlying_value: f64,
}

/// One strike price's call/put pair for an index or equity option chain.
/// `call`/`put` are `Option` defensively - every strike had both in
/// testing, but NSE doesn't guarantee it (e.g. a strike with no listed
/// contract on one side).
#[derive(Debug, Serialize, Deserialize)]
pub struct OptionChainRow {
    #[serde(rename(deserialize = "strikePrice"))]
    pub strike_price: f64,
    #[serde(
        rename(deserialize = "expiryDates"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub expiry: NaiveDate,
    #[serde(rename(deserialize = "CE"))]
    pub call: Option<OptionLeg>,
    #[serde(rename(deserialize = "PE"))]
    pub put: Option<OptionLeg>,
}

/// One `OptionChainRow` leg, flattened for CSV: a "CE"/"PE" `option_type`
/// column instead of separate `call`/`put` fields, matching the
/// one-row-per-contract convention `DerivativeQuoteRow`/`LiveFoRow` use
/// elsewhere. Legs with no real contract (`identifier: None`) are skipped
/// entirely rather than written as a mostly-empty row.
#[derive(Debug, Serialize)]
struct OptionChainCsvRow {
    strike_price: f64,
    expiry: NaiveDate,
    option_type: &'static str,
    identifier: String,
    last_price: f64,
    change: f64,
    percent_change: f64,
    open_interest: u64,
    change_in_open_interest: i64,
    percent_change_in_open_interest: f64,
    total_traded_volume: u64,
    implied_volatility: f64,
    buy_price: f64,
    buy_quantity: u64,
    sell_price: f64,
    sell_quantity: u64,
    total_buy_quantity: u64,
    total_sell_quantity: u64,
    underlying_value: f64,
}

fn option_leg_csv_row(
    strike_price: f64,
    expiry: NaiveDate,
    option_type: &'static str,
    leg: &OptionLeg,
) -> Option<OptionChainCsvRow> {
    Some(OptionChainCsvRow {
        strike_price,
        expiry,
        option_type,
        identifier: leg.identifier.clone()?,
        last_price: leg.last_price,
        change: leg.change,
        percent_change: leg.percent_change,
        open_interest: leg.open_interest,
        change_in_open_interest: leg.change_in_open_interest,
        percent_change_in_open_interest: leg.percent_change_in_open_interest,
        total_traded_volume: leg.total_traded_volume,
        implied_volatility: leg.implied_volatility,
        buy_price: leg.buy_price,
        buy_quantity: leg.buy_quantity,
        sell_price: leg.sell_price,
        sell_quantity: leg.sell_quantity,
        total_buy_quantity: leg.total_buy_quantity,
        total_sell_quantity: leg.total_sell_quantity,
        underlying_value: leg.underlying_value,
    })
}

fn option_chain_csv_rows(rows: &[OptionChainRow]) -> Vec<OptionChainCsvRow> {
    rows.iter()
        .flat_map(|row| {
            let call = row
                .call
                .as_ref()
                .and_then(|leg| option_leg_csv_row(row.strike_price, row.expiry, "CE", leg));
            let put = row
                .put
                .as_ref()
                .and_then(|leg| option_leg_csv_row(row.strike_price, row.expiry, "PE", leg));
            call.into_iter().chain(put)
        })
        .collect()
}

/// Like `OptionLeg`, but for `option-chain-currency` - confirmed live to
/// use a different shape (`bidPrice`/`bidQty`/`askPrice`/`askQty` instead
/// of `buyPrice1`/.../`sellQuantity1`, and no duplicate `PChange`).
/// `identifier` is `Option` for the same no-real-contract reason as
/// `OptionLeg`.
#[derive(Debug, Serialize, Deserialize)]
pub struct CurrencyOptionLeg {
    pub identifier: Option<String>,
    #[serde(rename(deserialize = "lastPrice"))]
    pub last_price: f64,
    pub change: f64,
    #[serde(rename(deserialize = "pChange"))]
    pub percent_change: f64,
    // Same float-or-integer inconsistency as `OptionLeg` - see its comment.
    #[serde(
        rename(deserialize = "openInterest"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub open_interest: u64,
    #[serde(
        rename(deserialize = "changeinOpenInterest"),
        deserialize_with = "deserialize_lenient_i64"
    )]
    pub change_in_open_interest: i64,
    #[serde(rename(deserialize = "pchangeinOpenInterest"))]
    pub percent_change_in_open_interest: f64,
    #[serde(
        rename(deserialize = "totalTradedVolume"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_traded_volume: u64,
    #[serde(rename(deserialize = "impliedVolatility"))]
    pub implied_volatility: f64,
    #[serde(rename(deserialize = "bidprice"))]
    pub bid_price: f64,
    #[serde(
        rename(deserialize = "bidQty"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub bid_quantity: u64,
    #[serde(rename(deserialize = "askPrice"))]
    pub ask_price: f64,
    #[serde(
        rename(deserialize = "askQty"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub ask_quantity: u64,
    #[serde(
        rename(deserialize = "totalBuyQuantity"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_buy_quantity: u64,
    #[serde(
        rename(deserialize = "totalSellQuantity"),
        deserialize_with = "deserialize_lenient_u64"
    )]
    pub total_sell_quantity: u64,
    #[serde(rename(deserialize = "underlyingValue"))]
    pub underlying_value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CurrencyOptionChainRow {
    #[serde(rename(deserialize = "strikePrice"))]
    pub strike_price: f64,
    #[serde(
        rename(deserialize = "expiryDate"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub expiry: NaiveDate,
    #[serde(rename(deserialize = "CE"))]
    pub call: Option<CurrencyOptionLeg>,
    #[serde(rename(deserialize = "PE"))]
    pub put: Option<CurrencyOptionLeg>,
}

/// Like `OptionChainCsvRow`, but for `CurrencyOptionChainRow`'s
/// bid/ask-shaped legs.
#[derive(Debug, Serialize)]
struct CurrencyOptionChainCsvRow {
    strike_price: f64,
    expiry: NaiveDate,
    option_type: &'static str,
    identifier: String,
    last_price: f64,
    change: f64,
    percent_change: f64,
    open_interest: u64,
    change_in_open_interest: i64,
    percent_change_in_open_interest: f64,
    total_traded_volume: u64,
    implied_volatility: f64,
    bid_price: f64,
    bid_quantity: u64,
    ask_price: f64,
    ask_quantity: u64,
    total_buy_quantity: u64,
    total_sell_quantity: u64,
    underlying_value: f64,
}

fn currency_option_leg_csv_row(
    strike_price: f64,
    expiry: NaiveDate,
    option_type: &'static str,
    leg: &CurrencyOptionLeg,
) -> Option<CurrencyOptionChainCsvRow> {
    Some(CurrencyOptionChainCsvRow {
        strike_price,
        expiry,
        option_type,
        identifier: leg.identifier.clone()?,
        last_price: leg.last_price,
        change: leg.change,
        percent_change: leg.percent_change,
        open_interest: leg.open_interest,
        change_in_open_interest: leg.change_in_open_interest,
        percent_change_in_open_interest: leg.percent_change_in_open_interest,
        total_traded_volume: leg.total_traded_volume,
        implied_volatility: leg.implied_volatility,
        bid_price: leg.bid_price,
        bid_quantity: leg.bid_quantity,
        ask_price: leg.ask_price,
        ask_quantity: leg.ask_quantity,
        total_buy_quantity: leg.total_buy_quantity,
        total_sell_quantity: leg.total_sell_quantity,
        underlying_value: leg.underlying_value,
    })
}

fn currency_option_chain_csv_rows(
    rows: &[CurrencyOptionChainRow],
) -> Vec<CurrencyOptionChainCsvRow> {
    rows.iter()
        .flat_map(|row| {
            let call = row.call.as_ref().and_then(|leg| {
                currency_option_leg_csv_row(row.strike_price, row.expiry, "CE", leg)
            });
            let put = row.put.as_ref().and_then(|leg| {
                currency_option_leg_csv_row(row.strike_price, row.expiry, "PE", leg)
            });
            call.into_iter().chain(put)
        })
        .collect()
}

/// Shared envelope for `option-chain-v3`/`option-chain-currency`: the
/// `records` key is missing entirely (not just empty) for an unknown
/// symbol - confirmed live - so this is `Option`, not a plain struct.
#[derive(Debug, Deserialize)]
struct OptionChainResponse<T> {
    records: Option<OptionChainRecords<T>>,
}

#[derive(Debug, Deserialize)]
struct OptionChainRecords<T> {
    data: Vec<T>,
}

/// `option-chain-contract-info`'s response - only `expiryDates` is used,
/// to find the nearest expiry when the caller doesn't specify one. Missing
/// entirely (not just empty) for an unknown symbol - confirmed live.
#[derive(Debug, Deserialize, Default)]
struct ContractInfo {
    #[serde(rename = "expiryDates", default)]
    expiry_dates: Vec<String>,
}

/// A symbol's regulatory/compliance status.
///
/// `reg_action`/`series`/`reg_note` are `None` for every ordinary,
/// unrestricted symbol checked live (SBIN, TCS) - presumably these only
/// populate for a symbol NSE has flagged (suspended, under
/// investigation, etc.), which wasn't available to confirm.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegDetailsRow {
    pub symbol: String,
    #[serde(rename(deserialize = "scripCode"))]
    pub scrip_code: String,
    #[serde(rename(deserialize = "nseExclusive"))]
    pub nse_exclusive: String,
    pub status: String,
    #[serde(rename(deserialize = "regAction"))]
    pub reg_action: Option<String>,
    pub series: Option<String>,
    #[serde(rename(deserialize = "regNote"))]
    pub reg_note: Option<String>,
}

/// A symbol's static metadata - eligibility flags, series, ISIN.
///
/// NSE sends every `is*`/`casFlag` field as the JSON string `"true"`/
/// `"false"`, not a real JSON boolean - confirmed live. Parsed into
/// actual `bool`s via `deserialize_string_bool`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SymbolMetaRow {
    pub symbol: String,
    #[serde(rename(deserialize = "companyName"))]
    pub company_name: String,
    pub isin: String,
    #[serde(rename(deserialize = "marketType"))]
    pub market_type: String,
    #[serde(rename(deserialize = "parentSymbol"))]
    pub parent_symbol: String,
    #[serde(rename(deserialize = "activeSeries"))]
    pub active_series: Vec<String>,
    #[serde(rename(deserialize = "debtSeries"))]
    pub debt_series: Vec<String>,
    #[serde(rename(deserialize = "tempSuspendedSeries"))]
    pub temp_suspended_series: Vec<String>,
    #[serde(
        rename(deserialize = "isFNOSec"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_fno_eligible: bool,
    #[serde(
        rename(deserialize = "isCASec"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_corporate_action_sec: bool,
    #[serde(
        rename(deserialize = "isSLBSec"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_slb_eligible: bool,
    #[serde(
        rename(deserialize = "isDebtSec"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_debt_sec: bool,
    #[serde(
        rename(deserialize = "isSuspended"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_suspended: bool,
    #[serde(
        rename(deserialize = "isETFSec"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_etf: bool,
    #[serde(
        rename(deserialize = "isDelisted"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_delisted: bool,
    #[serde(
        rename(deserialize = "isMunicipalBond"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_municipal_bond: bool,
    #[serde(
        rename(deserialize = "isHybridSymbol"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub is_hybrid_symbol: bool,
    #[serde(
        rename(deserialize = "casFlag"),
        deserialize_with = "deserialize_string_bool"
    )]
    pub cas_flag: bool,
}

/// `SymbolMetaRow` flattened for CSV - the `csv` crate can't write a
/// `Vec<String>` field as a column, so the three series lists are joined
/// into semicolon-separated strings here.
#[derive(Debug, Serialize)]
struct SymbolMetaCsvRow<'a> {
    symbol: &'a str,
    company_name: &'a str,
    isin: &'a str,
    market_type: &'a str,
    parent_symbol: &'a str,
    active_series: String,
    debt_series: String,
    temp_suspended_series: String,
    is_fno_eligible: bool,
    is_corporate_action_sec: bool,
    is_slb_eligible: bool,
    is_debt_sec: bool,
    is_suspended: bool,
    is_etf: bool,
    is_delisted: bool,
    is_municipal_bond: bool,
    is_hybrid_symbol: bool,
    cas_flag: bool,
}

fn symbol_meta_csv_row(row: &SymbolMetaRow) -> SymbolMetaCsvRow<'_> {
    SymbolMetaCsvRow {
        symbol: &row.symbol,
        company_name: &row.company_name,
        isin: &row.isin,
        market_type: &row.market_type,
        parent_symbol: &row.parent_symbol,
        active_series: row.active_series.join(";"),
        debt_series: row.debt_series.join(";"),
        temp_suspended_series: row.temp_suspended_series.join(";"),
        is_fno_eligible: row.is_fno_eligible,
        is_corporate_action_sec: row.is_corporate_action_sec,
        is_slb_eligible: row.is_slb_eligible,
        is_debt_sec: row.is_debt_sec,
        is_suspended: row.is_suspended,
        is_etf: row.is_etf,
        is_delisted: row.is_delisted,
        is_municipal_bond: row.is_municipal_bond,
        is_hybrid_symbol: row.is_hybrid_symbol,
        cas_flag: row.cas_flag,
    }
}

fn deserialize_string_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "expected \"true\" or \"false\", got {other:?}"
        ))),
    }
}

/// A symbol's basic identity - just the company name behind a symbol.
#[derive(Debug, Serialize, Deserialize)]
pub struct SymbolNameRow {
    pub symbol: String,
    #[serde(rename(deserialize = "companyName"))]
    pub company_name: String,
}

/// A symbol's price change over several trailing windows (yesterday
/// through 5 years), alongside its benchmark index's change over the
/// same windows for comparison. Despite the "yearwise" name, this is a
/// point-in-time multi-timeframe snapshot, not one row per calendar year
/// - confirmed live, NSE always returns exactly one entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct YearwiseDataRow {
    #[serde(rename(deserialize = "yesterday_chng_per"))]
    pub yesterday_change_pct: f64,
    #[serde(rename(deserialize = "one_week_chng_per"))]
    pub one_week_change_pct: f64,
    #[serde(rename(deserialize = "one_month_chng_per"))]
    pub one_month_change_pct: f64,
    #[serde(rename(deserialize = "three_month_chng_per"))]
    pub three_month_change_pct: f64,
    #[serde(rename(deserialize = "six_month_chng_per"))]
    pub six_month_change_pct: f64,
    #[serde(rename(deserialize = "one_year_chng_per"))]
    pub one_year_change_pct: f64,
    #[serde(rename(deserialize = "two_year_chng_per"))]
    pub two_year_change_pct: f64,
    #[serde(rename(deserialize = "three_year_chng_per"))]
    pub three_year_change_pct: f64,
    #[serde(rename(deserialize = "five_year_chng_per"))]
    pub five_year_change_pct: f64,
    // "16-SEP-26" - a two-digit year, unlike every other date field in
    // this crate (`%d-%b-%Y`) - confirmed live.
    #[serde(
        rename(deserialize = "one_week_date"),
        deserialize_with = "deserialize_nse_date_short_year"
    )]
    pub one_week_date: NaiveDate,
    #[serde(rename(deserialize = "index_name"))]
    pub index_name: String,
    #[serde(rename(deserialize = "index_yesterday_chng_per"))]
    pub index_yesterday_change_pct: f64,
    #[serde(rename(deserialize = "index_one_week_chng_per"))]
    pub index_one_week_change_pct: f64,
    #[serde(rename(deserialize = "index_one_month_chng_per"))]
    pub index_one_month_change_pct: f64,
    #[serde(rename(deserialize = "index_three_month_chng_per"))]
    pub index_three_month_change_pct: f64,
    #[serde(rename(deserialize = "index_six_month_chng_per"))]
    pub index_six_month_change_pct: f64,
    #[serde(rename(deserialize = "index_one_year_chng_per"))]
    pub index_one_year_change_pct: f64,
    #[serde(rename(deserialize = "index_two_year_chng_per"))]
    pub index_two_year_change_pct: f64,
    #[serde(rename(deserialize = "index_three_year_chng_per"))]
    pub index_three_year_change_pct: f64,
    #[serde(rename(deserialize = "index_five_year_chng_per"))]
    pub index_five_year_change_pct: f64,
    #[serde(
        rename(deserialize = "index_one_week_date"),
        deserialize_with = "deserialize_nse_date_short_year"
    )]
    pub index_one_week_date: NaiveDate,
}

fn deserialize_nse_date_short_year<'de, D>(
    deserializer: D,
) -> std::result::Result<NaiveDate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&raw, "%d-%b-%y").map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone)]
pub struct NseQuote {
    client: HttpClient,
}

impl NseQuote {
    pub fn new() -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client: with_retry(client),
        })
    }

    /// Fetches a stock's live quote: price/change, day and 52-week range,
    /// traded volume/value/delivery, and order book depth.
    pub async fn stock_quote_raw(&self, symbol: &str) -> Result<StockQuote> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[
                ("functionName", "getSymbolData"),
                ("marketType", "N"),
                ("series", "EQ"),
                ("symbol", symbol),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => {
                return Err(Error::NotFound(format!(
                    "no live quote for symbol '{symbol}'"
                )));
            }
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: StockQuoteResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse stock quote response: {e}")))?;

        parsed
            .equity_response
            .into_iter()
            .next()
            .map(StockQuote::from)
            .ok_or_else(|| Error::NotFound(format!("no live quote for symbol '{symbol}'")))
    }

    /// Fetches a stock's live quote the same way as `stock_quote_raw`,
    /// then writes it as a one-row CSV file into `dest` (a directory - the
    /// filename is derived from `symbol`). Returns the path written.
    pub async fn stock_quote_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let quote = self.stock_quote_raw(symbol).await?;
        let csv_row = StockQuoteCsvRow::from(&quote);
        let path = dest.join(format!("{symbol}-quote.csv"));
        write_csv(&[csv_row], &path)
    }

    /// Fetches a stock's intraday or historical price chart. Replaces the
    /// documented-dead `chart-databyindex` endpoint (see
    /// `docs/nse-findings.md`) - confirmed live, this NextApi function is
    /// the one NSE's own site actually calls today.
    pub async fn stock_chart_data_raw(
        &self,
        symbol: &str,
        period: ChartPeriod,
    ) -> Result<ChartData> {
        let identifier = format!("{symbol}EQN");
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[
                ("functionName", "getSymbolChartData"),
                ("symbol", identifier.as_str()),
                ("days", period.as_query_param()),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => {
                return Err(Error::NotFound(format!(
                    "no chart data for symbol '{symbol}'"
                )));
            }
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let raw: RawChartData = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse chart data response: {e}")))?;

        let points = raw
            .graph_data
            .into_iter()
            .map(chart_data_point_from_tuple)
            .collect::<Result<Vec<_>>>()?;

        Ok(ChartData {
            identifier: raw.identifier,
            name: raw.name,
            close_price: raw.close_price,
            points,
        })
    }

    /// Fetches a stock's price chart the same way as `stock_chart_data_raw`,
    /// then writes it as a CSV file into `dest` (a directory - the filename
    /// is derived from `symbol` and `period`), one row per point. Returns
    /// the path written.
    pub async fn stock_chart_data_csv(
        &self,
        symbol: &str,
        period: ChartPeriod,
        dest: &Path,
    ) -> Result<PathBuf> {
        let data = self.stock_chart_data_raw(symbol, period).await?;
        let path = dest.join(format!("{symbol}-chart-{}.csv", period.label()));
        let csv_rows = chart_data_csv_rows(&data);
        write_csv(&csv_rows, &path)
    }

    /// Fetches an index's intraday or historical price chart - the
    /// per-index counterpart to `stock_chart_data_raw`. A genuinely
    /// different endpoint (`getGraphChart` against `NEXTAPI_APICLIENT_URL`,
    /// not `getSymbolChartData` against `NEXTAPI_URL`), found by
    /// inspecting the live-equity-market index page's network requests
    /// after `stock_chart_data_raw`'s endpoint confirmed it only accepts
    /// stock symbols. Returns `Error::NotFound` for an unknown index name
    /// - confirmed live, HTTP 404 with an empty `data`.
    pub async fn index_chart_data_raw(
        &self,
        name: &str,
        period: IndexChartPeriod,
    ) -> Result<IndexChartData> {
        let response = self
            .client
            .get(NEXTAPI_APICLIENT_URL)
            .query(&[
                ("functionName", "getGraphChart"),
                ("type", name),
                ("flag", period.as_query_param()),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => {
                return Err(Error::NotFound(format!("no chart data for index '{name}'")));
            }
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: IndexChartResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse index chart data response: {e}")))?;

        let points = parsed
            .data
            .graph_data
            .into_iter()
            .map(index_chart_data_point_from_tuple)
            .collect::<Result<Vec<_>>>()?;

        Ok(IndexChartData {
            identifier: parsed.data.identifier,
            name: parsed.data.name,
            close_price: parsed.data.close_price,
            points,
        })
    }

    /// Fetches an index's price chart the same way as `index_chart_data_raw`,
    /// then writes it as a CSV file into `dest` (a directory - the filename
    /// is derived from `name` and `period`), one row per point. Returns the
    /// path written.
    pub async fn index_chart_data_csv(
        &self,
        name: &str,
        period: IndexChartPeriod,
        dest: &Path,
    ) -> Result<PathBuf> {
        let data = self.index_chart_data_raw(name, period).await?;
        let path = dest.join(format!("{name}-chart-{}.csv", period.label()));
        let csv_rows = index_chart_data_csv_rows(&data);
        write_csv(&csv_rows, &path)
    }

    /// Fetches every F&O contract (all expiries, all strikes, futures and
    /// options alike) for a single underlying symbol. Returns an empty
    /// `Vec` for an unknown symbol - confirmed live.
    pub async fn derivative_quote_raw(&self, symbol: &str) -> Result<Vec<DerivativeQuoteRow>> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[
                ("functionName", "getSymbolDerivativesData"),
                ("symbol", symbol),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: DerivativeQuoteResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse derivative quote response: {e}")))?;

        Ok(parsed.data)
    }

    /// Fetches F&O contracts the same way as `derivative_quote_raw`, then
    /// writes them as a CSV file into `dest` (a directory - the filename
    /// is derived from `symbol`). Returns the path written.
    pub async fn derivative_quote_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let rows = self.derivative_quote_raw(symbol).await?;
        let path = dest.join(format!("{symbol}-derivative-quote.csv"));
        write_csv(&rows, &path)
    }

    /// Fetches a single index's live value, volume and turnover. Unlike
    /// the history/snapshot endpoints elsewhere in this crate, an unknown
    /// index name is treated as `Error::NotFound` rather than an empty
    /// result: confirmed live, there's no "market closed" ambiguity here
    /// (a valid index always has a live value, market open or not), so an
    /// empty response really does mean the name was wrong.
    pub async fn index_quote_raw(&self, name: &str) -> Result<IndexQuote> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/equity-stock-indices"))
            .query(&[("index", name)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: IndexQuoteResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse index quote response: {e}")))?;

        parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(format!("no live data for index '{name}'")))
    }

    /// Fetches an index's live quote the same way as `index_quote_raw`,
    /// then writes it as a one-row CSV file into `dest` (a directory - the
    /// filename is derived from `name`). Returns the path written.
    pub async fn index_quote_csv(&self, name: &str, dest: &Path) -> Result<PathBuf> {
        let quote = self.index_quote_raw(name).await?;
        let path = dest.join(format!("{name}-quote.csv"));
        write_csv(&[quote], &path)
    }

    /// Fetches the option chain for an index or equity `symbol`. If
    /// `expiry` is `None`, looks up the nearest available expiry first
    /// (matching Python's default behavior) via a separate
    /// `option-chain-contract-info` request.
    pub async fn option_chain_raw(
        &self,
        symbol: &str,
        kind: OptionChainKind,
        expiry: Option<NaiveDate>,
    ) -> Result<Vec<OptionChainRow>> {
        let expiry = match expiry {
            Some(e) => e,
            None => self.nearest_expiry(symbol).await?,
        };
        let expiry_str = expiry.format("%d-%b-%Y").to_string();

        let response = self
            .client
            .get(format!("{BASE_URL}/api/option-chain-v3"))
            .query(&[
                ("type", kind.as_query_param()),
                ("symbol", symbol),
                ("expiry", expiry_str.as_str()),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: OptionChainResponse<OptionChainRow> = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse option chain response: {e}")))?;

        Ok(parsed.records.map(|r| r.data).unwrap_or_default())
    }

    /// Fetches the option chain the same way as `option_chain_raw`, then
    /// writes it as a CSV file into `dest` (a directory), one row per
    /// contract (call/put legs with no real contract are skipped). The
    /// filename is derived from `symbol`, `kind` and the resolved expiry -
    /// taken from the fetched rows themselves, since `option_chain_raw`
    /// may have picked the nearest expiry internally when `expiry` is
    /// `None`. Returns the path written.
    pub async fn option_chain_csv(
        &self,
        symbol: &str,
        kind: OptionChainKind,
        expiry: Option<NaiveDate>,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self.option_chain_raw(symbol, kind, expiry).await?;
        let resolved_expiry = rows.first().map(|r| r.expiry).or(expiry);
        let file_name = match resolved_expiry {
            Some(e) => format!("{symbol}-{}-optionchain-{e}.csv", kind.label()),
            None => format!("{symbol}-{}-optionchain.csv", kind.label()),
        };
        let path = dest.join(file_name);
        let csv_rows = option_chain_csv_rows(&rows);
        write_csv(&csv_rows, &path)
    }

    /// Fetches a currency pair's option chain (defaults to `"USDINR"` in
    /// Python; the caller passes it explicitly here).
    pub async fn currency_option_chain_raw(
        &self,
        symbol: &str,
    ) -> Result<Vec<CurrencyOptionChainRow>> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/option-chain-currency"))
            .query(&[("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: OptionChainResponse<CurrencyOptionChainRow> =
            response.json().await.map_err(|e| {
                Error::Parse(format!(
                    "could not parse currency option chain response: {e}"
                ))
            })?;

        Ok(parsed.records.map(|r| r.data).unwrap_or_default())
    }

    /// Fetches the currency option chain the same way as
    /// `currency_option_chain_raw`, then writes it as a CSV file into
    /// `dest` (a directory - the filename is derived from `symbol`), one
    /// row per contract. Returns the path written.
    pub async fn currency_option_chain_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let rows = self.currency_option_chain_raw(symbol).await?;
        let path = dest.join(format!("{symbol}-currency-optionchain.csv"));
        let csv_rows = currency_option_chain_csv_rows(&rows);
        write_csv(&csv_rows, &path)
    }

    /// Looks up the nearest expiry date for `symbol` via
    /// `option-chain-contract-info`, for `option_chain_raw` callers that
    /// don't specify one. An unknown symbol has no expiry dates at all -
    /// confirmed live - which surfaces here as `Error::NotFound`.
    async fn nearest_expiry(&self, symbol: &str) -> Result<NaiveDate> {
        let response = self
            .client
            .get(format!("{BASE_URL}/api/option-chain-contract-info"))
            .query(&[("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: ContractInfo = response.json().await.map_err(|e| {
            Error::Parse(format!("could not parse option chain contract info: {e}"))
        })?;

        let first = parsed.expiry_dates.first().ok_or_else(|| {
            Error::NotFound(format!("no option chain contracts for symbol '{symbol}'"))
        })?;

        NaiveDate::parse_from_str(first, "%d-%b-%Y")
            .map_err(|e| Error::Parse(format!("could not parse expiry date '{first}': {e}")))
    }

    /// Fetches a symbol's regulatory/compliance status. Returns an empty
    /// `Vec` for an unknown symbol - confirmed live.
    pub async fn reg_details_raw(&self, symbol: &str) -> Result<Vec<RegDetailsRow>> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[("functionName", "getRegDetails"), ("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse reg details response: {e}")))
    }

    /// Fetches reg details the same way as `reg_details_raw`, then writes
    /// them as a CSV file into `dest` (a directory - the filename is
    /// derived from `symbol`). Returns the path written.
    pub async fn reg_details_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let rows = self.reg_details_raw(symbol).await?;
        let path = dest.join(format!("{symbol}-reg-details.csv"));
        write_csv(&rows, &path)
    }

    /// Fetches the names of every index `symbol` is a constituent of.
    /// Returns an empty `Vec` for an unknown symbol - confirmed live.
    pub async fn index_list_raw(&self, symbol: &str) -> Result<Vec<String>> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[("functionName", "getIndexList"), ("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse index list response: {e}")))
    }

    /// Fetches a symbol's static metadata (eligibility flags, series,
    /// ISIN). Confirmed live: an unknown symbol returns HTTP 200 with
    /// every field `null` rather than a 404 or an empty object, so this
    /// checks `symbol` itself before committing to the strict shape.
    pub async fn symbol_meta_raw(&self, symbol: &str) -> Result<SymbolMetaRow> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[("functionName", "getMetaData"), ("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse symbol meta response: {e}")))?;

        if !value.get("symbol").is_some_and(|v| v.is_string()) {
            return Err(Error::NotFound(format!(
                "no metadata for symbol '{symbol}'"
            )));
        }

        serde_json::from_value(value)
            .map_err(|e| Error::Parse(format!("could not parse symbol meta response: {e}")))
    }

    /// Fetches symbol metadata the same way as `symbol_meta_raw`, then
    /// writes it as a CSV file into `dest` (a directory - the filename is
    /// derived from `symbol`). Returns the path written.
    pub async fn symbol_meta_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let row = self.symbol_meta_raw(symbol).await?;
        let path = dest.join(format!("{symbol}-meta.csv"));
        write_csv(&[symbol_meta_csv_row(&row)], &path)
    }

    /// Fetches the company name behind a symbol. Confirmed live: an
    /// unknown symbol returns HTTP 200 with an empty object rather than a
    /// 404 - see `symbol_meta_raw`.
    pub async fn symbol_name_raw(&self, symbol: &str) -> Result<SymbolNameRow> {
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[("functionName", "getSymbolName"), ("symbol", symbol)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse symbol name response: {e}")))?;

        if !value.get("symbol").is_some_and(|v| v.is_string()) {
            return Err(Error::NotFound(format!("no symbol name for '{symbol}'")));
        }

        serde_json::from_value(value)
            .map_err(|e| Error::Parse(format!("could not parse symbol name response: {e}")))
    }

    /// Fetches the symbol name the same way as `symbol_name_raw`, then
    /// writes it as a CSV file into `dest` (a directory - the filename is
    /// derived from `symbol`). Returns the path written.
    pub async fn symbol_name_csv(&self, symbol: &str, dest: &Path) -> Result<PathBuf> {
        let row = self.symbol_name_raw(symbol).await?;
        let path = dest.join(format!("{symbol}-name.csv"));
        write_csv(&[row], &path)
    }

    /// Fetches `symbol`'s price change over several trailing windows
    /// (yesterday through 5 years) alongside its benchmark index's change
    /// over the same windows. Returns an empty `Vec` for an unknown
    /// symbol - confirmed live.
    pub async fn yearwise_data_raw(
        &self,
        symbol: &str,
        series: &str,
    ) -> Result<Vec<YearwiseDataRow>> {
        let identifier = format!("{symbol}{series}N");
        let response = self
            .client
            .get(NEXTAPI_URL)
            .query(&[
                ("functionName", "getYearwiseData"),
                ("symbol", identifier.as_str()),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse yearwise data response: {e}")))
    }

    /// Fetches yearwise data the same way as `yearwise_data_raw`, then
    /// writes it as a CSV file into `dest` (a directory - the filename is
    /// derived from `symbol` and `series`). Returns the path written.
    pub async fn yearwise_data_csv(
        &self,
        symbol: &str,
        series: &str,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self.yearwise_data_raw(symbol, series).await?;
        let path = dest.join(format!("{symbol}-{series}-yearwise.csv"));
        write_csv(&rows, &path)
    }
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

    // Real response captured from NSE's NextApi getSymbolData for SBIN,
    // trimmed to one field per sub-object plus what this crate keeps.
    const SAMPLE_STOCK_QUOTE: &str =
        include_str!("../../tests/fixtures/quote/sample_stock_quote.json");

    #[test]
    fn deserializes_real_stock_quote_shape() {
        let parsed: StockQuoteResponse = serde_json::from_str(SAMPLE_STOCK_QUOTE).unwrap();
        let quote = StockQuote::from(parsed.equity_response.into_iter().next().unwrap());

        assert_eq!(quote.symbol, "SBIN");
        assert_eq!(quote.company_name, "State Bank of India");
        assert_eq!(quote.last_price, 996.2);
        assert_eq!(quote.total_market_cap, Some(9195689872213.2));
        assert_eq!(quote.order_book.levels[0].buy_price, 998.0);
        assert_eq!(quote.order_book.levels[0].sell_quantity, 50);
        assert_eq!(quote.order_book.levels[4].buy_price, 0.0);
        assert_eq!(quote.order_book.total_buy_quantity, 100);
    }

    #[test]
    fn stock_quote_handles_null_delivery_fields() {
        const NO_DELIVERY: &str = include_str!("../../tests/fixtures/quote/no_delivery.json");
        let parsed: StockQuoteResponse = serde_json::from_str(NO_DELIVERY).unwrap();
        let quote = StockQuote::from(parsed.equity_response.into_iter().next().unwrap());

        assert_eq!(quote.delivery_quantity, None);
        assert_eq!(quote.delivery_pct, None);
        assert_eq!(quote.total_market_cap, None);
    }

    // Real response captured from NSE's NextApi getSymbolDerivativesData
    // for NIFTY - one futures row (sentinel strike/option-type values).
    const SAMPLE_DERIVATIVE_QUOTE: &str =
        include_str!("../../tests/fixtures/quote/sample_derivative_quote.json");

    #[test]
    fn deserializes_real_derivative_quote_shape_and_trims_padded_strike() {
        let parsed: DerivativeQuoteResponse =
            serde_json::from_str(SAMPLE_DERIVATIVE_QUOTE).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.underlying, "NIFTY");
        assert_eq!(row.instrument_type, "FUTIDX");
        assert_eq!(row.strike_price, 0.0);
        assert_eq!(row.expiry, date(2026, 9, 29));
        assert_eq!(row.change_in_open_interest, -3775);
    }

    #[test]
    fn derivative_quote_empty_data_deserializes_to_empty_vec() {
        let parsed: DerivativeQuoteResponse =
            serde_json::from_str(r#"{"data":[],"timestamp":""}"#).unwrap();
        assert!(parsed.data.is_empty());
    }

    // Confirmed live: NSE formats openInterest/changeinOpenInterest as a
    // JSON float (not a plain integer) for some contracts within the same
    // response that has plain integers for others.
    #[test]
    fn derivative_quote_accepts_float_formatted_open_interest() {
        const FLOAT_OI: &str = include_str!("../../tests/fixtures/quote/float_oi.json");
        let parsed: DerivativeQuoteResponse = serde_json::from_str(FLOAT_OI).unwrap();

        assert_eq!(parsed.data[0].open_interest, 195_178);
        assert_eq!(parsed.data[0].change_in_open_interest, 90_670);
    }

    // Real response captured from NSE's equity-stock-indices API for
    // NIFTY 50.
    const SAMPLE_INDEX_QUOTE: &str =
        include_str!("../../tests/fixtures/quote/sample_index_quote.json");

    #[test]
    fn deserializes_real_index_quote_shape() {
        let parsed: IndexQuoteResponse = serde_json::from_str(SAMPLE_INDEX_QUOTE).unwrap();
        let quote = parsed.data.into_iter().next().unwrap();

        assert_eq!(quote.name, "NIFTY 50");
        assert_eq!(quote.last, 23346.4);
        assert_eq!(quote.total_traded_volume, 375_346_024);
    }

    #[test]
    fn index_quote_empty_data_has_no_row() {
        let parsed: IndexQuoteResponse =
            serde_json::from_str(r#"{"data":[],"marketStatus":{}}"#).unwrap();
        assert!(parsed.data.into_iter().next().is_none());
    }

    // Real response captured from NSE's option-chain-v3 API for NIFTY.
    const SAMPLE_OPTION_CHAIN: &str =
        include_str!("../../tests/fixtures/quote/sample_option_chain.json");

    #[test]
    fn deserializes_real_option_chain_shape() {
        let parsed: OptionChainResponse<OptionChainRow> =
            serde_json::from_str(SAMPLE_OPTION_CHAIN).unwrap();
        let rows = parsed.records.unwrap().data;

        assert_eq!(rows[0].strike_price, 21350.0);
        assert_eq!(rows[0].expiry, date(2026, 9, 22));
        assert_eq!(rows[0].call.as_ref().unwrap().buy_price, 1900.75);
        assert_eq!(rows[0].put.as_ref().unwrap().open_interest, 97418);
    }

    // Confirmed live on deep SBIN equity strikes: a leg with no real
    // contract still comes back as a `CE`/`PE` object (not omitted), but
    // with a null identifier and every numeric field zeroed out.
    #[test]
    fn option_leg_with_no_real_contract_has_null_identifier() {
        const NO_CONTRACT_LEG: &str =
            include_str!("../../tests/fixtures/quote/no_contract_leg.json");
        let parsed: OptionChainResponse<OptionChainRow> =
            serde_json::from_str(NO_CONTRACT_LEG).unwrap();
        let row = &parsed.records.unwrap().data[0];

        assert!(row.call.as_ref().unwrap().identifier.is_some());
        assert!(row.put.as_ref().unwrap().identifier.is_none());
    }

    // Confirmed live: the real NIFTY index option chain (128 strikes) had
    // at least one leg with `changeinOpenInterest` as a JSON float
    // (`48608.769230769234`) rather than an integer - a plain `i64` fails
    // to deserialize it, breaking the whole response even though every
    // other strike in the same response was fine. Same class of bug
    // `DerivativeQuoteRow` already guards against on a different endpoint
    // (see `deserialize_lenient_u64`/`_i64`), just not caught here until a
    // full live NIFTY fetch happened to hit the affected strike - a
    // hand-picked small sample never would have.
    #[test]
    fn option_leg_accepts_float_formatted_change_in_open_interest() {
        const FLOAT_CHANGE_IN_OI: &str =
            include_str!("../../tests/fixtures/quote/float_change_in_oi.json");
        let parsed: OptionChainResponse<OptionChainRow> =
            serde_json::from_str(FLOAT_CHANGE_IN_OI).unwrap();
        let row = &parsed.records.unwrap().data[0];

        assert_eq!(row.call.as_ref().unwrap().change_in_open_interest, 48609);
    }

    // A full real NIFTY option chain response (138 strikes), captured live
    // specifically because the small hand-picked sample above didn't
    // contain a float-formatted `changeinOpenInterest` and let that bug
    // through. Deserializing the whole thing, not just one row, is the
    // actual regression guard - a smaller/different sample could just as
    // easily miss the next edge case the way this one did.
    const LARGE_OPTION_CHAIN_NIFTY: &str =
        include_str!("../../tests/fixtures/quote/large_option_chain_nifty.json");

    #[test]
    fn deserializes_full_real_nifty_option_chain_response() {
        let parsed: OptionChainResponse<OptionChainRow> =
            serde_json::from_str(LARGE_OPTION_CHAIN_NIFTY).unwrap();
        let rows = parsed.records.unwrap().data;

        assert_eq!(rows.len(), 138);
        assert!(rows.iter().all(|r| r.call.is_some() || r.put.is_some()));
    }

    #[test]
    fn option_chain_missing_records_key_is_treated_as_empty() {
        let parsed: OptionChainResponse<OptionChainRow> = serde_json::from_str("{}").unwrap();
        assert!(parsed.records.is_none());
    }

    // Real response captured from NSE's option-chain-currency API for
    // USDINR - note the different leg shape (bidprice/askPrice, not
    // buyPrice1/sellPrice1) and the singular "expiryDate" key.
    const SAMPLE_CURRENCY_OPTION_CHAIN: &str =
        include_str!("../../tests/fixtures/quote/sample_currency_option_chain.json");

    #[test]
    fn deserializes_real_currency_option_chain_shape() {
        let parsed: OptionChainResponse<CurrencyOptionChainRow> =
            serde_json::from_str(SAMPLE_CURRENCY_OPTION_CHAIN).unwrap();
        let rows = parsed.records.unwrap().data;

        assert_eq!(rows[0].expiry, date(2026, 9, 28));
        assert_eq!(rows[0].put.as_ref().unwrap().ask_price, 0.0);
        assert_eq!(rows[0].call.as_ref().unwrap().underlying_value, 95.791);
    }

    // Real response captured from NSE's option-chain-contract-info API.
    #[test]
    fn contract_info_missing_key_defaults_to_empty() {
        let parsed: ContractInfo = serde_json::from_str("{}").unwrap();
        assert!(parsed.expiry_dates.is_empty());
    }

    #[test]
    fn contract_info_deserializes_expiry_dates() {
        let parsed: ContractInfo =
            serde_json::from_str(r#"{"expiryDates":["22-Sep-2026","29-Sep-2026"]}"#).unwrap();
        assert_eq!(parsed.expiry_dates, vec!["22-Sep-2026", "29-Sep-2026"]);
    }

    // Real response captured live from NSE's NextApi getSymbolChartData for
    // SBIN with days=1D - one pre-open point, one normal-market point.
    const SAMPLE_CHART_DATA_1D: &str =
        include_str!("../../tests/fixtures/quote/sample_chart_data_1d.json");

    #[test]
    fn deserializes_real_chart_data_shape() {
        let raw: RawChartData = serde_json::from_str(SAMPLE_CHART_DATA_1D).unwrap();
        let points: Vec<ChartDataPoint> = raw
            .graph_data
            .into_iter()
            .map(chart_data_point_from_tuple)
            .collect::<Result<_>>()
            .unwrap();

        assert_eq!(raw.identifier, "SBINEQN");
        assert_eq!(raw.close_price, 996.2);
        assert_eq!(points[0].session, "PO");
        assert_eq!(points[0].price, 996.0);
        assert_eq!(points[0].change, Some(-0.2));
        assert_eq!(points[0].percent_change, Some(-0.02));
        assert_eq!(points[1].session, "NM");
    }

    // Real response captured live for days=1W - only daily closes, no
    // intraday change/percent_change (both null).
    const SAMPLE_CHART_DATA_1W: &str =
        include_str!("../../tests/fixtures/quote/sample_chart_data_1w.json");

    #[test]
    fn chart_data_1w_has_no_change_or_percent_change() {
        let raw: RawChartData = serde_json::from_str(SAMPLE_CHART_DATA_1W).unwrap();
        let points: Vec<ChartDataPoint> = raw
            .graph_data
            .into_iter()
            .map(chart_data_point_from_tuple)
            .collect::<Result<_>>()
            .unwrap();

        assert_eq!(points[0].change, None);
        assert_eq!(points[0].percent_change, None);
    }

    // Confirmed live: NSE builds this epoch from IST wall-clock digits
    // rather than a genuine UTC instant (see `ist_timestamp_from_millis`).
    // 1789981259000ms decodes as UTC 2026-09-21 09:00:59, which is the
    // real IST wall-clock reading, not the real UTC one.
    #[test]
    fn chart_data_timestamp_is_read_as_ist_wall_clock() {
        let ts = ist_timestamp_from_millis(1_789_981_259_000).unwrap();
        assert_eq!(ts.to_string(), "2026-09-21 09:00:59");
    }

    // Real response captured live for SBIN.
    const SAMPLE_REG_DETAILS: &str =
        include_str!("../../tests/fixtures/quote/sample_reg_details.json");

    #[test]
    fn deserializes_real_reg_details_shape() {
        let rows: Vec<RegDetailsRow> = serde_json::from_str(SAMPLE_REG_DETAILS).unwrap();
        assert_eq!(rows[0].symbol, "SBIN");
        assert_eq!(rows[0].reg_action, None);
    }

    // Real response captured live for SBIN - `is*`/`casFlag` fields are
    // JSON strings ("true"/"false"), not real JSON booleans.
    const SAMPLE_SYMBOL_META: &str =
        include_str!("../../tests/fixtures/quote/sample_symbol_meta.json");

    #[test]
    fn deserializes_real_symbol_meta_shape_with_string_booleans() {
        let row: SymbolMetaRow = serde_json::from_str(SAMPLE_SYMBOL_META).unwrap();
        assert_eq!(row.symbol, "SBIN");
        assert!(row.is_fno_eligible);
        assert!(!row.is_corporate_action_sec);
        assert_eq!(row.active_series, vec!["EQ", "T0"]);
    }

    // Confirmed live: an unknown symbol returns HTTP 200 with every field
    // null instead of a 404.
    #[test]
    fn symbol_meta_all_null_response_is_not_the_strict_shape() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"symbol":null,"activeSeries":null,"companyName":null,"debtSeries":null,
                "isFNOSec":null,"isCASec":null,"isSLBSec":null,"isDebtSec":null,
                "tempSuspendedSeries":null,"isSuspended":null,"isETFSec":null,
                "isDelisted":null,"isin":null,"isMunicipalBond":null,"isHybridSymbol":null,
                "marketType":null,"parentSymbol":null,"casFlag":null}"#,
        )
        .unwrap();
        assert!(!value.get("symbol").is_some_and(|v| v.is_string()));
    }

    #[test]
    fn deserializes_real_symbol_name_shape() {
        let row: SymbolNameRow =
            serde_json::from_str(r#"{"symbol":"SBIN","companyName":"State Bank of India"}"#)
                .unwrap();
        assert_eq!(row.company_name, "State Bank of India");
    }

    // Real response shape captured live for SBIN - note the two-digit
    // year date format, unlike every other date field in this crate.
    const SAMPLE_YEARWISE: &str = include_str!("../../tests/fixtures/quote/sample_yearwise.json");

    #[test]
    fn deserializes_real_yearwise_data_shape_with_short_year_date() {
        let rows: Vec<YearwiseDataRow> = serde_json::from_str(SAMPLE_YEARWISE).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].one_week_date, date(2026, 9, 16));
        assert_eq!(rows[0].index_name, "NIFTY 50");
    }

    // Real response captured live from `getGraphChart` for NIFTY 50,
    // days=1D - note change/percent_change are real numbers here, unlike
    // `stock_chart_data_raw`'s 1D shape (which sends them as strings).
    const SAMPLE_INDEX_CHART_1D: &str =
        include_str!("../../tests/fixtures/quote/sample_index_chart_1d.json");

    #[test]
    fn deserializes_real_index_chart_1d_shape() {
        let parsed: IndexChartResponse = serde_json::from_str(SAMPLE_INDEX_CHART_1D).unwrap();
        let points: Vec<IndexChartDataPoint> = parsed
            .data
            .graph_data
            .into_iter()
            .map(index_chart_data_point_from_tuple)
            .collect::<Result<_>>()
            .unwrap();

        assert_eq!(parsed.data.identifier, "NIFTY 50");
        assert_eq!(points[0].session, "PO");
        assert_eq!(points[1].change, 117.8);
        assert_eq!(points[1].percent_change, 0.5);
        assert_eq!(
            points[1].timestamp,
            date(2026, 9, 23).and_hms_opt(15, 39, 59).unwrap()
        );
    }

    // Real response captured live for days=1W - non-1D windows send a
    // literal 0/0 for change/percent_change, not null.
    const SAMPLE_INDEX_CHART_1W: &str =
        include_str!("../../tests/fixtures/quote/sample_index_chart_1w.json");

    #[test]
    fn index_chart_1w_uses_zero_not_null_for_change() {
        let parsed: IndexChartResponse = serde_json::from_str(SAMPLE_INDEX_CHART_1W).unwrap();
        let points: Vec<IndexChartDataPoint> = parsed
            .data
            .graph_data
            .into_iter()
            .map(index_chart_data_point_from_tuple)
            .collect::<Result<_>>()
            .unwrap();

        assert_eq!(points[0].change, 0.0);
        assert_eq!(points[0].percent_change, 0.0);
    }
}
