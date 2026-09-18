//! Integration tests that hit real NSE endpoints. These are marked
//! `#[ignore]` so `cargo test` (what CI runs) skips them by default and
//! doesn't flake on network issues or NSE rate limits. Run them manually
//! with:
//!
//!     cargo test -p jugaad-core -- --ignored
#![allow(clippy::unwrap_used)]

use chrono::NaiveDate;
use jugaad_core::nse::{Instrument, NseArchives, NseHistory, NseIndexHistory, OptionType};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_raw_fetches_a_known_trading_day() {
    let archives = NseArchives::new().unwrap();
    let text = archives.bhavcopy_raw(date(2024, 8, 1)).await.unwrap();

    assert!(text.starts_with("TradDt"));
    assert!(text.lines().count() > 1000);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_raw_reports_no_data_on_a_weekend() {
    // 2024-08-03 was a Saturday.
    let archives = NseArchives::new().unwrap();
    let err = archives.bhavcopy_raw(date(2024, 8, 3)).await.unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NoData));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_raw_fetches_a_pre_udiff_trading_day() {
    let archives = NseArchives::new().unwrap();
    let text = archives.bhavcopy_raw(date(2020, 1, 1)).await.unwrap();

    // Old format's header includes ISIN; new (UDiff) format starts "TradDt".
    assert!(text.starts_with("SYMBOL,SERIES"));
    assert!(text.contains("ISIN"));
    assert!(text.lines().count() > 1000);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_raw_reports_no_data_for_a_pre_udiff_weekend() {
    // 2020-01-04 was a Saturday.
    let archives = NseArchives::new().unwrap();
    let err = archives.bhavcopy_raw(date(2020, 1, 4)).await.unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NoData));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_fo_raw_fetches_a_udiff_era_trading_day() {
    let archives = NseArchives::new().unwrap();
    let text = archives.bhavcopy_fo_raw(date(2024, 8, 1)).await.unwrap();

    assert!(text.starts_with("TradDt"));
    assert!(text.lines().count() > 1000);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bhavcopy_fo_raw_fetches_a_pre_udiff_trading_day() {
    let archives = NseArchives::new().unwrap();
    let text = archives.bhavcopy_fo_raw(date(2020, 1, 1)).await.unwrap();

    assert!(text.starts_with("INSTRUMENT,SYMBOL"));
    assert!(text.lines().count() > 1000);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn full_bhavcopy_raw_fetches_a_known_trading_day() {
    let archives = NseArchives::new().unwrap();
    let text = archives.full_bhavcopy_raw(date(2024, 8, 1)).await.unwrap();

    assert!(text.starts_with("SYMBOL"));
    assert!(text.contains("DELIV_QTY"));
    assert!(text.lines().count() > 1000);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn full_bhavcopy_raw_reports_no_data_on_a_weekend() {
    let archives = NseArchives::new().unwrap();
    let err = archives
        .full_bhavcopy_raw(date(2024, 8, 3))
        .await
        .unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NoData));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn bulk_deals_raw_fetches_the_current_report() {
    let archives = NseArchives::new().unwrap();
    let text = archives.bulk_deals_raw().await.unwrap();

    assert!(text.starts_with("Date,Symbol"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_history_raw_fetches_a_known_range() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .stock_history_raw("SBIN", date(2024, 8, 1), date(2024, 8, 5), "EQ")
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.symbol == "SBIN"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_history_raw_splits_a_multi_month_range() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .stock_history_raw("SBIN", date(2024, 6, 15), date(2024, 8, 15), "EQ")
        .await
        .unwrap();

    // Roughly 20 trading days/month; just check chunking actually stitched
    // together more than a single month's worth of rows.
    assert!(rows.len() > 30);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_history_raw_returns_empty_for_weekend_only_range() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .stock_history_raw("SBIN", date(2024, 8, 3), date(2024, 8, 4), "EQ")
        .await
        .unwrap();

    assert!(rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_history_raw_fetches_a_known_range() {
    let history = NseIndexHistory::new().unwrap();
    let rows = history
        .index_history_raw("NIFTY 50", date(2024, 8, 1), date(2024, 8, 5))
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.index_name == "Nifty 50"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_history_raw_splits_a_multi_month_range() {
    let history = NseIndexHistory::new().unwrap();
    let rows = history
        .index_history_raw("NIFTY 50", date(2024, 6, 15), date(2024, 8, 15))
        .await
        .unwrap();

    assert!(rows.len() > 30);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_history_raw_returns_empty_for_an_unknown_index_name() {
    let history = NseIndexHistory::new().unwrap();
    let rows = history
        .index_history_raw("NOT A REAL INDEX", date(2024, 8, 1), date(2024, 8, 5))
        .await
        .unwrap();

    assert!(rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn derivatives_history_raw_fetches_index_futures() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .derivatives_history_raw(
            "NIFTY",
            date(2024, 12, 1),
            date(2024, 12, 5),
            date(2024, 12, 26),
            Instrument::FutIdx,
        )
        .await
        .unwrap();

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.instrument == "FUTIDX"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn derivatives_history_raw_fetches_index_options() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .derivatives_history_raw(
            "NIFTY",
            date(2024, 12, 1),
            date(2024, 12, 5),
            date(2024, 12, 26),
            Instrument::OptIdx {
                strike_price: 24000.0,
                option_type: OptionType::Call,
            },
        )
        .await
        .unwrap();

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.option_type == "CE"));
    assert!(rows.iter().all(|row| row.strike_price == 24000.0));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn derivatives_history_raw_returns_empty_for_an_unknown_expiry() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .derivatives_history_raw(
            "NIFTY",
            date(2024, 12, 1),
            date(2024, 12, 5),
            date(2099, 1, 1),
            Instrument::FutIdx,
        )
        .await
        .unwrap();

    assert!(rows.is_empty());
}
