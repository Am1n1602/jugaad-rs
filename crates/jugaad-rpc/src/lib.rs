use jugaad_core::nse::{NseQuote, OrderBook as CoreOrderBook, StockQuote as CoreStockQuote};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub mod jugaad {
    tonic::include_proto!("jugaad");
}

use jugaad::jugaad_server::Jugaad;
pub use jugaad::jugaad_server::JugaadServer;
use jugaad::{OrderBook, OrderBookLevel, StockQuote, StockQuoteRequest, WatchStockQuoteRequest};

const DEFAULT_WATCH_INTERVAL_SECS: u64 = 3;

#[derive(Debug)]
pub struct JugaadService {
    quote: NseQuote,
}

impl JugaadService {
    pub fn new() -> jugaad_core::Result<Self> {
        Ok(Self {
            quote: NseQuote::new()?,
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
}
