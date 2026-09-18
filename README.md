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
| Live stock quote (price, order book depth, volume) | `NseQuote::stock_quote_raw`/`stock_quote_csv` |
| Live F&O contracts for a symbol (all expiries/strikes) | `NseQuote::derivative_quote_raw`/`derivative_quote_csv` |
| Live single-index value, volume and turnover | `NseQuote::index_quote_raw`/`index_quote_csv` |
| Index/equity option chain | `NseQuote::option_chain_raw`/`option_chain_csv` |
| Currency pair option chain | `NseQuote::currency_option_chain_raw`/`currency_option_chain_csv` |

Bhavcopy and F&O bhavcopy both automatically pick the right format for the date requested - NSE changed both formats on 2024-07-08, and callers don't need to know or care which side of that date they're asking about.

All the live/quote endpoints above were built and verified while NSE's market was closed. They work correctly for a closed market, but some intraday-only behavior (e.g. whether `market-turnover`'s same-day figures populate, whether order book depth actually has bid/ask quotes, whether snapshot values move) hasn't been confirmed against a real trading session yet - see [`docs/nse-findings.md`](docs/nse-findings.md#live-endpoints-have-only-been-verified-while-the-market-was-closed) for what specifically still needs a spot-check during NSE's trading hours (9:15-15:30 IST, Monday-Friday).

Per-symbol live quotes and option chains are **not** behind Akamai bot detection, contrary to what an earlier version of this README claimed - NSE had just moved those endpoints to different URLs than the ones first tried here. See [`docs/nse-findings.md`](docs/nse-findings.md) for the full story.

## Pending

- **The `dataframe`/`polars` Cargo feature** - declared in `Cargo.toml` but unused; would add optional `Vec<Row>` → `polars::DataFrame` conversions on top of the fetchers that already exist, not a new data source
- **`chart_data`/`tick_data`, `eq_derivative_turnover`, `block_deal_session`, `top_stocks`** (top gainers/losers/most-active) - confirmed reachable live, not yet designed/built. `chart_data`/`tick_data` return an empty shell even via Python's own library while the market's closed, so their real shape is still unverified.

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

Run `jugaad --help` for the full, always-current command list (24 commands
as of this writing - bhavcopy in three variants, stock/index/derivatives
history, bulk deals, generic daily reports, index P/E/TRI/discovery, live
market status/index snapshot/turnover/F&O, and live per-symbol
quotes/option chains). A few representative examples:

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
