# jugaad-rs

[![CI](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Am1n1602/jugaad-rs/actions/workflows/ci.yml)

A Rust rewrite of [`jugaad-data`](https://github.com/jugaad-py/jugaad-data), a Python library for downloading historical and live market data from NSE (National Stock Exchange of India). This project follows the same behavior where it makes sense, but is written idiomatically in Rust rather than as a line-by-line port - see [`docs/nse-findings.md`](docs/nse-findings.md) for the API details this rewrite has had to work out from scratch, since NSE's endpoints have no official documentation.

## What's implemented

| Data | Function |
|---|---|
| Daily bhavcopy (whole-market OHLC) | `NseArchives::bhavcopy_raw`/`bhavcopy_save` |
| F&O daily bhavcopy | `NseArchives::bhavcopy_fo_raw`/`bhavcopy_fo_save` |
| "Full" bhavcopy (every series + delivery data) | `NseArchives::full_bhavcopy_raw`/`full_bhavcopy_save` |
| Per-stock daily OHLCV history | `NseHistory::stock_history_raw`/`stock_history_csv` |
| Per-index daily OHLC history | `NseIndexHistory::index_history_raw`/`index_history_csv` |
| F&O (futures/options) price + open-interest history | `NseHistory::derivatives_history_raw`/`derivatives_history_csv` |

Bhavcopy automatically picks the right format for the date requested - NSE changed its bhavcopy format on 2024-07-08, and callers don't need to know or care which side of that date they're asking about.

## Pending

Nothing below has any code written yet:

- **Bulk deals** - NSE's daily bulk-deal report
- **NSE's generic daily-reports downloader** - the Python original's `NSEDailyReports` subsystem for browsing/downloading any of NSE's 39+ report types, not just bhavcopy
- **Index P/E, P/B and dividend-yield data**, **Total Return Index** values, and the **index-name discovery endpoints** (`index_type_list`/`index_subtype_list`/`index_name_list`) - all on niftyindices.com, extending the client `NseIndexHistory` already provides
- **Live quotes** - real-time data, as opposed to everything above which is historical
- **RBI current rates** - a completely separate site/module in the Python original
- **Disk caching** - the Python original caches responses to disk; deliberately deprioritized here since a compiled Rust binary doesn't pay the interpreter-startup cost that makes caching worthwhile in Python
- **The `dataframe`/`polars` Cargo feature** - declared in `Cargo.toml` but unused; would add optional `Vec<Row>` → `polars::DataFrame` conversions on top of the fetchers that already exist, not a new data source

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

```
Commands:
  version        Print the current version
  bhavcopy       Download NSE's daily bhavcopy (whole-market OHLC data) for one date
  bhavcopy-fo    Download NSE's daily F&O (derivatives) bhavcopy for one date
  full-bhavcopy  Download NSE's "full" bhavcopy (every series, with delivery data)
  stock          Download a stock's daily price/volume history over a date range
  index          Download an index's daily OHLC history over a date range
  derivatives    Download F&O (futures/options) daily price and open-interest history
```

```bash
# Whole-market bhavcopy for one day
jugaad bhavcopy 2024-08-01

# A stock's price history over a range
jugaad stock SBIN --from 2024-08-01 --to 2024-08-31

# An index's price history
jugaad index "NIFTY 50" --from 2024-08-01 --to 2024-08-31

# A futures contract's history
jugaad derivatives NIFTY --from 2024-12-01 --to 2024-12-05 --expiry 2024-12-26 --instrument fut-idx
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

MIT, per `Cargo.toml`.
