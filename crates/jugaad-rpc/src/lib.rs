use std::cmp::Reverse;

use chrono::{NaiveDate, NaiveDateTime};
use jugaad_core::nse::{
    ChartData as CoreChartData, ChartPeriod as CoreChartPeriod,
    CorporateAnnouncementRow as CoreCorporateAnnouncementRow,
    DerivativeHistoryRow as CoreDerivativeHistoryRow, FiftyTwoWeekRow as CoreFiftyTwoWeekRow,
    HolidayRow as CoreHolidayRow, IndexChartData as CoreIndexChartData,
    IndexChartPeriod as CoreIndexChartPeriod, IndexHistoryRow as CoreIndexHistoryRow,
    IndexSnapshotRow as CoreIndexSnapshotRow, Instrument as CoreInstrument,
    LargeDealRow as CoreLargeDealRow, MarketMoverRow as CoreMarketMoverRow,
    MarketSegmentStatus as CoreMarketSegmentStatus, MostActiveEquityRow as CoreMostActiveEquityRow,
    NseArchives, NseCorporateAnnouncements, NseHistory, NseIndexHistory, NseLiveMarket, NseQuote,
    OptionChainKind as CoreOptionChainKind, OptionChainRow as CoreOptionChainRow,
    OptionLeg as CoreOptionLeg, OptionType as CoreOptionType, OrderBook as CoreOrderBook,
    StockHistoryRow as CoreStockHistoryRow, StockQuote as CoreStockQuote,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub mod jugaad {
    tonic::include_proto!("jugaad");
}

use jugaad::jugaad_server::Jugaad;
pub use jugaad::jugaad_server::JugaadServer;
use jugaad::{
    BhavcopyRequest, BhavcopyResponse, ChartPeriod, CorporateAnnouncementRow,
    CorporateAnnouncementsRequest, CorporateAnnouncementsResponse, CsvChunk,
    DerivativesHistoryRequest, DerivativesHistoryResponse, DerivativesHistoryRow,
    FiftyTwoWeekRequest, FiftyTwoWeekResponse, FiftyTwoWeekRow, HolidayListRequest,
    HolidayListResponse, HolidayRow, IndexChart, IndexChartPeriod, IndexChartPoint,
    IndexChartRequest, IndexHistoryRequest, IndexHistoryResponse, IndexHistoryRow,
    IndexSnapshotRequest, IndexSnapshotResponse, IndexSnapshotRow, Instrument, LargeDealRow,
    LargeDealsRequest, LargeDealsResponse, MarketMoverRow, MarketMoversRequest,
    MarketMoversResponse, MarketSegmentStatus, MarketStatusRequest, MarketStatusResponse,
    MostActiveEquitiesRequest, MostActiveEquitiesResponse, MostActiveEquityRow, OptionChainKind,
    OptionChainRequest, OptionChainResponse, OptionChainRow, OptionExpiriesRequest,
    OptionExpiriesResponse, OptionLeg, OptionType, OrderBook, OrderBookLevel, StockChart,
    StockChartPoint, StockChartRequest, StockHistoryRequest, StockHistoryResponse, StockHistoryRow,
    StockQuote, StockQuoteRequest, WatchStockQuoteRequest,
};

const DEFAULT_WATCH_INTERVAL_SECS: u64 = 3;

// Well under gRPC's default 4 MiB message cap, so any client can receive a
// bhavcopy stream without raising its limits.
const CSV_CHUNK_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub struct JugaadService {
    archives: NseArchives,
    quote: NseQuote,
    history: NseHistory,
    index_history: NseIndexHistory,
    live_market: NseLiveMarket,
    corporate_announcements: NseCorporateAnnouncements,
}

impl JugaadService {
    pub fn new() -> jugaad_core::Result<Self> {
        Ok(Self {
            archives: NseArchives::new()?,
            quote: NseQuote::new()?,
            history: NseHistory::new()?,
            index_history: NseIndexHistory::new()?,
            live_market: NseLiveMarket::new()?,
            corporate_announcements: NseCorporateAnnouncements::new()?,
        })
    }
}

fn to_status(err: jugaad_core::Error) -> Status {
    match err {
        jugaad_core::Error::NotFound(msg) => Status::not_found(msg),
        jugaad_core::Error::NoData => Status::not_found(err.to_string()),
        jugaad_core::Error::Blocked => Status::unavailable(err.to_string()),
        other => Status::internal(other.to_string()),
    }
}

/// Every date field on this service's requests/responses is a plain
/// "YYYY-MM-DD" string, regardless of whatever wire format the underlying
/// NSE/niftyindices endpoint actually wants - jugaad-core already handles
/// that translation internally (see e.g. `corporate_announcements_raw`
/// formatting its own dates as `%d-%m-%Y` for NSE).
fn parse_date(s: &str, field: &str) -> Result<NaiveDate, Status> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
        Status::invalid_argument(format!("{field}: expected YYYY-MM-DD, got {s:?} ({e})"))
    })
}

