use chrono::NaiveDate;
use jugaad_core::nse::{
    CorporateAnnouncementRow as CoreCorporateAnnouncementRow,
    IndexHistoryRow as CoreIndexHistoryRow, IndexSnapshotRow as CoreIndexSnapshotRow,
    LargeDealRow as CoreLargeDealRow, MarketSegmentStatus as CoreMarketSegmentStatus,
    NseCorporateAnnouncements, NseHistory, NseIndexHistory, NseLiveMarket, NseQuote,
    OptionChainKind as CoreOptionChainKind, OptionChainRow as CoreOptionChainRow,
    OptionLeg as CoreOptionLeg, OrderBook as CoreOrderBook, StockHistoryRow as CoreStockHistoryRow,
    StockQuote as CoreStockQuote,
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
    CorporateAnnouncementRow, CorporateAnnouncementsRequest, CorporateAnnouncementsResponse,
    IndexHistoryRequest, IndexHistoryResponse, IndexHistoryRow, IndexSnapshotRequest,
    IndexSnapshotResponse, IndexSnapshotRow, LargeDealRow, LargeDealsRequest, LargeDealsResponse,
    MarketSegmentStatus, MarketStatusRequest, MarketStatusResponse, OptionChainKind,
    OptionChainRequest, OptionChainResponse, OptionChainRow, OptionExpiriesRequest,
    OptionExpiriesResponse, OptionLeg, OrderBook, OrderBookLevel, StockHistoryRequest,
    StockHistoryResponse, StockHistoryRow, StockQuote, StockQuoteRequest, WatchStockQuoteRequest,
};

const DEFAULT_WATCH_INTERVAL_SECS: u64 = 3;

#[derive(Debug)]
pub struct JugaadService {
    quote: NseQuote,
    history: NseHistory,
    index_history: NseIndexHistory,
    live_market: NseLiveMarket,
    corporate_announcements: NseCorporateAnnouncements,
}

impl JugaadService {
    pub fn new() -> jugaad_core::Result<Self> {
        Ok(Self {
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
}
