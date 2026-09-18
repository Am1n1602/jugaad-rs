//! Integration tests that hit real NSE endpoints. These are marked
//! `#[ignore]` so `cargo test` (what CI runs) skips them by default and
//! doesn't flake on network issues or NSE rate limits. Run them manually
//! with:
//!
//!     cargo test -p jugaad-core -- --ignored
#![allow(clippy::unwrap_used)]

use chrono::NaiveDate;
use jugaad_core::nse::{NseArchives, NseHistory};

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
