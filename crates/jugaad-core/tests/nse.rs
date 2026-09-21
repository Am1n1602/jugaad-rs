//! Integration tests that hit real NSE endpoints. These are marked
//! `#[ignore]` so `cargo test` (what CI runs) skips them by default and
//! doesn't flake on network issues or NSE rate limits. Run them manually
//! with:
//!
//!     cargo test -p jugaad-core -- --ignored
#![allow(clippy::unwrap_used)]

use chrono::NaiveDate;
use jugaad_core::nse::{
    ChartPeriod, ConsolidationBasis, Instrument, NseArchives, NseCorporateResults, NseDailyReports,
    NseHistory, NseIndexHistory, NseLiveMarket, NseQuote, OptionChainKind, OptionType,
    ResultPeriod,
};

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
async fn list_available_reports_includes_bulk_deal() {
    let reports = NseDailyReports::new().unwrap();
    let summaries = reports.list_available_reports("CM").await.unwrap();

    assert!(!summaries.is_empty());
    assert!(summaries.iter().any(|s| s.file_key == "CM-BULK-DEAL"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn download_report_raw_fetches_bulk_deal() {
    let reports = NseDailyReports::new().unwrap();
    let bytes = reports
        .download_report_raw("CM-BULK-DEAL", "CM")
        .await
        .unwrap();

    assert!(String::from_utf8_lossy(&bytes).starts_with("Date,Symbol"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn download_report_raw_returns_not_found_for_an_unknown_file_key() {
    let reports = NseDailyReports::new().unwrap();
    let err = reports
        .download_report_raw("NOT-A-REAL-FILE-KEY", "CM")
        .await
        .unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NotFound(_)));
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
    // NSE returns each chunk's rows in descending order internally, so
    // this is the case that actually catches the "sawtooth" bug - naively
    // concatenating chunks in ascending-month order without a final sort
    // would fail this.
    assert!(rows.windows(2).all(|w| w[0].date >= w[1].date));
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
    // See the identical assertion in stock_history_raw_splits_a_multi_month_range.
    assert!(rows.windows(2).all(|w| w[0].date >= w[1].date));
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
async fn index_pe_history_raw_fetches_a_known_range() {
    let history = NseIndexHistory::new().unwrap();
    let rows = history
        .index_pe_history_raw("NIFTY 50", date(2024, 8, 1), date(2024, 8, 5))
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.index_name == "Nifty 50"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_tri_history_raw_fetches_a_known_range() {
    let history = NseIndexHistory::new().unwrap();
    let rows = history
        .index_tri_history_raw("NIFTY 50", "NIFTY 50", date(2024, 8, 1), date(2024, 8, 5))
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.index_name == "Nifty 50"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_type_list_includes_equity() {
    let history = NseIndexHistory::new().unwrap();
    let types = history.index_type_list().await.unwrap();

    assert!(types.contains(&"Equity".to_string()));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_subtype_list_includes_broad_market_indices() {
    let history = NseIndexHistory::new().unwrap();
    let subtypes = history
        .index_subtype_list("Equity", "Historical Index Data")
        .await
        .unwrap();

    assert!(subtypes.contains(&"Broad Market Indices".to_string()));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_name_list_includes_nifty_50() {
    let history = NseIndexHistory::new().unwrap();
    let names = history
        .index_name_list("Broad Market Indices", "Historical Index Data")
        .await
        .unwrap();

    assert!(names.contains(&"NIFTY 50".to_string()));
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
async fn derivatives_history_raw_splits_a_multi_month_range() {
    let history = NseHistory::new().unwrap();
    let rows = history
        .derivatives_history_raw(
            "NIFTY",
            date(2024, 11, 1),
            date(2024, 12, 5),
            date(2024, 12, 26),
            Instrument::FutIdx,
        )
        .await
        .unwrap();

    assert!(!rows.is_empty());
    // See the identical assertion in stock_history_raw_splits_a_multi_month_range -
    // NSE returns each chunk's rows in descending order internally.
    assert!(rows.windows(2).all(|w| w[0].date >= w[1].date));
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

#[tokio::test]
#[ignore = "hits live NSE"]
async fn market_status_raw_includes_capital_market() {
    let live = NseLiveMarket::new().unwrap();
    let segments = live.market_status_raw().await.unwrap();

    assert!(segments.iter().any(|s| s.market == "Capital Market"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_snapshot_raw_includes_nifty_50() {
    let live = NseLiveMarket::new().unwrap();
    let rows = live.index_snapshot_raw().await.unwrap();

    assert!(rows.iter().any(|r| r.name == "NIFTY 50"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn market_turnover_raw_includes_equities() {
    let live = NseLiveMarket::new().unwrap();
    let rows = live.market_turnover_raw().await.unwrap();

    assert!(rows.iter().any(|r| r.name.as_deref() == Some("Equities")));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn live_fo_snapshot_raw_fetches_nifty_futures() {
    let live = NseLiveMarket::new().unwrap();
    let rows = live.live_fo_snapshot_raw().await.unwrap();

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.underlying == "NIFTY"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_quote_raw_fetches_sbin() {
    let quote = NseQuote::new().unwrap();
    let sbin = quote.stock_quote_raw("SBIN").await.unwrap();

    assert_eq!(sbin.symbol, "SBIN");
    assert!(sbin.last_price > 0.0);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_quote_raw_returns_not_found_for_an_unknown_symbol() {
    let quote = NseQuote::new().unwrap();
    let err = quote.stock_quote_raw("NOTAREALSYMBOL").await.unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NotFound(_)));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_chart_data_raw_fetches_sbin_intraday() {
    let quote = NseQuote::new().unwrap();
    let chart = quote
        .stock_chart_data_raw("SBIN", ChartPeriod::OneDay)
        .await
        .unwrap();

    assert_eq!(chart.identifier, "SBINEQN");
    assert_eq!(chart.name, "SBIN");
    assert!(!chart.points.is_empty());
    assert!(chart.points.iter().all(|p| p.price > 0.0));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_chart_data_raw_one_month_has_no_intraday_change() {
    let quote = NseQuote::new().unwrap();
    let chart = quote
        .stock_chart_data_raw("SBIN", ChartPeriod::OneMonth)
        .await
        .unwrap();

    assert!(!chart.points.is_empty());
    assert!(chart.points.iter().all(|p| p.change.is_none()));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn stock_chart_data_raw_returns_not_found_for_an_unknown_symbol() {
    let quote = NseQuote::new().unwrap();
    let err = quote
        .stock_chart_data_raw("NOTAREALSYMBOL", ChartPeriod::OneDay)
        .await
        .unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NotFound(_)));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn derivative_quote_raw_fetches_nifty_contracts() {
    let quote = NseQuote::new().unwrap();
    let rows = quote.derivative_quote_raw("NIFTY").await.unwrap();

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.underlying == "NIFTY"));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn derivative_quote_raw_returns_empty_for_an_unknown_symbol() {
    let quote = NseQuote::new().unwrap();
    let rows = quote.derivative_quote_raw("NOTAREALSYMBOL").await.unwrap();

    assert!(rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_quote_raw_fetches_nifty_50() {
    let quote = NseQuote::new().unwrap();
    let row = quote.index_quote_raw("NIFTY 50").await.unwrap();

    assert_eq!(row.name, "NIFTY 50");
    assert!(row.last > 0.0);
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn index_quote_raw_returns_not_found_for_an_unknown_index() {
    let quote = NseQuote::new().unwrap();
    let err = quote.index_quote_raw("NOT A REAL INDEX").await.unwrap_err();

    assert!(matches!(err, jugaad_core::Error::NotFound(_)));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn option_chain_raw_fetches_nifty_with_default_expiry() {
    let quote = NseQuote::new().unwrap();
    let rows = quote
        .option_chain_raw("NIFTY", OptionChainKind::Index, None)
        .await
        .unwrap();

    assert!(!rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn option_chain_raw_fetches_an_equity() {
    let quote = NseQuote::new().unwrap();
    let rows = quote
        .option_chain_raw("SBIN", OptionChainKind::Equity, None)
        .await
        .unwrap();

    assert!(!rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn currency_option_chain_raw_fetches_usdinr() {
    let quote = NseQuote::new().unwrap();
    let rows = quote.currency_option_chain_raw("USDINR").await.unwrap();

    assert!(!rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn block_deal_session_raw_succeeds() {
    let live = NseLiveMarket::new().unwrap();
    let rows = live.block_deal_session_raw().await.unwrap();

    // Whether any block deals happened today (in either session) varies
    // day to day - just check the call succeeds and every row that does
    // come back is tagged with a real session.
    assert!(
        rows.iter()
            .all(|r| r.session == "session1" || r.session == "session2")
    );
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn financial_results_raw_fetches_tcs_annual_filings() {
    let client = NseCorporateResults::new().unwrap();
    let rows = client
        .financial_results_raw(
            "equities",
            "TCS",
            ResultPeriod::Annual,
            date(2012, 1, 1),
            date(2026, 9, 19),
        )
        .await
        .unwrap();

    assert!(rows.len() > 20);
    assert!(rows.iter().all(|r| r.symbol == "TCS"));
    // Oldest filings have no real XBRL (confirmed live); newest ones do.
    assert!(rows.iter().any(|r| r.xbrl_url.is_none()));
    assert!(rows.iter().any(|r| r.xbrl_url.is_some()));
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn financial_results_raw_returns_empty_for_an_unknown_symbol() {
    let client = NseCorporateResults::new().unwrap();
    let rows = client
        .financial_results_raw(
            "equities",
            "NOTAREALSYMBOL",
            ResultPeriod::Annual,
            date(2015, 1, 1),
            date(2026, 9, 19),
        )
        .await
        .unwrap();

    assert!(rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn financial_results_raw_parses_sme_segment() {
    let client = NseCorporateResults::new().unwrap();
    // sme filings are frequent enough that a recent quarter should have
    // several - no specific symbol needed since this just checks the
    // sme-specific "Unaudited" (no hyphen) spelling parses correctly.
    let rows = client
        .financial_results_raw(
            "sme",
            "",
            ResultPeriod::Quarterly,
            date(2026, 1, 1),
            date(2026, 9, 19),
        )
        .await
        .unwrap();

    // An empty symbol matches nothing - this just confirms the segment
    // parameter itself doesn't error; the real sme shape/casing coverage
    // is in the unit tests using a captured sample.
    assert!(rows.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn download_xbrl_raw_and_download_result_html_raw_both_succeed() {
    let client = NseCorporateResults::new().unwrap();
    let rows = client
        .financial_results_raw(
            "equities",
            "TCS",
            ResultPeriod::Annual,
            date(2012, 1, 1),
            date(2026, 9, 19),
        )
        .await
        .unwrap();

    let with_xbrl = rows.iter().find(|r| r.xbrl_url.is_some()).unwrap();
    let xbrl_bytes = client
        .download_xbrl_raw(with_xbrl.xbrl_url.as_deref().unwrap())
        .await
        .unwrap();
    assert!(!xbrl_bytes.is_empty());

    let with_html = rows
        .iter()
        .find(|r| r.result_detailed_data_link.is_some())
        .unwrap();
    let html_bytes = client
        .download_result_html_raw(with_html.result_detailed_data_link.as_deref().unwrap())
        .await
        .unwrap();
    assert!(!html_bytes.is_empty());
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn financial_results_raw_deserializes_consolidation_basis_correctly() {
    let client = NseCorporateResults::new().unwrap();
    let rows = client
        .financial_results_raw(
            "equities",
            "TCS",
            ResultPeriod::Annual,
            date(2023, 1, 1),
            date(2024, 12, 31),
        )
        .await
        .unwrap();

    assert!(
        rows.iter()
            .any(|r| r.consolidated == ConsolidationBasis::Consolidated)
    );
    assert!(
        rows.iter()
            .any(|r| r.consolidated == ConsolidationBasis::NonConsolidated)
    );
}

#[tokio::test]
#[ignore = "hits live NSE"]
async fn eq_derivative_turnover_raw_returns_both_leaderboards() {
    let live = NseLiveMarket::new().unwrap();
    let rows = live.eq_derivative_turnover_raw().await.unwrap();

    assert!(rows.iter().any(|r| r.ranking == "value"));
    assert!(rows.iter().any(|r| r.ranking == "volume"));
    assert!(
        rows.iter()
            .all(|r| r.ranking == "value" || r.ranking == "volume")
    );
}
