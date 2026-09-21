# jugaad-rs

[![CI](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml)

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
| Index category/name discovery | `NseIndexHistory::index_type_list`/`index_subtype_list`/`index_name_list` |
| F&O (futures/options) price + open-interest history | `NseHistory::derivatives_history_raw`/`derivatives_history_csv` |
| Generic daily reports (39+ types, by file key) | `NseDailyReports::list_available_reports`/`download_report_raw`/`download_report_save` |
| Live market status (open/closed per segment) | `NseLiveMarket::market_status_raw`/`market_status_csv` |
| Live index snapshot (every index, current price/change/ratios) | `NseLiveMarket::index_snapshot_raw`/`index_snapshot_csv` |
| Live market-wide turnover by segment | `NseLiveMarket::market_turnover_raw`/`market_turnover_csv` |
| Live NIFTY futures/options snapshot | `NseLiveMarket::live_fo_snapshot_raw`/`live_fo_snapshot_csv` |
| Today's block deals (pre-open and mid-day sessions) | `NseLiveMarket::block_deal_session_raw`/`block_deal_session_csv` |
| F&O turnover leaderboards (top-20 by value and by volume) | `NseLiveMarket::eq_derivative_turnover_raw`/`eq_derivative_turnover_csv` |
| Live stock quote (price, order book depth, volume) | `NseQuote::stock_quote_raw`/`stock_quote_csv` |
| Live F&O contracts for a symbol (all expiries/strikes) | `NseQuote::derivative_quote_raw`/`derivative_quote_csv` |
| Live single-index value, volume and turnover | `NseQuote::index_quote_raw`/`index_quote_csv` |
| Stock intraday/historical price chart | `NseQuote::stock_chart_data_raw` |
| Index/equity option chain | `NseQuote::option_chain_raw`/`option_chain_csv` |
| Currency pair option chain | `NseQuote::currency_option_chain_raw`/`currency_option_chain_csv` |
| Financial-results filings (equities/sme only) + XBRL/HTML download | `NseCorporateResults::financial_results_raw`/`financial_results_csv`/`download_xbrl_raw`/`download_xbrl_save`/`download_result_html_raw`/`download_result_html_save` |

Bhavcopy and F&O bhavcopy both automatically pick the right format for the date requested - NSE changed both formats on 2024-07-08, and callers don't need to know or care which side of that date they're asking about.

`NseCorporateResults` only supports the `equities` and `sme` segments - confirmed live, NSE's `insurance` and `reitsinvits` segments return a completely different, incompatible response shape through the same endpoint (not a documentation gap, a real schema difference), and `debt` returned no data in testing. See [`docs/nse-findings.md`](docs/nse-findings.md#corporates-financial-results-returns-a-genuinely-different-schema-per-segment) for the full breakdown.

The live/quote endpoints above were built while NSE's market was closed, then spot-checked again live with the market genuinely open (2026-09-21) - order book depth and index values do populate/move as expected; `market-turnover`'s same-day figures and the Currency/Commodity/Debt segments' snapshot values turned out to just never populate through these endpoints, open market or not, which is now confirmed rather than assumed. See [`docs/nse-findings.md`](docs/nse-findings.md#the-market-hours-retest-done-live-on-2026-09-21-nse-genuinely-open) for the full rundown.

Per-symbol live quotes and option chains are **not** behind Akamai bot detection, contrary to what an earlier version of this README claimed - NSE had just moved those endpoints to different URLs than the ones first tried here. See [`docs/nse-findings.md`](docs/nse-findings.md) for the full story.

## What this adds beyond jugaad-data

**Data jugaad-data doesn't have at all:**
- **Financial-results filings + XBRL/HTML download** (`NseCorporateResults`) - jugaad-data's `NSELive` wraps the newer SEBI Integrated Filing framework (`corporate_integrated_filing`), but not the older Regulation 33 filing type this crate covers, which is the only source for machine-readable financials before that framework existed (roughly FY2024-25). Company financials going back to FY2012-13 (for TCS; likely similar for other long-listed companies) aren't reachable through jugaad-data at all.

**Real correctness issues avoided or caught**
- **A live date bug jugaad-data actually has.** NSE's stock-history API returns two date fields for the same row - `mTIMESTAMP` (correct) and `CH_TIMESTAMP` (a UTC-shifted timestamp that lands on the wrong calendar day if read directly). jugaad-data uses `CH_TIMESTAMP`; this crate uses `mTIMESTAMP` instead, confirmed live.
- **The same UTC-shift bug, found again in a second endpoint.** The stock chart-data endpoint's per-point epoch timestamps have the identical problem as `CH_TIMESTAMP` above - built from IST wall-clock digits but labeled as UTC. Caught by comparing a live point's timestamp against a same-moment live quote's `last_update_time` (genuine IST) and finding a ~5:30 gap; corrected before being exposed publicly, so this crate never emits the wrong instant in the first place.
- **Type-safety that makes a documented real-world mistake impossible to write.** Financial filings mark whether figures are consolidated via a string field - `"Consolidated"` or `"Non-Consolidated"`. A plain substring check for `"consolidated"` matches inside `"Non-Consolidated"` too. jugaad-data hands this back as an untyped string either way; this crate models it (and the equivalent audited/not-audited field) as enums, so that specific mistake can't be written at all.
- **NSE's own data is inconsistently typed, and this crate surfaces that immediately instead of silently absorbing it.** Confirmed live: the same numeric field (open interest, on F&O contracts) comes back as a JSON integer for some contracts and a JSON float for others in the identical response. A dynamically-typed client just accepts either silently; this crate's strict deserialization caught the inconsistency immediately during testing.
- **A silent multi-month ordering bug, caught by testing against real exported output.** Every history endpoint that splits a long date range into per-month chunks was concatenating those chunks assuming NSE returns each one oldest-first - confirmed live that NSE actually returns each chunk newest-first, which without a fix scrambles a multi-month CSV into a "sawtooth" (descending within each month, ascending month to month) rather than one consistent order.

**Structural**
- `unsafe_code = "forbid"` at the workspace level - a checkable guarantee, enforced by the compiler on every build.
- A running, dated log of every undocumented NSE quirk found ([`docs/nse-findings.md`](docs/nse-findings.md)) - the endpoint migrations, date-format traps, and schema-per-segment differences above are all recorded there with how they were confirmed.
- A single static binary (`cargo build --release`) as an alternative to the library - no interpreter or `pip install` needed to just download data.

None of this makes jugaad-rs a strict superset yet - see Pending below for what jugaad-data still covers that this doesn't.

## Pending

- **A `dataframe`/`polars` Cargo feature** - not started; would add optional `Vec<Row>` → `polars::DataFrame` conversions on top of the fetchers that already exist, not a new data source. An earlier, empty placeholder for this feature flag was removed as dead config - it'll be added back in the same change that actually implements the conversions
- **`top_stocks`** (top gainers/losers/most-active) - confirmed reachable live via the same NextApi pattern that fixed `stock_chart_data_raw`, not yet designed/built; only one of its 8 sub-lists has been shape-checked so far.

## Requirements

- Rust 1.88 or newer (see `rust-toolchain.toml` - `rustup` will pick this up automatically)

## Building

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

Run `jugaad --help` for the full, always-current command list (29 commands
as of this writing - bhavcopy in three variants, stock/index/derivatives
history, bulk deals, generic daily reports, index P/E/TRI/discovery, live
market status/index snapshot/turnover/F&O/block deals, live per-symbol
quotes/option chains, and financial-results filings). A few representative
examples:

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

Every command writes a CSV to a `data/nse/...` directory by default (configurable with `--output`). See [`docs/cli.md`](docs/cli.md) for the full reference: every command, flag, default, and behavior (like what happens on a weekend or an unknown symbol).

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

## Documentation

- [`docs/cli.md`](docs/cli.md) - full CLI reference, kept in sync with `--help` output
- [`docs/nse-findings.md`](docs/nse-findings.md) - a running log of undocumented NSE API behavior discovered while building this

## License

MIT - see [LICENSE](LICENSE) for the full text.
