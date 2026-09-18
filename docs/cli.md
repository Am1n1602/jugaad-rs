# `jugaad` CLI manual

The `jugaad` binary (crate: `jugaad-cli`) is a command-line interface for
downloading Indian stock market data from NSE. It wraps the `jugaad-core`
library.

Run any command with `-h`/`--help` to see this same information from the
tool itself; it's generated from the same source, so it will never drift
from what's documented here.

```bash
cargo run -p jugaad-cli -- <COMMAND> [OPTIONS]
```

(or, once built, `jugaad <COMMAND> [OPTIONS]` directly)

## Commands

- [`version`](#version) - print the library version
- [`bhavcopy`](#bhavcopy) - download the whole market's daily OHLC data for one date
- [`stock`](#stock) - download one stock's daily price/volume history over a date range
- [`index`](#index) - download one index's daily OHLC history over a date range
- [`derivatives`](#derivatives) - download F&O price/open-interest history for one contract

---

## `version`

Prints the `jugaad-core` library version.

```bash
jugaad version
```

```
jugaad-core 0.1.0
```

---

## `bhavcopy`

Downloads NSE's daily bhavcopy - a single file containing OHLC (open,
high, low, close) data for every stock traded on a given day - and saves
it as a CSV.

```bash
jugaad bhavcopy [OPTIONS] <DATE>
```

### Arguments

| Argument | Description |
|---|---|
| `<DATE>` | Trading date to fetch, in `yyyy-mm-dd` format (e.g. `2024-08-01`) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/daily_bhavcopy` | Directory to save the CSV into |

### Examples

```bash
# Save to the default directory
jugaad bhavcopy 2024-08-01

# Save somewhere specific
jugaad bhavcopy 2024-08-01 --output ./my-data
```

```
Saved bhavcopy to data/nse/daily_bhavcopy/cm01Aug2024bhav.csv
```

### Notes

- Works for any trading day, old or new. NSE switched bhavcopy to a new
  "UDiff" format on **2024-07-08** with different columns than before
  (the old format has an `ISIN` column the new one doesn't); this command
  picks the right one automatically based on `<DATE>`, so you don't need
  to do anything differently either way. See
  [nse-findings.md](nse-findings.md#bhavcopy-format-changed-on-2024-07-08).
- If `<DATE>` falls on a weekend, holiday, or a day NSE hasn't published
  data for yet, the command fails with a "no data" error rather than
  producing an empty or corrupt file.
- If the file already exists at the target path, it's reused rather than
  re-downloaded.

---

## `stock`

Downloads a stock's daily price/volume history over a date range and saves
it as a CSV, with one row per trading day.

```bash
jugaad stock [OPTIONS] --from <FROM> --to <TO> <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Stock symbol, e.g. `SBIN` or `TCS` |

### Options

| Flag | Default | Description |
|---|---|---|
| `-f, --from <FROM>` | *(required)* | Start date (inclusive), `yyyy-mm-dd` |
| `-t, --to <TO>` | *(required)* | End date (inclusive), `yyyy-mm-dd` |
| `-s, --series <SERIES>` | `EQ` | NSE series - `EQ` is ordinary equity shares |
| `-o, --output <OUTPUT>` | `data/nse/stock_history` | Directory to save the CSV into |

### Examples

```bash
# One month of SBIN's ordinary equity trading
jugaad stock SBIN --from 2024-08-01 --to 2024-08-31

# A different series
jugaad stock SBIN --from 2024-08-01 --to 2024-08-31 --series BE

# Custom output directory
jugaad stock TCS --from 2024-01-01 --to 2024-12-31 --output ./tcs-2024
```

```
Saved stock history to data/nse/stock_history/SBIN-2024-08-01-2024-08-31-EQ.csv
```

### CSV columns

`symbol, series, date, open, high, low, prev_close, ltp, close, vwap, volume, value, trades, delivery_qty, delivery_pct`

`trades`, `delivery_qty` and `delivery_pct` may be blank for very old
records - NSE didn't track these in its earlier history.

### Notes

- A date range spanning multiple calendar months is automatically split
  into per-month requests and fetched concurrently (up to 2 at a time) -
  you don't need to do anything differently for long ranges.
- `--series` filters results to exactly that series. Pass `--series ALL`
  to get every series NSE has for that symbol/date instead of just one.
  See [nse-findings.md](nse-findings.md#seriesall-returns-every-series-not-just-the-default-one)
  for why this matters.
- If the range has no trading days (e.g. it only covers a weekend) or the
  symbol doesn't exist, the command still succeeds, but writes a
  completely empty file (no header row either) - the CSV writer only
  emits headers once it has at least one row to write. NSE's API doesn't
  distinguish "no trading happened" from "bad symbol," so this tool can't
  either.

---

## `index`

Downloads an index's daily OHLC history over a date range and saves it as
a CSV, with one row per trading day. NSE indices (e.g. "NIFTY 50") are
served from a different site (niftyindices.com) than stocks and bhavcopy.

```bash
jugaad index [OPTIONS] --from <FROM> --to <TO> <NAME>
```

### Arguments

| Argument | Description |
|---|---|
| `<NAME>` | Index name, e.g. `"NIFTY 50"` (quote it - it contains a space) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-f, --from <FROM>` | *(required)* | Start date (inclusive), `yyyy-mm-dd` |
| `-t, --to <TO>` | *(required)* | End date (inclusive), `yyyy-mm-dd` |
| `-o, --output <OUTPUT>` | `data/nse/index_history` | Directory to save the CSV into |

### Examples

```bash
# One month of NIFTY 50
jugaad index "NIFTY 50" --from 2024-08-01 --to 2024-08-31

# Custom output directory
jugaad index "NIFTY BANK" --from 2024-01-01 --to 2024-12-31 --output ./banknifty-2024
```

```
Saved index history to data/nse/index_history/NIFTY 50-2024-08-01-2024-08-31.csv
```

### CSV columns

`index_name, date, open, high, low, close`

### Notes

- Same chunking behavior as `stock`: multi-month ranges are automatically
  split and fetched concurrently (up to 2 at a time).
- Same empty-result behavior as `stock` too: an unknown index name and a
  trading-day-free range both succeed but write a completely empty file
  (no header row) - niftyindices' API doesn't distinguish the two cases.
- Only OHLC history is supported - P/E, P/B, dividend yield, and Total
  Return Index data are not available through this command yet.
- The filename is built directly from `<NAME>`, so index names containing
  characters that aren't valid in filenames on your OS aren't handled
  specially; ordinary index names like `"NIFTY 50"` are fine.

---

## `derivatives`

Downloads price/open-interest history for one F&O (futures & options)
contract over a date range and saves it as a CSV, with one row per
trading day.

```bash
jugaad derivatives [OPTIONS] --from <FROM> --to <TO> --expiry <EXPIRY> --instrument <INSTRUMENT> <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Symbol, e.g. `NIFTY` (index) or `RELIANCE` (stock) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-f, --from <FROM>` | *(required)* | Start date (inclusive), `yyyy-mm-dd` |
| `-t, --to <TO>` | *(required)* | End date (inclusive), `yyyy-mm-dd` |
| `-e, --expiry <EXPIRY>` | *(required)* | Contract expiry date, `yyyy-mm-dd` |
| `-i, --instrument <INSTRUMENT>` | *(required)* | One of `fut-idx`, `fut-stk`, `opt-idx`, `opt-stk` |
| `-p, --strike-price <STRIKE_PRICE>` | *(required for options)* | Strike price, e.g. `24000` |
| `--option-type <OPTION_TYPE>` | *(required for options)* | `call` or `put` |
| `-o, --output <OUTPUT>` | `data/nse/derivatives_history` | Directory to save the CSV into |

`--strike-price` and `--option-type` are only valid (and required) for
`opt-idx`/`opt-stk`; the command errors out immediately if either is
missing for an option instrument, or ignores them if given for a future.

### Examples

```bash
# NIFTY index futures
jugaad derivatives NIFTY --from 2024-12-01 --to 2024-12-05 --expiry 2024-12-26 --instrument fut-idx

# NIFTY 24000 call option
jugaad derivatives NIFTY --from 2024-12-01 --to 2024-12-05 --expiry 2024-12-26 \
  --instrument opt-idx --strike-price 24000 --option-type call

# A stock future
jugaad derivatives RELIANCE --from 2024-12-01 --to 2024-12-05 --expiry 2024-12-26 --instrument fut-stk
```

```
Saved derivatives history to data/nse/derivatives_history/NIFTY-2024-12-01-2024-12-05-2024-12-26-OPTIDX-24000-CE.csv
```

The filename includes the strike price and option type for options (not
just futures), so different contracts for the same symbol/expiry/range
don't overwrite each other's files.

### CSV columns

`instrument, symbol, expiry, strike_price, option_type, date, open, high, low, close, ltp, prev_close, settle_price, volume, value, open_interest, change_in_oi, market_lot, underlying_value`

`strike_price` is `0` and `option_type` is `XX` for futures rows, since
neither applies to a future. `underlying_value` is blank for index
instruments (e.g. NIFTY) but populated for stock instruments.

### Notes

- Same chunking behavior as `stock`/`index`: multi-month ranges are split
  and fetched concurrently.
- Same empty-result behavior too: an expiry date that doesn't exist for
  the symbol succeeds but writes a completely empty file.
- `change_in_oi` can be negative (open interest shrinking day over day) -
  don't assume it's an unsigned count.

---

## Errors

Errors are printed as a plain message and exit with a non-zero status.
The full list, from [`error.rs`](../crates/jugaad-core/src/error.rs):

| Message | Cause |
|---|---|
| `network request failed: ...` | Couldn't reach NSE at all (DNS, timeout, connection reset, ...) |
| `NSE refused the request, reason can be bot protection or an invalid session` | NSE's bot protection blocked the request |
| `Data not published due to holiday,weekend or not released yet` | `bhavcopy` was asked for a non-trading day |
| `unexpected HTTP status from NSE: ...` | NSE returned something other than success/not-found/forbidden |
| `failed to parse response: ...` | NSE's response wasn't shaped as expected (e.g. an undocumented format change) |
| `io error: ...` | A local filesystem problem (e.g. no permission to write the output directory) |
| `a concurrent fetch task panicked or was cancelled: ...` | Internal bug in a `stock`/`index`/`derivatives` chunk-fetch task; please report this |
| `failed to write CSV: ...` | Something went wrong writing the output file |