fn order_book_to_proto(book: CoreOrderBook) -> OrderBook {
    OrderBook {
        levels: book
            .levels
            .into_iter()
            .map(|l| OrderBookLevel {
                buy_price: l.buy_price,
                buy_quantity: l.buy_quantity,
                sell_price: l.sell_price,
                sell_quantity: l.sell_quantity,
            })
            .collect(),
        total_buy_quantity: book.total_buy_quantity,
        total_sell_quantity: book.total_sell_quantity,
    }
}

fn quote_to_proto(q: CoreStockQuote) -> StockQuote {
    StockQuote {
        symbol: q.symbol,
        company_name: q.company_name,
        series: q.series,
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
        order_book: Some(order_book_to_proto(q.order_book)),
        last_update_time: q.last_update_time,
    }
}

fn stock_history_row_to_proto(r: CoreStockHistoryRow) -> StockHistoryRow {
    StockHistoryRow {
        symbol: r.symbol,
        series: r.series,
        date: r.date.format("%Y-%m-%d").to_string(),
        open: r.open,
        high: r.high,
        low: r.low,
        previous_close: r.prev_close,
        last_traded_price: r.ltp,
        close: r.close,
        vwap: r.vwap,
        volume: r.volume,
        value: r.value,
        trades: r.trades,
        delivery_quantity: r.delivery_qty,
        delivery_pct: r.delivery_pct,
    }
}

fn index_history_row_to_proto(r: CoreIndexHistoryRow) -> IndexHistoryRow {
    IndexHistoryRow {
        index_name: r.index_name,
        date: r.date.format("%Y-%m-%d").to_string(),
        open: r.open,
        high: r.high,
        low: r.low,
        close: r.close,
    }
}

fn index_snapshot_row_to_proto(r: CoreIndexSnapshotRow) -> IndexSnapshotRow {
    IndexSnapshotRow {
        category: r.category,
        name: r.name,
        symbol: r.symbol,
        last: r.last,
        change: r.change,
        percent_change: r.percent_change,
        open: r.open,
        high: r.high,
        low: r.low,
        previous_close: r.prev_close,
        year_high: r.year_high,
        year_low: r.year_low,
        pe: r.pe,
        pb: r.pb,
        div_yield: r.div_yield,
        advances: r.advances,
        declines: r.declines,
        unchanged: r.unchanged,
    }
}

fn large_deal_row_to_proto(r: CoreLargeDealRow) -> LargeDealRow {
    LargeDealRow {
        deal_type: r.deal_type,
        symbol: r.symbol,
        company_name: r.company_name,
        client_name: r.client_name,
        buy_sell: r.buy_sell,
        quantity: r.quantity,
        weighted_avg_price: r.weighted_avg_price,
        remarks: r.remarks,
        date: r.date.format("%Y-%m-%d").to_string(),
    }
}

fn market_segment_status_to_proto(r: CoreMarketSegmentStatus) -> MarketSegmentStatus {
    MarketSegmentStatus {
        market: r.market,
        status: r.status,
        trade_date: r.trade_date,
        index: r.index,
        last: r.last,
        change: r.change,
        percent_change: r.percent_change,
        status_message: r.status_message,
    }
}

fn option_leg_to_proto(l: CoreOptionLeg) -> OptionLeg {
    OptionLeg {
        identifier: l.identifier,
        last_price: l.last_price,
        change: l.change,
        percent_change: l.percent_change,
        open_interest: l.open_interest,
        change_in_open_interest: l.change_in_open_interest,
        percent_change_in_open_interest: l.percent_change_in_open_interest,
        total_traded_volume: l.total_traded_volume,
        implied_volatility: l.implied_volatility,
        buy_price: l.buy_price,
        buy_quantity: l.buy_quantity,
        sell_price: l.sell_price,
        sell_quantity: l.sell_quantity,
        total_buy_quantity: l.total_buy_quantity,
        total_sell_quantity: l.total_sell_quantity,
        underlying_value: l.underlying_value,
    }
}

