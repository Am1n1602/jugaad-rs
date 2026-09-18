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
- [`bhavcopy-fo`](#bhavcopy-fo) - download the whole F&O market's daily bhavcopy for one date
- [`full-bhavcopy`](#full-bhavcopy) - download the "full" bhavcopy (every series, with delivery data) for one date
- [`bulk-deals`](#bulk-deals) - download the current bulk deals report
- [`list-daily-reports`](#list-daily-reports-daily-report) - list NSE's 39+ generic daily reports for a segment
- [`daily-report`](#list-daily-reports-daily-report) - download one generic daily report by file key
- [`stock`](#stock) - download one stock's daily price/volume history over a date range
- [`index`](#index) - download one index's daily OHLC history over a date range
- [`index-pe`](#index-pe) - download one index's daily P/E, P/B and dividend yield over a date range
- [`index-tri`](#index-tri) - download one index's daily Total Return Index values over a date range
- [`index-types`](#index-types-index-subtypes-index-names) - list index categories, sub-categories, and names for browsing what's available
- [`derivatives`](#derivatives) - download F&O price/open-interest history for one contract
- [`market-status`](#market-status) - show whether each market segment is currently open
- [`index-snapshot`](#index-snapshot) - download a live snapshot of every NSE index
- [`market-turnover`](#market-turnover) - download market-wide turnover by segment
- [`live-fo`](#live-fo) - download a live snapshot of NIFTY index futures/options
- [`block-deal-session`](#block-deal-session) - download today's block deals (pre-open and mid-day sessions)
- [`stock-quote`](#stock-quote) - download a stock's live quote, including order book depth
- [`derivative-quote`](#derivative-quote) - download every F&O contract for a symbol
- [`index-quote`](#index-quote) - download a single index's live value, volume and turnover
- [`option-chain`](#option-chain) - download an index or equity's option chain
- [`currency-option-chain`](#currency-option-chain) - download a currency pair's option chain

---

## `version`

Prints the `jugaad-core` library version.

```bash
jugaad version
```

```
jugaad-core 0.2.0
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

## `bhavcopy-fo`

Downloads NSE's daily F&O (futures & options market) bhavcopy - OHLC and
open-interest data for every derivatives contract traded on a given day -
and saves it as a CSV.

```bash
jugaad bhavcopy-fo [OPTIONS] <DATE>
```

### Arguments

| Argument | Description |
|---|---|
| `<DATE>` | Trading date to fetch, in `yyyy-mm-dd` format (e.g. `2024-08-01`) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/fo_bhavcopy` | Directory to save the CSV into |

### Examples

```bash
jugaad bhavcopy-fo 2024-08-01
```

```
Saved F&O bhavcopy to data/nse/fo_bhavcopy/fo01Aug2024bhav.csv
```

### Notes

- Same auto-dispatch behavior as `bhavcopy`: NSE switched F&O bhavcopy to
  the UDiff format on the same date, 2024-07-08, and this command picks
  the right format automatically either way.
- Same "no data" behavior as `bhavcopy` for weekends/holidays/unpublished
  dates.

---

## `full-bhavcopy`

Downloads NSE's "full" bhavcopy - like `bhavcopy`, but covering every
series (not just ordinary equity - includes government securities,
trade-for-trade, etc.) and adding delivery quantity/percentage columns
`bhavcopy` doesn't have. Saved as a CSV; this one is served unzipped
directly by NSE.

```bash
jugaad full-bhavcopy [OPTIONS] <DATE>
```

### Arguments

| Argument | Description |
|---|---|
| `<DATE>` | Trading date to fetch, in `yyyy-mm-dd` format (e.g. `2024-08-01`) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/full_bhavcopy` | Directory to save the CSV into |

### Examples

```bash
jugaad full-bhavcopy 2024-08-01
```

```
Saved full bhavcopy to data/nse/full_bhavcopy/sec_bhavdata_full_01Aug2024.csv
```

### Notes

- Only one format exists for this endpoint (no old/UDiff split), but it
  doesn't go back as far as `bhavcopy` does - dates before roughly 2020
  fail with a "no data" error rather than succeeding.
- The `DELIV_QTY`/`DELIV_PER` columns use a literal `-` for series where
  delivery data doesn't apply, rather than leaving the field blank.

---

## `bulk-deals`

Downloads NSE's current bulk deals report and saves it as a CSV. Unlike
every other command in this manual, there's no date to give - NSE only
serves the latest snapshot at a fixed URL, so this always fetches
whatever's current right now.

```bash
jugaad bulk-deals [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/bulk_deals.csv` | File path to save the CSV to |

Note `--output` here is a **file path**, not a directory, unlike every
other command's `--output` flag - there's no date to derive a filename
from, so you name the file directly.

### Examples

```bash
jugaad bulk-deals

jugaad bulk-deals --output ./deals-today.csv
```

```
Saved bulk deals to data/nse/bulk_deals.csv
```

### Notes

- Always overwrites the target file, unlike the bhavcopy-family commands
  which skip re-downloading if the file already exists - the data here
  can change intraday, so re-running later the same day is expected to
  give you fresher content, not a cached copy.

### CSV columns

`Date, Symbol, Security Name, Client Name, Buy/Sell, Quantity Traded, Trade Price / Wght. Avg. Price, Remarks`

---

## `list-daily-reports`, `daily-report`

NSE publishes 39+ different daily report types (bulk deals, volatility,
block deals, short selling, circuit breaker updates, VaR margin files,
and many more) through one generic API, identified by a `fileKey` rather
than each needing its own URL. **Only today's and yesterday's files are
ever available this way** - there's no historical access through this
API, unlike `bhavcopy`/`stock`/etc.

Use them together: `list-daily-reports` tells you what file keys exist
for a segment, `daily-report` downloads one of them.

```bash
jugaad list-daily-reports [OPTIONS]
jugaad daily-report [OPTIONS] <FILE_KEY>
```

### Options

| Flag | Default | Applies to | Description |
|---|---|---|---|
| `-s, --segment <SEGMENT>` | `CM` | both | Market segment, e.g. `CM` (capital market) or `FO` (derivatives) |
| `-o, --output <OUTPUT>` | `data/nse/daily_reports` | `daily-report` only | Directory to save the file into |

### Arguments

| Argument | Applies to | Description |
|---|---|---|
| `<FILE_KEY>` | `daily-report` only | A file key from `list-daily-reports`, e.g. `CM-BULK-DEAL` |

### Examples

```bash
jugaad list-daily-reports
```
```
CM-VAR-BEGIN-DAY (VaR Begin Day File)
  2024-08-01  1.05 MB  C_VAR1_01082024_1.DAT
  2024-07-31  1.05 MB  C_VAR1_31072024_1.DAT
CM-BULK-DEAL (Bulk Deals (csv))
  2024-08-01  0.00 KB  bulk.csv
...
```

```bash
jugaad daily-report CM-BULK-DEAL
```
```
Saved CM-BULK-DEAL to data/nse/daily_reports/bulk.csv
```

### Notes

- `daily-report` saves under **NSE's own filename** for the report, not
  one this tool invents - unlike every other command here, there's no
  date/range to build a filename from, and the correct file extension
  depends entirely on which report you asked for.
- Report formats vary by file key: CSV, zip, and proprietary `.DAT` files
  have all been seen. `daily-report` writes whatever bytes NSE returns
  as-is, with no assumption about content type.
- If `<FILE_KEY>` doesn't exist for the given `--segment`, the command
  fails with a "not found" error (not the same "no data" error other
  commands use for holidays/weekends - this one really means the key was
  wrong, not that a date has no data).

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
- This command covers OHLC only - see `index-pe` and `index-tri` for P/E/
  P/B/dividend-yield and Total Return Index data.
- The filename is built directly from `<NAME>`, so index names containing
  characters that aren't valid in filenames on your OS aren't handled
  specially; ordinary index names like `"NIFTY 50"` are fine.

---

## `index-pe`

Downloads an index's daily P/E, P/B and dividend yield over a date range
and saves it as a CSV.

```bash
jugaad index-pe [OPTIONS] --from <FROM> --to <TO> <NAME>
```

### Arguments

| Argument | Description |
|---|---|
| `<NAME>` | Index name, e.g. `"NIFTY 50"` |

### Options

| Flag | Default | Description |
|---|---|---|
| `-f, --from <FROM>` | *(required)* | Start date (inclusive), `yyyy-mm-dd` |
| `-t, --to <TO>` | *(required)* | End date (inclusive), `yyyy-mm-dd` |
| `-o, --output <OUTPUT>` | `data/nse/index_pe_history` | Directory to save the CSV into |

### Examples

```bash
jugaad index-pe "NIFTY 50" --from 2024-08-01 --to 2024-08-31
```

```
Saved index P/E history to data/nse/index_pe_history/NIFTY 50-pe-2024-08-01-2024-08-31.csv
```

### CSV columns

`index_name, date, pe, pb, div_yield`

### Notes

- Same chunking and empty-result behavior as `index`.

---

## `index-tri`

Downloads an index's daily Total Return Index values over a date range
and saves it as a CSV.

```bash
jugaad index-tri [OPTIONS] --from <FROM> --to <TO> <NAME>
```

### Arguments

| Argument | Description |
|---|---|
| `<NAME>` | Index name, e.g. `"NIFTY 50"`. For most indices this is also the display name; for "strategy" indices it's a short internal code - pass `--index-name` for those |

### Options

| Flag | Default | Description |
|---|---|---|
| `-f, --from <FROM>` | *(required)* | Start date (inclusive), `yyyy-mm-dd` |
| `-t, --to <TO>` | *(required)* | End date (inclusive), `yyyy-mm-dd` |
| `--index-name <INDEX_NAME>` | same as `<NAME>` | Display name, only needed if different from `<NAME>` |
| `-o, --output <OUTPUT>` | `data/nse/index_tri_history` | Directory to save the CSV into |

### Examples

```bash
jugaad index-tri "NIFTY 50" --from 2024-08-01 --to 2024-08-31
```

```
Saved index TRI history to data/nse/index_tri_history/NIFTY 50-tri-2024-08-01-2024-08-31.csv
```

### CSV columns

`index_name, date, total_returns_index, ntr_value`

### Notes

- Same chunking and empty-result behavior as `index`.

---

## `index-types`, `index-subtypes`, `index-names`

Three commands for browsing what index names are available, rather than
downloading data - print results to the terminal, one per line, instead
of writing a file. Use them together, top-down: `index-types` gives you a
category to pass into `index-subtypes`, which gives you a sub-category to
pass into `index-names`, which gives you an actual index name to use with
`index`/`index-pe`/`index-tri`.

```bash
jugaad index-types
jugaad index-subtypes --index-type <INDEX_TYPE> --index-group <INDEX_GROUP>
jugaad index-names --index-type <INDEX_TYPE> --index-group <INDEX_GROUP>
```

### Options

`index-types` takes no options. `index-subtypes` and `index-names` both
take:

| Flag | Description |
|---|---|
| `--index-type <INDEX_TYPE>` | A category from `index-types` (for `index-names`, a sub-category from `index-subtypes`) |
| `--index-group <INDEX_GROUP>` | One of `"Historical Index Data"`, `"Total returns Index Values "`, `"P/E, P/B & Div.Yield values"` (note the trailing space in the second - that's niftyindices' own data, not a typo here) |

### Examples

```bash
jugaad index-types
```
```
Equity
Fixed Income
Multi Asset
```

```bash
jugaad index-subtypes --index-type Equity --index-group "Historical Index Data"
```
```
Broad Market Indices
Sectoral Indices
Strategy Indices
Thematic Indices
```

```bash
jugaad index-names --index-type "Broad Market Indices" --index-group "Historical Index Data"
```
```
NIFTY 100
NIFTY 200
NIFTY 50
...
```

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

## `market-status`

Shows whether each market segment (Capital Market, Currency, Commodity,
Debt) is currently open, and saves it as a CSV. Unlike every command
above, this isn't historical - it's a live snapshot of right now.

```bash
jugaad market-status [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/market_status.csv` | File path to save the CSV to |

Note `--output` here is a **file path**, not a directory - like
`bulk-deals`, there's no date to derive a filename from.

### Examples

```bash
jugaad market-status
```

```
Saved market status to data/nse/market_status.csv
```

### CSV columns

`market, status, trade_date, index, last, change, percent_change, status_message`

### Notes

- Always overwrites the target file - the data is live and changes
  intraday, so re-running later is expected to give fresher content.
- `last`/`change`/`percent_change` are blank while a segment is closed
  (NSE sends an empty value for them, not a stale last-known number) and
  are kept as raw text rather than parsed to numbers, since NSE sends them
  in inconsistent shapes (plain number, numeric string, or empty) across
  segments. See [nse-findings.md](nse-findings.md#marketstatuss-marketstate-array-has-no-consistent-shape).
- NSE's response also includes a few extra entries that don't name a real
  market segment (a USD-adjusted NIFTY figure, for instance) - those are
  dropped, not included in the output.
- Built and verified while the market was closed - not yet spot-checked
  during actual trading hours. See
  [nse-findings.md](nse-findings.md#live-endpoints-have-only-been-verified-while-the-market-was-closed).

---

## `index-snapshot`

Downloads a live snapshot of every NSE index - current price/change, day
range, 52-week range, valuation ratios (P/E, P/B, dividend yield) and
market breadth (advances/declines) - and saves it as a CSV, one row per
index.

```bash
jugaad index-snapshot [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/index_snapshot.csv` | File path to save the CSV to |

### Examples

```bash
jugaad index-snapshot
```

```
Saved index snapshot to data/nse/index_snapshot.csv
```

### CSV columns

`category, name, symbol, last, change, percent_change, open, high, low, prev_close, year_high, year_low, pe, pb, div_yield, advances, declines, unchanged`

### Notes

- Always overwrites the target file (live data, like `market-status`).
- `pe`/`pb`/`div_yield` are blank for indices with no meaningful ratio
  (e.g. currency-adjusted indices like NIFTY50 USD).
- `advances`/`declines`/`unchanged` are blank for indices with no
  derivatives exposure (e.g. INDIA VIX) - NSE omits them entirely rather
  than sending zero.
- This is the live counterpart to `index`/`index-pe`/`index-tri` - those
  three fetch historical data over a date range; this one is a single
  point-in-time snapshot across every index at once.
- Built and verified while the market was closed - not yet spot-checked
  during actual trading hours. See
  [nse-findings.md](nse-findings.md#live-endpoints-have-only-been-verified-while-the-market-was-closed).

---

## `market-turnover`

Downloads market-wide turnover (volume, value, open interest) by segment
as of the last completed trading session, and saves it as a CSV.

```bash
jugaad market-turnover [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/market_turnover.csv` | File path to save the CSV to |

### Examples

```bash
jugaad market-turnover
```

```
Saved market turnover to data/nse/market_turnover.csv
```

### CSV columns

`name, volume, value, open_interest`

### Notes

- Always overwrites the target file (live data, like `market-status`).
- One row has a blank `name` - a real NSE data quirk (a turnover figure
  with no label), kept rather than dropped.
- NSE's response also includes a same-day ("today") figure per segment;
  it's not included here, since it's empty/null while the market's closed
  and shaped differently for the "Total" row than every other segment.
- Built and verified while the market was closed - it's untested whether
  the dropped "today" figure actually populates during live trading. See
  [nse-findings.md](nse-findings.md#live-endpoints-have-only-been-verified-while-the-market-was-closed).

---

## `live-fo`

Downloads a live snapshot of NIFTY index futures/options and saves it as
a CSV, one row per contract.

```bash
jugaad live-fo [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/live_fo.csv` | File path to save the CSV to |

### Examples

```bash
jugaad live-fo
```

```
Saved live F&O snapshot to data/nse/live_fo.csv
```

### CSV columns

`underlying, identifier, instrument_type, instrument, contract, expiry, option_type, strike_price, last_price, change, percent_change, open, high, low, close_price, volume, turnover, underlying_value, open_interest, trades`

### Notes

- Always overwrites the target file (live data, like `market-status`).
- Takes no arguments - it always covers NIFTY index futures/options. NSE's
  API technically accepts an `index` parameter for other buckets, but
  every value tried besides the one this command uses returns an error
  server-side, so there's nothing else to expose.
- This is a whole-market NIFTY F&O snapshot, distinct from
  `derivative-quote` (below), which fetches every contract for one
  specific symbol - see
  [nse-findings.md](nse-findings.md#correction-per-symbol-live-quotes-are-not-behind-a-bot-wall---the-old-urls-were-just-dead)
  for how these two relate.
- Built and verified while the market was closed - it's untested whether
  more contracts (or NIFTY options) appear in this same bucket during
  active trading. See
  [nse-findings.md](nse-findings.md#live-endpoints-have-only-been-verified-while-the-market-was-closed).

---

## `block-deal-session`

Downloads today's block deals - large negotiated trades reported outside
the normal order book - across both trading sessions (the pre-open window
and the mid-day window), and saves them as a CSV, one row per deal.

```bash
jugaad block-deal-session [OPTIONS]
```

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/block_deal_session.csv` | File path to save the CSV to |

### Examples

```bash
jugaad block-deal-session
```

```
Saved block deal session to data/nse/block_deal_session.csv
```

### CSV columns

`session, identifier, symbol, series, market_type, change, percent_change, last_price, open, day_high, day_low, previous_close, average_price, total_traded_volume, total_traded_value, total_buy_quantity, total_sell_quantity, status, ex_date, purpose, last_update_time`

`session` is `"session1"` (pre-open negotiated-deal window) or
`"session2"` (mid-day window) - NSE returns these as two separate lists;
this command flattens them into one CSV tagged by this column.

### Notes

- Always overwrites the target file (live data, like `market-status`).
- Both sessions can legitimately be empty on a day with no block deals -
  that's not an error, just an empty (or header-only) file.
- `status`/`ex_date`/`purpose` were blank in every deal seen live - kept
  in the output in case they populate for a deal tied to a corporate
  action. See
  [nse-findings.md](nse-findings.md#block-deals-two-separate-session-lists-flattened-into-one-tagged-vec).

---

## `stock-quote`

Downloads a stock's live quote - price/change, day and 52-week range,
traded volume/value/delivery, and 5-level order book depth - and saves it
as a one-row CSV.

```bash
jugaad stock-quote [OPTIONS] <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Stock symbol, e.g. `SBIN` or `TCS` |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/stock_quote` | Directory to save the CSV into |

### Examples

```bash
jugaad stock-quote SBIN
```

```
Saved stock quote to data/nse/stock_quote/SBIN-quote.csv
```

### CSV columns

`symbol, company_name, series, open, day_high, day_low, previous_close, last_price, change, percent_change, year_high, year_low, total_traded_volume, total_traded_value, total_market_cap, face_value, delivery_quantity, delivery_pct, buy_price_1, buy_quantity_1, sell_price_1, sell_quantity_1, ... (through level 5), total_buy_quantity, total_sell_quantity, last_update_time`

The order book has no natural CSV shape as a nested array, so each of the
5 depth levels gets its own numbered columns (`buy_price_1`/
`sell_price_1` through `buy_price_5`/`sell_price_5`) rather than being
dropped.

### Notes

- Always overwrites the target file (live data, like `market-status`).
- Fails with a "not found" error for an unknown symbol - confirmed live,
  this endpoint 404s rather than returning an empty result.
- NSE's real response also carries ~70 more fields (compliance/margin
  data mostly relevant to debt securities, not equities) that aren't
  included here. See
  [nse-findings.md](nse-findings.md#correction-per-symbol-live-quotes-are-not-behind-a-bot-wall---the-old-urls-were-just-dead).

---

## `derivative-quote`

Downloads every F&O contract (all expiries, all strikes, futures and
options alike) for a single underlying symbol, and saves it as a CSV, one
row per contract.

```bash
jugaad derivative-quote [OPTIONS] <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Symbol, e.g. `NIFTY` (index) or `RELIANCE` (stock) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/derivative_quote` | Directory to save the CSV into |

### Examples

```bash
jugaad derivative-quote NIFTY
```

```
Saved derivative quote to data/nse/derivative_quote/NIFTY-derivative-quote.csv
```

### CSV columns

`underlying, identifier, instrument_type, expiry, option_type, strike_price, last_price, change, percent_change, open, high, low, prev_close, close_price, volume, turnover, underlying_value, open_interest, change_in_open_interest, percent_change_in_open_interest`

`strike_price` is `0` and `option_type` is `"XX"` for futures rows, same
sentinel convention as `derivatives`/`live-fo`.

### Notes

- Returns an empty file (no header row) for an unknown symbol, rather than
  an error - confirmed live.
- Unlike `derivatives` (which needs `--expiry`/`--instrument`/date range
  flags), this fetches everything available for the symbol at once - no
  filtering options.

---

## `index-quote`

Downloads a single index's live value, volume and turnover, and saves it
as a one-row CSV.

```bash
jugaad index-quote [OPTIONS] <NAME>
```

### Arguments

| Argument | Description |
|---|---|
| `<NAME>` | Index name, e.g. `"NIFTY 50"` (quote it - it contains a space) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/index_quote` | Directory to save the CSV into |

### Examples

```bash
jugaad index-quote "NIFTY 50"
```

```
Saved index quote to data/nse/index_quote/NIFTY 50-quote.csv
```

### CSV columns

`name, last, change, percent_change, open, day_high, day_low, previous_close, year_high, year_low, total_traded_volume, total_traded_value, last_update_time`

### Notes

- Fails with a "not found" error for an unknown index name - unlike
  `index`/`index-pe`/`index-tri` (which write an empty file for an unknown
  name), there's no "market closed" ambiguity for a live lookup, so an
  empty result unambiguously means the name was wrong. See
  [nse-findings.md](nse-findings.md#correction-per-symbol-live-quotes-are-not-behind-a-bot-wall---the-old-urls-were-just-dead).
- Different data from `index-snapshot` (adds traded volume/value, drops
  P/E-P/B-dividend-yield and market breadth) - not a filtered view of the
  same endpoint.

---

## `option-chain`

Downloads the option chain for an index or an equity, and saves it as a
CSV, one row per contract (call and put legs are separate rows,
distinguished by `option_type`).

```bash
jugaad option-chain [OPTIONS] --kind <KIND> <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Symbol, e.g. `NIFTY` (index) or `SBIN` (equity) |

### Options

| Flag | Default | Description |
|---|---|---|
| `-k, --kind <KIND>` | *(required)* | `index` or `equity` |
| `-e, --expiry <EXPIRY>` | nearest available | Expiry date, `yyyy-mm-dd` |
| `-o, --output <OUTPUT>` | `data/nse/option_chain` | Directory to save the CSV into |

### Examples

```bash
# Nearest expiry
jugaad option-chain NIFTY --kind index

# A specific expiry
jugaad option-chain SBIN --kind equity --expiry 2026-09-29
```

```
Saved option chain to data/nse/option_chain/NIFTY-index-optionchain-2026-09-22.csv
```

The filename includes the resolved expiry (even when `--expiry` was
omitted and the nearest one was looked up automatically), so different
expiries for the same symbol don't overwrite each other's files.

### CSV columns

`strike_price, expiry, option_type, identifier, last_price, change, percent_change, open_interest, change_in_open_interest, percent_change_in_open_interest, total_traded_volume, implied_volatility, buy_price, buy_quantity, sell_price, sell_quantity, total_buy_quantity, total_sell_quantity, underlying_value`

### Notes

- A strike with no real contract on one side (common for deep in/out-of-
  the-money equity strikes) is skipped entirely for that side, rather than
  written as a mostly-empty row - confirmed live. See
  [nse-findings.md](nse-findings.md#correction-per-symbol-live-quotes-are-not-behind-a-bot-wall---the-old-urls-were-just-dead).
- Without `--expiry`, the nearest available expiry is looked up
  automatically via a separate request - matching the Python original's
  default behavior.

---

## `currency-option-chain`

Downloads a currency pair's option chain, and saves it as a CSV, one row
per contract - same layout idea as `option-chain`, but currency legs carry
`bid`/`ask` fields instead of `buy`/`sell`.

```bash
jugaad currency-option-chain [OPTIONS] <SYMBOL>
```

### Arguments

| Argument | Description |
|---|---|
| `<SYMBOL>` | Currency pair symbol, e.g. `USDINR` |

### Options

| Flag | Default | Description |
|---|---|---|
| `-o, --output <OUTPUT>` | `data/nse/currency_option_chain` | Directory to save the CSV into |

### Examples

```bash
jugaad currency-option-chain USDINR
```

```
Saved currency option chain to data/nse/currency_option_chain/USDINR-currency-optionchain.csv
```

### CSV columns

`strike_price, expiry, option_type, identifier, last_price, change, percent_change, open_interest, change_in_open_interest, percent_change_in_open_interest, total_traded_volume, implied_volatility, bid_price, bid_quantity, ask_price, ask_quantity, total_buy_quantity, total_sell_quantity, underlying_value`

### Notes

- Unlike `option-chain`, always fetches every expiry at once (NSE's
  currency option-chain endpoint doesn't take an expiry filter) - no
  `--expiry` flag.
- Same no-real-contract skip behavior as `option-chain`.

---

## Errors

Errors are printed as a plain message and exit with a non-zero status.
The full list, from [`error.rs`](../crates/jugaad-core/src/error.rs):

| Message | Cause |
|---|---|
| `network request failed: ...` | Couldn't reach NSE at all (DNS, timeout, connection reset, ...) |
| `NSE refused the request, reason can be bot protection or an invalid session` | NSE's bot protection blocked the request |
| `Data not published due to holiday,weekend or not released yet` | `bhavcopy` was asked for a non-trading day |
| `not found: ...` | `daily-report` was asked for a `<FILE_KEY>` that doesn't exist for that segment |
| `unexpected HTTP status from NSE: ...` | NSE returned something other than success/not-found/forbidden |
| `failed to parse response: ...` | NSE's response wasn't shaped as expected (e.g. an undocumented format change) |
| `io error: ...` | A local filesystem problem (e.g. no permission to write the output directory) |
| `a concurrent fetch task panicked or was cancelled: ...` | Internal bug in a `stock`/`index`/`derivatives` chunk-fetch task; please report this |
| `failed to write CSV: ...` | Something went wrong writing the output file |
