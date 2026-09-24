# jugaad-rs

[![CI](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Am1n1602/jugaad-rs)](https://github.com/Am1n1602/jugaad-rs/releases/latest)

A Rust rewrite of [`jugaad-data`](https://github.com/jugaad-py/jugaad-data), a Python library for downloading historical and live market data from NSE (National Stock Exchange of India). This project follows the same behavior where it makes sense, but is written idiomatically in Rust rather than as a line-by-line port - see [`docs/nse-findings.md`](docs/nse-findings.md) for the API details this rewrite has had to work out from scratch, since NSE's endpoints have no official documentation.

## What's implemented

| Data | Function |
|---|---|
| Daily bhavcopy (whole-market OHLC), old and new format | `NseArchives::bhavcopy_raw`/`bhavcopy_save` |
| F&O daily bhavcopy, old and new format | `NseArchives::bhavcopy_fo_raw`/`bhavcopy_fo_save` |
| "Full" bhavcopy (every series + delivery data) | `NseArchives::full_bhavcopy_raw`/`full_bhavcopy_save` |
| Bulk deals (current snapshot, no date) | `NseArchives::bulk_deals_raw`/`bulk_deals_save` |
| Per-stock daily OHLCV history | `NseHistory::stock_history_raw`/`stock_history_csv` |
| Per-index daily OHLC history | `NseIndexHistory::index_history_raw`/`index_history_csv` |
| Per-index P/E, P/B, dividend yield | `NseIndexHistory::index_pe_history_raw`/`index_pe_history_csv` |
| Per-index Total Return Index | `NseIndexHistory::index_tri_history_raw`/`index_tri_history_csv` |
| Whole-market index bhavcopy (every index's OHLC/turnover/P-E-P-B-yield, one day) | `NseIndexHistory::index_bhavcopy_raw`/`index_bhavcopy_save` |
| Index category/name discovery | `NseIndexHistory::index_type_list`/`index_subtype_list`/`index_name_list` |
| F&O (futures/options) price + open-interest history | `NseHistory::derivatives_history_raw`/`derivatives_history_csv` |
| Generic daily reports (39+ types, by file key) | `NseDailyReports::list_available_reports`/`download_report_raw`/`download_report_save` |
| Live market status (open/closed per segment) | `NseLiveMarket::market_status_raw`/`market_status_csv` |
| Live index snapshot (every index, current price/change/ratios) | `NseLiveMarket::index_snapshot_raw`/`index_snapshot_csv` |
| Live market-wide turnover by segment | `NseLiveMarket::market_turnover_raw`/`market_turnover_csv` |
| Live NIFTY futures/options snapshot | `NseLiveMarket::live_fo_snapshot_raw`/`live_fo_snapshot_csv` |
| Today's block deals (pre-open and mid-day sessions) | `NseLiveMarket::block_deal_session_raw`/`block_deal_session_csv` |
| F&O turnover leaderboards (top-20 by value and by volume) | `NseLiveMarket::eq_derivative_turnover_raw`/`eq_derivative_turnover_csv` |
| Top gainers/losers across 7 index/security scopes | `NseLiveMarket::market_movers_raw`/`market_movers_csv` |
| Most active equities (by value and by volume) | `NseLiveMarket::most_active_equities_raw` |
| Volume gainers | `NseLiveMarket::volume_gainers_raw` |
| 52-week highs/lows | `NseLiveMarket::fifty_two_week_raw` |
| Large deals (bulk, short, block) | `NseLiveMarket::large_deals_raw` |
| Trading holiday calendar (every segment) | `NseLiveMarket::holiday_list_raw` |
| Symbol regulatory/compliance status | `NseQuote::reg_details_raw` |
| Indices a symbol belongs to | `NseQuote::index_list_raw` |
| Symbol static metadata (eligibility flags, ISIN) | `NseQuote::symbol_meta_raw` |
| Symbol name lookup | `NseQuote::symbol_name_raw` |
| Symbol change vs. benchmark across trailing windows | `NseQuote::yearwise_data_raw` |
| Live stock quote (price, order book depth, volume) | `NseQuote::stock_quote_raw`/`stock_quote_csv` |
| Live F&O contracts for a symbol (all expiries/strikes) | `NseQuote::derivative_quote_raw`/`derivative_quote_csv` |
| Live single-index value, volume and turnover | `NseQuote::index_quote_raw`/`index_quote_csv` |
| Stock intraday/historical price chart | `NseQuote::stock_chart_data_raw`/`stock_chart_data_csv` |
| Index intraday/historical price chart | `NseQuote::index_chart_data_raw`/`index_chart_data_csv` |
| Index/equity option chain | `NseQuote::option_chain_raw`/`option_chain_csv` |
| Currency pair option chain | `NseQuote::currency_option_chain_raw`/`currency_option_chain_csv` |
| Financial-results filings (equities/sme only) + XBRL/HTML download | `NseCorporateResults::financial_results_raw`/`financial_results_csv`/`download_xbrl_raw`/`download_xbrl_save`/`download_result_html_raw`/`download_result_html_save` |
| Corporate announcements (equities/sme/debt/mf/invitsreits/municipalBond) | `NseCorporateAnnouncements::corporate_announcements_raw`/`corporate_announcements_csv` |
| Social Stock Exchange announcements | `NseCorporateAnnouncements::sse_announcements_raw`/`sse_announcements_csv` |
| SEBI Integrated Filing entries (Financials/Governance) | `NseCorporateAnnouncements::corporate_integrated_filing_raw`/`corporate_integrated_filing_csv` |
| Announcement/filing attachments (PDF/XBRL/iXBRL) | `NseCorporateAnnouncements::download_attachment_raw`/`download_attachment_save` |

Bhavcopy and F&O bhavcopy both automatically pick the right format for the date requested - NSE changed both formats on 2024-07-08, and callers don't need to know or care which side of that date they're asking about.

An optional `dataframe` Cargo feature adds `jugaad_core::dataframe::to_dataframe`, converting any `Vec<Row>` from the table above into a `polars::DataFrame`. It's one generic function, not a conversion method per type - every row type already implements `Serialize` (for CSV export), so rows are serialized to newline-delimited JSON in memory and handed to polars' own JSON reader, rather than hand-writing a column builder for each of the ~20 row types in this crate. Enable it with `jugaad-core = { path = "...", features = ["dataframe"] }`.

`NseCorporateResults` only supports the `equities` and `sme` segments - confirmed live, NSE's `insurance` and `reitsinvits` segments return a completely different, incompatible response shape through the same endpoint (not a documentation gap, a real schema difference), and `debt` returned no data in testing. See [`docs/nse-findings.md`](docs/nse-findings.md#corporates-financial-results) for the full breakdown.

The live/quote endpoints above were built while NSE's market was closed, then spot-checked again live with the market genuinely open (2026-09-21) - order book depth and index values do populate/move as expected; `market-turnover`'s same-day figures and the Currency/Commodity/Debt segments' snapshot values turned out to just never populate through these endpoints, open market or not, which is now confirmed rather than assumed. See [`docs/nse-findings.md`](docs/nse-findings.md#market-hours-retest-2026-09-21-market-genuinely-open) for the full rundown.

Per-symbol live quotes and option chains are **not** behind Akamai bot detection, contrary to what an earlier version of this README claimed - NSE had just moved those endpoints to different URLs than the ones first tried here. See [`docs/nse-findings.md`](docs/nse-findings.md) for the full story.


## Not implemented

- **`pre_open_market`** - dropped from scope for now. NSE's pre-open session data only exists during the actual pre-open window (roughly 9:00-9:15 IST each trading day), which makes it impractical to verify and maintain the same way as everything else here; confirmed live outside that window that the endpoint works but has nothing to show (`{"data":[],"msg":"No Data Found"}`), so the real row shape was never confirmed. See [`docs/nse-findings.md`](docs/nse-findings.md#pre_open_market) for what was found before this was scoped out.

## Requirements

- Rust 1.88 or newer (see `rust-toolchain.toml` - `rustup` will pick this up automatically)

## Building

Prebuilt binaries for Linux, macOS (Intel and Apple Silicon), and Windows
are attached to every [release](https://github.com/Am1n1602/jugaad-rs/releases/latest) -
download the one for your platform and skip building from source entirely.

To build from source instead:

```bash
git clone https://github.com/Am1n1602/jugaad-rs.git
cd jugaad-rs
cargo build --release
```

The CLI binary is `jugaad`, built at `target/release/jugaad`.

## CLI usage

```bash
cargo run -p jugaad-cli -- <COMMAND> [OPTIONS]
```

Run `jugaad --help` for the full, always-current command list (47 commands
as of this writing - bhavcopy in three variants (including the
whole-market index bhavcopy), stock/index/derivatives history, bulk
deals, generic daily reports, index P/E/TRI/discovery, live market
status/index snapshot/turnover/F&O/block deals/F&O turnover
leaderboards/market movers/most-active/volume-gainers/52-week-high-low/
large-deals/holidays, live per-symbol quotes/charts/reg-details/meta/
name/yearwise-data, index price charts, option chains, financial-results
filings, corporate announcements, and SEBI Integrated Filing entries). A
few representative examples:

```bash
# Whole-market bhavcopy for one day
jugaad bhavcopy 2024-08-01

# A stock's price history over a range
jugaad stock SBIN --from 2024-08-01 --to 2024-08-31

# An index's price history
jugaad index "NIFTY 50" --from 2024-08-01 --to 2024-08-31

# A futures contract's history
jugaad derivatives NIFTY --from 2024-12-01 --to 2024-12-05 --expiry 2024-12-26 --instrument fut-idx

# A stock's live quote (price, order book, volume)
jugaad stock-quote SBIN

# An index or equity's option chain
jugaad option-chain NIFTY --kind index
```

Every command writes a CSV under `data/nse/...` by default (configurable with `--output`) - most take a directory and derive the filename from the arguments given (e.g. the symbol), while whole-market live snapshots (`market-status`, `eq-derivative-turnover`, `market-movers`, etc.) take a full file path instead, since there's no per-call argument to name the file from. See [`docs/cli.md`](docs/cli.md) for the full reference: every command, flag, default, and behavior (like what happens on a weekend or an unknown symbol).

## Using it as a library

Add `jugaad-core` as a path or git dependency and call the client types directly - no CLI required:

```rust
use chrono::NaiveDate;
use jugaad_core::nse::NseHistory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = NseHistory::new()?;
    let from = NaiveDate::from_ymd_opt(2024, 8, 1).ok_or("invalid date")?;
    let to = NaiveDate::from_ymd_opt(2024, 8, 31).ok_or("invalid date")?;

    let rows = history.stock_history_raw("SBIN", from, to, "EQ").await?;
    for row in &rows {
        println!("{}: close={}", row.date, row.close);
    }

    Ok(())
}
```

More usage examples live in [`crates/jugaad-core/examples/`](crates/jugaad-core/examples/) - runnable with `cargo run -p jugaad-core --example <name>`.

## Project structure

- [`crates/jugaad-core`](crates/jugaad-core/) - the library: NSE client types, request/response handling, error types
- [`crates/jugaad-cli`](crates/jugaad-cli/) - the `jugaad` command-line binary, built on top of `jugaad-core`

## Development

```bash
cargo build --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
```

`cargo test` only runs network-free unit tests by default. There's also a suite of integration tests that hit live NSE endpoints, marked `#[ignore]` so they never run in CI or block a normal `cargo test`:

```bash
cargo test -p jugaad-core -- --ignored
```

To test the optional `dataframe` feature specifically:

```bash
cargo test -p jugaad-core --features dataframe
```

If that (or `cargo test --workspace --all-features`) intermittently fails with errors like `crate ... required to be available in rlib format, but was not found in this form` for a different, seemingly unrelated crate each run, it's very likely cargo's default job count (one per logical CPU) exceeding available RAM once `polars`'s large dependency tree is in the build - not a real compile error. Cap parallelism in `.cargo/config.toml`: `[build]` / `jobs = 4` (or pass `--jobs 4` on the command line) and retry.


## Documentation

- [`docs/cli.md`](docs/cli.md) - full CLI reference, kept in sync with `--help` output
- [`docs/nse-findings.md`](docs/nse-findings.md) - a running log of undocumented NSE API behavior discovered while building this

## License

MIT - see [LICENSE](LICENSE) for the full text.