fn option_chain_row_to_proto(r: CoreOptionChainRow) -> OptionChainRow {
    OptionChainRow {
        strike_price: r.strike_price,
        expiry: r.expiry.format("%Y-%m-%d").to_string(),
        call: r.call.map(option_leg_to_proto),
        put: r.put.map(option_leg_to_proto),
    }
}

fn corporate_announcement_row_to_proto(
    r: CoreCorporateAnnouncementRow,
) -> CorporateAnnouncementRow {
    CorporateAnnouncementRow {
        symbol: r.symbol,
        isin: r.isin,
        company_name: r.company_name,
        category: r.category,
        description: r.description,
        industry: r.industry,
        has_xbrl: r.has_xbrl,
        attachment_url: r.attachment_url,
        file_size: r.file_size,
        // NSE's own timestamp is naive (implicitly IST) - no timezone to
        // encode here without guessing, so this stays a plain local
        // datetime string rather than a false UTC "Z" suffix.
        announcement_time: r.announcement_time.format("%Y-%m-%dT%H:%M:%S").to_string(),
        sequence_id: r.sequence_id,
    }
}

fn date_string(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

// NSE's chart timestamps are naive IST wall-clock times, so no "Z" suffix.
fn datetime_string(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn derivative_history_row_to_proto(r: CoreDerivativeHistoryRow) -> DerivativesHistoryRow {
    DerivativesHistoryRow {
        instrument: r.instrument,
        symbol: r.symbol,
        expiry: date_string(r.expiry),
        strike_price: r.strike_price,
        option_type: r.option_type,
        date: date_string(r.date),
        open: r.open,
        high: r.high,
        low: r.low,
        close: r.close,
        last_traded_price: r.ltp,
        previous_close: r.prev_close,
        settle_price: r.settle_price,
        volume: r.volume,
        value: r.value,
        open_interest: r.open_interest,
        change_in_open_interest: r.change_in_oi,
        market_lot: r.market_lot,
        underlying_value: r.underlying_value,
    }
}

/// Futures carry no strike or option side and options must carry both, so a
/// request that mixes them up is rejected here instead of silently ignored.
fn instrument_from_request(req: &DerivativesHistoryRequest) -> Result<CoreInstrument, Status> {
    let option_type = OptionType::try_from(req.option_type).unwrap_or(OptionType::Unspecified);
    let option = || -> Result<(f64, CoreOptionType), Status> {
        let strike_price = req.strike_price.ok_or_else(|| {
            Status::invalid_argument("strike_price: required for an option instrument")
        })?;
        let option_type = match option_type {
            OptionType::Call => CoreOptionType::Call,
            OptionType::Put => CoreOptionType::Put,
            OptionType::Unspecified => {
                return Err(Status::invalid_argument(
                    "option_type: required for an option instrument (CALL or PUT)",
                ));
            }
        };
        Ok((strike_price, option_type))
    };
    let no_option = || -> Result<(), Status> {
        if req.strike_price.is_some() || option_type != OptionType::Unspecified {
            return Err(Status::invalid_argument(
                "strike_price and option_type only apply to option instruments",
            ));
        }
        Ok(())
    };
    match Instrument::try_from(req.instrument).unwrap_or(Instrument::Unspecified) {
        Instrument::Unspecified => Err(Status::invalid_argument(
            "instrument: required (FUT_IDX, FUT_STK, OPT_IDX or OPT_STK)",
        )),
        Instrument::FutIdx => no_option().map(|()| CoreInstrument::FutIdx),
        Instrument::FutStk => no_option().map(|()| CoreInstrument::FutStk),
        Instrument::OptIdx => option().map(|(strike_price, option_type)| CoreInstrument::OptIdx {
            strike_price,
            option_type,
        }),
        Instrument::OptStk => option().map(|(strike_price, option_type)| CoreInstrument::OptStk {
            strike_price,
            option_type,
        }),
    }
}

fn chart_period_from_proto(value: i32) -> CoreChartPeriod {
    match ChartPeriod::try_from(value).unwrap_or(ChartPeriod::Unspecified) {
        ChartPeriod::Unspecified | ChartPeriod::OneDay => CoreChartPeriod::OneDay,
        ChartPeriod::OneWeek => CoreChartPeriod::OneWeek,
        ChartPeriod::OneMonth => CoreChartPeriod::OneMonth,
        ChartPeriod::OneYear => CoreChartPeriod::OneYear,
        ChartPeriod::FiveYears => CoreChartPeriod::FiveYears,
    }
}

fn index_chart_period_from_proto(value: i32) -> CoreIndexChartPeriod {
    match IndexChartPeriod::try_from(value).unwrap_or(IndexChartPeriod::Unspecified) {
        IndexChartPeriod::Unspecified | IndexChartPeriod::OneDay => CoreIndexChartPeriod::OneDay,
        IndexChartPeriod::OneWeek => CoreIndexChartPeriod::OneWeek,
        IndexChartPeriod::OneMonth => CoreIndexChartPeriod::OneMonth,
        IndexChartPeriod::ThreeMonths => CoreIndexChartPeriod::ThreeMonths,
        IndexChartPeriod::SixMonths => CoreIndexChartPeriod::SixMonths,
        IndexChartPeriod::OneYear => CoreIndexChartPeriod::OneYear,
        IndexChartPeriod::FiveYears => CoreIndexChartPeriod::FiveYears,
    }
}

// NSE sends stock charts oldest-first for 1D but newest-first for every longer
// window, and index charts oldest-first, so the points are sorted here rather
// than trusted: every chart this service returns is newest first.
fn stock_chart_to_proto(c: CoreChartData) -> StockChart {
    let mut points = c.points;
    points.sort_by_key(|p| Reverse(p.timestamp));
    StockChart {
        identifier: c.identifier,
        name: c.name,
        close_price: c.close_price,
        points: points
            .into_iter()
            .map(|p| StockChartPoint {
                timestamp: datetime_string(p.timestamp),
                price: p.price,
                session: p.session,
                change: p.change,
                percent_change: p.percent_change,
            })
            .collect(),
    }
}

fn index_chart_to_proto(c: CoreIndexChartData) -> IndexChart {
    let mut points = c.points;
    points.sort_by_key(|p| Reverse(p.timestamp));
    IndexChart {
        identifier: c.identifier,
        name: c.name,
        close_price: c.close_price,
        points: points
            .into_iter()
            .map(|p| IndexChartPoint {
                timestamp: datetime_string(p.timestamp),
                price: p.price,
                session: p.session,
                change: p.change,
                percent_change: p.percent_change,
            })
            .collect(),
    }
}

fn market_mover_row_to_proto(r: CoreMarketMoverRow) -> MarketMoverRow {
    MarketMoverRow {
        scope: r.scope,
        direction: r.direction,
        symbol: r.symbol,
        series: r.series,
        open: r.open,
        high: r.high,
        low: r.low,
        last_price: r.last_price,
        previous_close: r.previous_close,
        change: r.change,
        percent_change: r.percent_change,
        traded_quantity: r.traded_quantity,
        turnover: r.turnover,
        market_type: r.market_type,
        ca_ex_date: r.ca_ex_date,
        ca_purpose: r.ca_purpose,
    }
}

fn most_active_equity_row_to_proto(r: CoreMostActiveEquityRow) -> MostActiveEquityRow {
    MostActiveEquityRow {
        ranking: r.ranking,
        symbol: r.symbol,
        identifier: r.identifier,
        last_price: r.last_price,
        percent_change: r.percent_change,
        quantity_traded: r.quantity_traded,
        total_traded_volume: r.total_traded_volume,
        total_traded_value: r.total_traded_value,
        previous_close: r.previous_close,
        ex_date: r.ex_date,
        purpose: r.purpose,
        year_high: r.year_high,
        year_low: r.year_low,
        change: r.change,
        open: r.open,
        day_high: r.day_high,
        day_low: r.day_low,
        last_update_time: r.last_update_time,
    }
}

fn fifty_two_week_row_to_proto(r: CoreFiftyTwoWeekRow) -> FiftyTwoWeekRow {
    FiftyTwoWeekRow {
        direction: r.direction,
        symbol: r.symbol,
        company_name: r.company_name,
        series: r.series,
        last_price: r.last_price,
        change: r.change,
        percent_change: r.percent_change,
        new_52_week_value: r.new_52_week_value,
        previous_52_week_value: r.previous_52_week_value,
        previous_close: r.previous_close,
        previous_52_week_date: r.previous_52_week_date.map(date_string),
    }
}

fn holiday_row_to_proto(r: CoreHolidayRow) -> HolidayRow {
    HolidayRow {
        segment: r.segment,
        date: date_string(r.date),
        week_day: r.week_day,
        description: r.description,
        morning_session: r.morning_session,
        evening_session: r.evening_session,
        serial_number: r.serial_number,
    }
}

#[tonic::async_trait]
impl Jugaad for JugaadService {
    async fn get_stock_quote(
        &self,
        request: Request<StockQuoteRequest>,
    ) -> Result<Response<StockQuote>, Status> {
        let symbol = request.into_inner().symbol;
        let quote = self
            .quote
            .stock_quote_raw(&symbol)
            .await
            .map_err(to_status)?;
        Ok(Response::new(quote_to_proto(quote)))
    }

    type WatchStockQuoteStream = ReceiverStream<Result<StockQuote, Status>>;

    async fn watch_stock_quote(
        &self,
        request: Request<WatchStockQuoteRequest>,
    ) -> Result<Response<Self::WatchStockQuoteStream>, Status> {
        let req = request.into_inner();
        let interval_secs = if req.interval_seconds == 0 {
            DEFAULT_WATCH_INTERVAL_SECS
        } else {
            u64::from(req.interval_seconds)
        };
        let quote_client = self.quote.clone();
        let (tx, rx) = mpsc::channel(4);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                ticker.tick().await;
                let result = quote_client
                    .stock_quote_raw(&req.symbol)
                    .await
                    .map(quote_to_proto)
                    .map_err(to_status);
                if tx.send(result).await.is_err() {
                    // Client disconnected - stop polling NSE for it.
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_stock_history(
        &self,
        request: Request<StockHistoryRequest>,
    ) -> Result<Response<StockHistoryResponse>, Status> {
        let req = request.into_inner();
        let from_date = parse_date(&req.from_date, "from_date")?;
        let to_date = parse_date(&req.to_date, "to_date")?;
        let series = req.series.as_deref().unwrap_or("ALL");
        let rows = self
            .history
            .stock_history_raw(&req.symbol, from_date, to_date, series)
            .await
            .map_err(to_status)?;
        Ok(Response::new(StockHistoryResponse {
            rows: rows.into_iter().map(stock_history_row_to_proto).collect(),
        }))
    }

    async fn get_index_history(
        &self,
        request: Request<IndexHistoryRequest>,
    ) -> Result<Response<IndexHistoryResponse>, Status> {
        let req = request.into_inner();
        let from_date = parse_date(&req.from_date, "from_date")?;
        let to_date = parse_date(&req.to_date, "to_date")?;
        let rows = self
            .index_history
            .index_history_raw(&req.name, from_date, to_date)
            .await
            .map_err(to_status)?;
        Ok(Response::new(IndexHistoryResponse {
            rows: rows.into_iter().map(index_history_row_to_proto).collect(),
        }))
    }

    async fn get_index_snapshot(
        &self,
        _request: Request<IndexSnapshotRequest>,
    ) -> Result<Response<IndexSnapshotResponse>, Status> {
        let rows = self
            .live_market
            .index_snapshot_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(IndexSnapshotResponse {
            rows: rows.into_iter().map(index_snapshot_row_to_proto).collect(),
        }))
    }

    async fn get_large_deals(
        &self,
        _request: Request<LargeDealsRequest>,
    ) -> Result<Response<LargeDealsResponse>, Status> {
        let rows = self
            .live_market
            .large_deals_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(LargeDealsResponse {
            rows: rows.into_iter().map(large_deal_row_to_proto).collect(),
        }))
    }

    async fn get_market_status(
        &self,
        _request: Request<MarketStatusRequest>,
    ) -> Result<Response<MarketStatusResponse>, Status> {
        let segments = self
            .live_market
            .market_status_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(MarketStatusResponse {
            segments: segments
                .into_iter()
                .map(market_segment_status_to_proto)
                .collect(),
        }))
    }

    async fn get_option_chain(
        &self,
        request: Request<OptionChainRequest>,
    ) -> Result<Response<OptionChainResponse>, Status> {
        let req = request.into_inner();
        let expiry = req
            .expiry
            .as_deref()
            .map(|s| parse_date(s, "expiry"))
            .transpose()?;
        let kind = match OptionChainKind::try_from(req.kind).unwrap_or(OptionChainKind::Unspecified)
        {
            OptionChainKind::Equity => CoreOptionChainKind::Equity,
            OptionChainKind::Index | OptionChainKind::Unspecified => CoreOptionChainKind::Index,
        };
        let rows = self
            .quote
            .option_chain_raw(&req.symbol, kind, expiry)
            .await
            .map_err(to_status)?;
        Ok(Response::new(OptionChainResponse {
            rows: rows.into_iter().map(option_chain_row_to_proto).collect(),
        }))
    }

    async fn get_option_expiries(
        &self,
        request: Request<OptionExpiriesRequest>,
    ) -> Result<Response<OptionExpiriesResponse>, Status> {
        let symbol = request.into_inner().symbol;
        let expiries = self
            .quote
            .option_expiries_raw(&symbol)
            .await
            .map_err(to_status)?;
        Ok(Response::new(OptionExpiriesResponse {
            expiries: expiries
                .into_iter()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .collect(),
        }))
    }

    async fn get_corporate_announcements(
        &self,
        request: Request<CorporateAnnouncementsRequest>,
    ) -> Result<Response<CorporateAnnouncementsResponse>, Status> {
        let req = request.into_inner();
        let from_date = parse_date(&req.from_date, "from_date")?;
        let to_date = parse_date(&req.to_date, "to_date")?;
        let rows = self
            .corporate_announcements
            .corporate_announcements_raw(&req.segment, req.symbol.as_deref(), from_date, to_date)
            .await
            .map_err(to_status)?;
        Ok(Response::new(CorporateAnnouncementsResponse {
            rows: rows
                .into_iter()
                .map(corporate_announcement_row_to_proto)
                .collect(),
        }))
    }

    async fn get_bhavcopy(
        &self,
        request: Request<BhavcopyRequest>,
    ) -> Result<Response<BhavcopyResponse>, Status> {
        let date = parse_date(&request.into_inner().date, "date")?;
        let csv = self.archives.bhavcopy_raw(date).await.map_err(to_status)?;
        Ok(Response::new(BhavcopyResponse { csv }))
    }

    type GetFoBhavcopyStream = ReceiverStream<Result<CsvChunk, Status>>;

    async fn get_fo_bhavcopy(
        &self,
        request: Request<BhavcopyRequest>,
    ) -> Result<Response<Self::GetFoBhavcopyStream>, Status> {
        let date = parse_date(&request.into_inner().date, "date")?;
        // Fetched before the stream opens, so "no data for this date" arrives
        // as an ordinary error status rather than a stream that dies midway.
        let csv = self
            .archives
            .bhavcopy_fo_raw(date)
            .await
            .map_err(to_status)?;
        let (tx, rx) = mpsc::channel(4);
        tokio::spawn(async move {
            for piece in csv.as_bytes().chunks(CSV_CHUNK_BYTES) {
                let chunk = CsvChunk {
                    data: piece.to_vec(),
                };
                if tx.send(Ok(chunk)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_derivatives_history(
        &self,
        request: Request<DerivativesHistoryRequest>,
    ) -> Result<Response<DerivativesHistoryResponse>, Status> {
        let req = request.into_inner();
        let from_date = parse_date(&req.from_date, "from_date")?;
        let to_date = parse_date(&req.to_date, "to_date")?;
        let expiry = parse_date(&req.expiry, "expiry")?;
        let instrument = instrument_from_request(&req)?;
        let rows = self
            .history
            .derivatives_history_raw(&req.symbol, from_date, to_date, expiry, instrument)
            .await
            .map_err(to_status)?;
        Ok(Response::new(DerivativesHistoryResponse {
            rows: rows
                .into_iter()
                .map(derivative_history_row_to_proto)
                .collect(),
        }))
    }

    async fn get_stock_chart(
        &self,
        request: Request<StockChartRequest>,
    ) -> Result<Response<StockChart>, Status> {
        let req = request.into_inner();
        let chart = self
            .quote
            .stock_chart_data_raw(&req.symbol, chart_period_from_proto(req.period))
            .await
            .map_err(to_status)?;
        Ok(Response::new(stock_chart_to_proto(chart)))
    }

    async fn get_index_chart(
        &self,
        request: Request<IndexChartRequest>,
    ) -> Result<Response<IndexChart>, Status> {
        let req = request.into_inner();
        let chart = self
            .quote
            .index_chart_data_raw(&req.name, index_chart_period_from_proto(req.period))
            .await
            .map_err(to_status)?;
        Ok(Response::new(index_chart_to_proto(chart)))
    }

    async fn get_market_movers(
        &self,
        _request: Request<MarketMoversRequest>,
    ) -> Result<Response<MarketMoversResponse>, Status> {
        let rows = self
            .live_market
            .market_movers_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(MarketMoversResponse {
            rows: rows.into_iter().map(market_mover_row_to_proto).collect(),
        }))
    }

    async fn get_most_active_equities(
        &self,
        _request: Request<MostActiveEquitiesRequest>,
    ) -> Result<Response<MostActiveEquitiesResponse>, Status> {
        let rows = self
            .live_market
            .most_active_equities_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(MostActiveEquitiesResponse {
            rows: rows
                .into_iter()
                .map(most_active_equity_row_to_proto)
                .collect(),
        }))
    }

    async fn get_fifty_two_week(
        &self,
        _request: Request<FiftyTwoWeekRequest>,
    ) -> Result<Response<FiftyTwoWeekResponse>, Status> {
        let rows = self
            .live_market
            .fifty_two_week_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(FiftyTwoWeekResponse {
            rows: rows.into_iter().map(fifty_two_week_row_to_proto).collect(),
        }))
    }

    async fn get_holiday_list(
        &self,
        _request: Request<HolidayListRequest>,
    ) -> Result<Response<HolidayListResponse>, Status> {
        let rows = self
            .live_market
            .holiday_list_raw()
            .await
            .map_err(to_status)?;
        Ok(Response::new(HolidayListResponse {
            rows: rows.into_iter().map(holiday_row_to_proto).collect(),
        }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use jugaad_core::nse::{ChartDataPoint, IndexChartDataPoint};

    fn at(day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    fn stock_chart(days: &[u32]) -> CoreChartData {
        CoreChartData {
            identifier: "SBINEQN".into(),
            name: "SBIN".into(),
            close_price: 1.0,
            points: days
                .iter()
                .map(|&d| ChartDataPoint {
                    timestamp: at(d),
                    price: f64::from(d),
                    session: "NM".into(),
                    change: None,
                    percent_change: None,
                })
                .collect(),
        }
    }

    fn index_chart(days: &[u32]) -> CoreIndexChartData {
        CoreIndexChartData {
            identifier: "NIFTY 50".into(),
            name: "NIFTY 50".into(),
            close_price: 1.0,
            points: days
                .iter()
                .map(|&d| IndexChartDataPoint {
                    timestamp: at(d),
                    price: f64::from(d),
                    session: "NM".into(),
                    change: 0.0,
                    percent_change: 0.0,
                })
                .collect(),
        }
    }

    fn prices(points: impl Iterator<Item = f64>) -> Vec<f64> {
        points.collect()
    }

    #[test]
    fn stock_charts_are_newest_first_whichever_way_nse_sent_them() {
        for sent in [[1, 2, 3, 4], [4, 3, 2, 1]] {
            let chart = stock_chart_to_proto(stock_chart(&sent));
            assert_eq!(
                prices(chart.points.iter().map(|p| p.price)),
                [4.0, 3.0, 2.0, 1.0]
            );
            assert_eq!(chart.points[0].timestamp, "2026-10-04T00:00:00");
        }
    }

    #[test]
    fn index_charts_are_newest_first_whichever_way_nse_sent_them() {
        for sent in [[1, 2, 3, 4], [4, 3, 2, 1]] {
            let chart = index_chart_to_proto(index_chart(&sent));
            assert_eq!(
                prices(chart.points.iter().map(|p| p.price)),
                [4.0, 3.0, 2.0, 1.0]
            );
        }
    }
}
