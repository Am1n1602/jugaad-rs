# NSE API findings

A running log of undocumented NSE behavior discovered while building
`jugaad-rs`. NSE's endpoints have no official documentation, so anything
non-obvious learned by testing against the live API is recorded here as we
go, to be pulled into proper user-facing docs later.

## Bhavcopy format changed on 2024-07-08

NSE switched to a new "UDiff" (Unified Distilled File Format) bhavcopy on
this date. Dates on or after it are served at a different URL, with a
different set of columns, than older dates. `jugaad-rs`'s `bhavcopy_raw`
([archives.rs](../crates/jugaad-core/src/nse/archives.rs)) picks the right
one automatically based on the date, so callers don't need to care which
era they're asking about.

The old format is fetched from a completely different endpoint
(`GET https://www.nseindia.com/api/reports`, not the `nsearchives.nseindia.com`
host the UDiff path uses) and needs a fixed, non-date-dependent `archives`
query parameter identifying which report to fetch:
`[{"name": "CM - Bhavcopy(csv)", "type": "daily-reports", "category": "capital-market", "section": "equities"}]`.
Its CSV has different columns than UDiff - notably an `ISIN` column the
new format doesn't have - and its date format is `"01-Jan-2020"` (same
`%d-%b-%Y` shape as the trustworthy date fields elsewhere, just via a
different query param name, `date`).

Python's version defensively retries once with a cookie warm-up if this
endpoint returns 403 with no cookies set yet. Tested live with a
completely fresh session (no cookies at all): it returned a normal 200
directly, no warm-up needed. `jugaad-rs` doesn't implement that retry -
if NSE starts requiring it again, it'll surface as `Error::Blocked`
rather than being silently retried.

Both a weekend date and a post-cutover date requested through the old
endpoint return the same 404 - consistent with the ambiguous-empty/no-data
pattern seen everywhere else in this API.

This endpoint has also shown occasional one-off flakiness independent of
any of the above - the exact same request that just worked via `curl`
failed once via `jugaad-rs` moments later, then succeeded again on retry
with no code change in between. Worth knowing if a `#[ignore]`d live test
against it fails once - re-run before assuming something broke.

## F&O bhavcopy uses the same UDiff cutover as equities

`bhavcopy_fo_raw` mirrors `bhavcopy_raw`'s auto-dispatch: NSE switched F&O
bhavcopy to UDiff format on the **same date**, 2024-07-08 (confirmed by
testing the old-style URL right at the boundary: it 404s starting exactly
that day). The modern URL follows the same pattern as equities, just with
`fo`/`FO` in place of `cm`/`CM`:
`https://nsearchives.nseindia.com/content/fo/BhavCopy_NSE_FO_0_0_0_{yyyymmdd}_F_0000.csv.zip`.
The old-format URL is
`https://nsearchives.nseindia.com/content/historical/DERIVATIVES/{yyyy}/{MMM}/fo{dd}{MMM}{yyyy}bhav.csv.zip`
(uppercase month, e.g. `JAN`) - a third host/path pattern in this project,
after the UDiff CM path and the `/api/reports` old-CM path.

## Full bhavcopy is a different report entirely, not zipped

`full_bhavcopy_raw` (`https://nsearchives.nseindia.com/products/content/sec_bhavdata_full_{ddmmyyyy}.csv`,
note: no separators, numeric month not abbreviated) returns plain CSV
directly - no zip wrapper, unlike every other bhavcopy variant. It covers
every series (not just EQ - includes government securities under series
`"GS"`, trade-for-trade under `"BE"`, etc.) and adds delivery
quantity/percentage columns the plain bhavcopy doesn't have. Those
delivery columns use a literal `"-"` string as their missing-value
sentinel (e.g. for non-equity series), not an empty field or JSON `null`
like elsewhere in this project - worth remembering if this ever gets
parsed into typed rows instead of raw CSV text.

## Bulk deals has no date parameter at all

`bulk_deals_raw` (`https://nsearchives.nseindia.com/content/equities/bulk.csv`)
is unlike every other endpoint in this project: there's no date to pass in.
NSE only serves the current snapshot at this fixed URL - every row in the
response was the same, current date when tested live. No cookies needed,
plain CSV directly (no zip). Because there's no date, `bulk_deals_save`
takes a full file path rather than a destination directory (there's no
date to derive a filename from), and always overwrites rather than
skipping if the target already exists - unlike the other `_save` methods,
skip-if-present would silently serve stale data here, since a re-run
later the same day is expected to return different content.

There's no format migration here (one shape covers every date the
endpoint serves), but there is a hard cutoff on how far back it goes -
2018 returns 404, unlike the other bhavcopy variants which go back
years further. Python's comment about this endpoint timing out for
pre-2020 dates could not be reproduced; today it just 404s immediately.

## niftyindices uses three different body encodings, and a fourth date-field naming

Extending `NseIndexHistory` to cover P/E/P/B/dividend-yield, Total Return
Index, and the three index-discovery endpoints surfaced how inconsistent
this one site is internally:

- **History-style endpoints** (OHLC, P/E, TRI - `getHistoricaldatatabletoString`,
  `getpepbHistoricaldataDBtoString`, `getTotalReturnIndexString`) all use
  the quirky string-encoded `cinfo` described above.
- **`index_subtype_list`** (`gethistoricaltypeSubindexdata`) accepts a
  proper nested JSON body: `{"cinfo": {"indextype": ..., "indexgroup": ...}}`.
  No string-encoding trick here.
- **`index_name_list`** (`gethistoricaltypeindexdata`) wants a
  form-urlencoded body with PHP-style array field names:
  `cinfo[indextype]=...&cinfo[indexgroup]=...`, not JSON at all.

Each of the three history-style endpoints also uses a different key for
the same concept - the date field is `"HistoricalDate"` for OHLC, `"DATE"`
for P/E, and `"Date"` for TRI. Three endpoints, three spellings.

The Total Return Index endpoint (`index_tri_history_raw`) is the one place
in this project where `name` and `indexName` genuinely need to differ: for
"strategy" indices, `name` is a short internal code while `indexName` is
the display name. Every other endpoint here just uses the same value for
both.

## A truly empty POST body gets rejected - by the server, not by us

`index_type_list` (`gethistoricaltypedata1`) needs no real request body,
and Python's version sends none at all. Naively doing the same with
`reqwest` (`.body("")` or `.form(&[])`, producing a zero-byte body) gets a
`411 Length Required` from niftyindices' edge server - but the *identical*
request sent via `curl` (down to the same headers) succeeds. Comparing the
two confirmed the difference: `curl -d ''` explicitly sends
`Content-Length: 0`, while `reqwest` sends no `Content-Length` header at
all for a genuinely empty body - it seems to treat "empty body" as "no
body" rather than "a body of length zero." The server apparently
requires an explicit length header even for an empty payload. Confirmed
with a targeted test: a 1-byte body succeeds, a 0-byte body always 411s,
regardless of headers otherwise being identical.

Worked around by sending a single space (`" "`) as the body instead of
nothing - the endpoint doesn't care about body content, only that
`Content-Length` is present and non-absent. Worth remembering for any
future endpoint that "doesn't need a body" - test the truly-empty case
specifically, since it can fail in a way a byte-for-byte header comparison
against a working `curl` command won't explain on its own.

## The stock history API needs a "warm-up" request first

`GET /api/historicalOR/generateSecurityWiseHistoricalData` rejects requests
that don't carry cookies from a prior visit to a normal HTML page on
nseindia.com. A plain `GET /report-detail/eq_security` first (discarding the
response, keeping the cookies) is enough. Without it, NSE returns a blocked
response - see `Error::Blocked`.

## Two different "date" fields, only one is trustworthy

The stock history JSON has both:

- `mTIMESTAMP`: a plain trading date, e.g. `"05-Aug-2024"`.
- `CH_TIMESTAMP`: a UTC instant, e.g. `"2024-08-04T18:30:00.000Z"`.

`CH_TIMESTAMP` is always exactly 18:30 UTC (midnight IST) on the day
*before* what `mTIMESTAMP` says. It's not extra precision - it's the same
daily-resolution date, just re-encoded in a way that lands on the wrong
calendar day if read naively. `jugaad-rs` uses `mTIMESTAMP` for this reason.
(The Python `jugaad-data` library uses `CH_TIMESTAMP`, inheriting this
off-by-one-day-in-UTC quirk.)

## Some numeric fields are `null`, not just old/absent

`CH_TOTAL_TRADES`, `COP_DELIV_QTY` and `COP_DELIV_PERC` can come back as
JSON `null` (confirmed on records from 2010, before NSE tracked total trade
counts) rather than being omitted from the response. Modeled as
`Option<T>` in `StockHistoryRow`.

## Empty results are ambiguous

A weekend-only date range and a symbol that doesn't exist both come back as
`{"data": []}` with HTTP 200 - there is no way to distinguish "no trading
happened" from "you made a typo in the symbol" from the response alone.
`jugaad-rs` returns `Ok(vec![])` for both rather than an error, since
forcing this into an error type would falsely imply we know which case
occurred.

## `series=ALL` returns every series, not just the default one

To fetch the default equity series, NSE actually wants the query parameter
`series=ALL` rather than `series=EQ` (passing `EQ` literally gives
incomplete results). The important catch: `ALL` really does mean *all*
series active for that symbol/date, not an alias for "the default one."

Confirmed example: SBIN on 2024-08-19 has two rows for the same date -
one with `CH_SERIES: "EQ"` (volume ~10.1M) and one with `CH_SERIES: "T0"`
(volume 1), where `"T0"` was a same-day-settlement trial NSE ran on some
symbols in 2024. A caller asking for `"EQ"` would silently receive the
`"T0"` row too if the response weren't filtered afterward.

`jugaad-rs` fixes this by filtering the response to the caller's requested
series client-side (`filter_by_series` in
[history.rs](../crates/jugaad-core/src/nse/history.rs)), unless the caller
explicitly asks for `"ALL"`. As far as we can tell, the Python `jugaad-data`
library does not do this filtering, so it can return extra rows for symbols
with unusual/trial series active on a given date.

## Index history lives on a different site with its own quirks

Historical index data (e.g. "NIFTY 50") is served from **niftyindices.com**,
not nseindia.com - a separate host with its own request format, unrelated
to everything else in this module. No cookie warm-up is needed here (unlike
stock history), but it has three quirks of its own:

1. **The request body's main field is a string, not nested JSON.** The
   endpoint (`POST /BackPage/getHistoricaldatatabletoString`) expects a
   JSON body like `{"cinfo": "..."}`, where the `"..."` is itself a string
   containing what looks like a single-quoted Python dict literal - e.g.
   `"{'name': 'NIFTY 50', 'startDate': '01-Aug-2024', 'endDate': '05-Aug-2024', 'indexName': 'NIFTY 50'}"`.
   Sending proper nested JSON (`{"cinfo": {"name": "NIFTY 50", ...}}`)
   is rejected outright (redirects to an error page). Confirmed live, not
   just inherited from old Python code - the server genuinely expects this
   today. See `build_request_body` in
   [index.rs](../crates/jugaad-core/src/nse/index.rs).
2. **Numbers are sent as JSON strings**, e.g. `"OPEN":"24302.85"` instead of
   `"OPEN":24302.85` - needs a custom deserializer, same idea as the date
   parsing elsewhere in this project but converting text to `f64`.
3. **A third date format**: `"05 Aug 2024"` (space-separated) - different
   from stock history's `"05-Aug-2024"` (hyphenated). Three different NSE-
   adjacent endpoints, three different date formats so far.

The ambiguous-empty-result pattern above applies here too: an unknown index
name and a trading-day-free range both come back as `[]` with HTTP 200.

## Derivatives (F&O) history shares stock history's host and quirks

`GET /api/historicalOR/foCPV` (futures and options price/open-interest
history) lives on the same host as stock history (nseindia.com), needs the
same cookie warm-up, and inherits the exact same two-date-fields trap:
`FH_TIMESTAMP` ("05-Dec-2024") is the trustworthy plain date, while
`FH_TIMESTAMP_ORDER` ("2024-12-04T18:30:00.000Z") is the same UTC-instant
encoding that lands on the previous calendar day if read naively - just
like `mTIMESTAMP`/`CH_TIMESTAMP` for stock history. Numbers here are real
JSON numbers, not strings (unlike niftyindices).

A few things specific to this endpoint:

- **The response includes both futures and options in one uniform shape.**
  Futures rows just have `FH_STRIKE_PRICE: 0` and `FH_OPTION_TYPE: "XX"`
  as sentinel values instead of real ones. This means one Rust type
  (`DerivativeHistoryRow`) covers all four instrument types (`FUTIDX`,
  `FUTSTK`, `OPTIDX`, `OPTSTK`), rather than needing Python's separate
  futures/options header lists.
- **`FH_CHANGE_IN_OI` can be negative** (open interest can shrink day over
  day) - needs a signed integer (`i64`), unlike every other quantity field
  in this project so far.
- **`FH_UNDERLYING_VALUE` is `null` for index instruments** (NIFTY futures/
  options) **but populated for stock instruments** (confirmed with
  RELIANCE futures, which had a real value). Modeled as `Option<f64>`.
- **The expiry date must be sent uppercased** (`"26-DEC-2024"`, not
  `"26-Dec-2024"`) - only tested uppercase against the live API, so that's
  what `jugaad-rs` sends; untested whether other casings would also work.
- Same ambiguous-empty-result pattern once again: an expiry date that
  doesn't exist for a symbol returns `{"data": []}` with HTTP 200.

`jugaad-rs` also diverges from Python's API shape here: instead of a bare
`instrument_type` string plus optional `strike_price`/`option_type`
arguments checked for consistency at runtime, it uses an `Instrument` enum
where `OptIdx`/`OptStk` variants always carry their strike price and option
type - making "asked for an option without a strike price" impossible to
construct rather than a runtime error.

## The generic daily-reports API only covers today and yesterday

`NseDailyReports` (`GET https://www.nseindia.com/api/daily-reports?key={segment}`)
is a metadata API listing every report NSE currently has available for a
segment (`"CM"`, `"FO"`, ...) - confirmed 39 report types for `"CM"`,
matching the Python docstring's "39+ report types" claim exactly. Each
entry gives a `fileKey` (e.g. `"CM-BULK-DEAL"`, `"CM-VOLATILITY"`), a
`filePath`, and a `fileActlName` - concatenating the two directly gives a
working download URL, no separate base URL needed (the API's own
`filePath` value already includes the full host, complete with an odd
double slash after the domain, e.g.
`"https://nsearchives.nseindia.com//content/equities/"` - that's the API's
own data, not something `jugaad-rs` introduces).

A few things worth knowing:

- **Unlike `NseHistory`, this needs no cookie warm-up at all** - confirmed
  by hitting the metadata endpoint from a completely fresh session with
  zero prior requests, which still returned 200 with real data.
- **The response only ever has a `CurrentDay` and `PreviousDay` entry**
  (plus an unused `FutureDay`, always empty in testing) - this API
  genuinely cannot fetch older data, matching the Python docstring's "API
  supports current day and previous day only." There's no point exposing a
  `trading_date` parameter the way the Python internals technically allow,
  since Python's own higher-level `download_report` convenience function
  doesn't either - it just prefers the current day's file, falling back to
  the previous day's.
- **The same `fileKey` can appear in both `CurrentDay` and `PreviousDay`**
  with different `tradingDate`s (e.g. `CM-UDIFF-BHAVCOPY-CSV` shows up
  twice, once per day) - `jugaad-rs` searches `CurrentDay` before
  `PreviousDay` so an unqualified lookup naturally prefers today's file,
  matching Python's exact search order.
- **An unknown `segment` value doesn't error - it returns a different JSON
  shape entirely**: `{"data":[],"msg":"no data found"}` instead of the
  normal `{"PreviousDay":[...],"CurrentDay":[...],...}` shape. Modeled with
  `#[serde(default)]` on both day-list fields, so this shape just
  deserializes as "no files today or yesterday either" - the same
  ambiguous-empty-result pattern used everywhere else in this project,
  rather than a special case to detect.
- **Downloaded files can be any format** - CSV, zip, proprietary `.DAT`
  files - so `download_report_raw` returns raw bytes rather than assuming
  UTF-8 text, unlike every other fetcher in this crate. `download_report_save`
  writes under NSE's own filename for the report rather than one
  `jugaad-rs` invents, since there's no date/range to build one from and
  the correct extension depends on the file.

An unknown `fileKey` (as opposed to an unknown `segment`) is a genuinely
different situation from "no data for this date" - it means the caller
asked for something that doesn't exist at all, not that a real date came
back empty. Originally this reused `Error::NoData`, whose message
("Data not published due to holiday, weekend or not released yet") is
actively misleading for a bad file key. Caught by actually reading the
CLI's error output while testing the `daily-report` command, not by
reasoning about it up front. Fixed by adding a dedicated `Error::NotFound`
variant rather than stretching `NoData`'s meaning further.

## The old URLs were just dead

The five specific URLs really are dead. Hitting them
directly - confirmed even through Python's own authenticated `requests`
session - returns the identical 403 "Access Denied" / 404 "Resource not
found" this project saw.

NSE moved this part of its API to different URLs at some point, and the currently
installed `jugaad-data` (`pip install jugaad-data`) already follows the
move; this project's design was based on stale endpoint names instead of
reading that library's actual current source
(`jugaad_data/nse/live.py`). The real, currently-working routes:

- **Equity quote / trade info** (`stock_quote`/`trade_info` in Python):
  `GET /api/NextApi/apiClient/GetQuoteApi?functionName=getSymbolData&marketType=N&series=EQ&symbol=SBIN`
  - a completely different host path shape than the old `quote-equity`,
  returning `{"equityResponse": [...]}`.
- **F&O quote for a symbol** (`stock_quote_fno`): same NextApi URL, with
  `functionName=getSymbolDerivativesData&symbol=...` instead.
- **Single index live value** (`live_index`):
  `GET /api/equity-stock-indices?index=NIFTY 50` - note the extra hyphen
  versus the dead `equity-stockIndices`.
- **Option chains** (`index_option_chain`/`equities_option_chain`):
  `GET /api/option-chain-v3?type=Indices&symbol=NIFTY&expiry=...` - the
  expiry is normally looked up first via
  `GET /api/option-chain-contract-info?symbol=NIFTY`. Currency option
  chains use a separate, still-alive `GET /api/option-chain-currency`.
- **Chart/tick data** (`chart_data`/`tick_data`): the old
  `GET /api/chart-databyindex?index=SBINEQN` documented here is also dead
  (see the 2026-09-21 section below for the real, working replacement).
- **Market-wide derivative turnover** (`eq_derivative_turnover`):
  `GET /api/equity-stock?index=allcontracts` - a confusingly generic path
  name for what it actually returns.
- **Block deals** (`block_deal_session`): another `functionName` call
  against the same NextApi URL (`getBlockDealSession`).
- **Top gainers/losers** (`top_stocks`): NextApi's `getTopTenStock` looks
  like the right route but isn't - see the "`top_stocks` implemented via
  `live-analysis-variations`" section below for the real one.

`NextApi/apiClient/GetQuoteApi` and `equity-stock-indices` return normal
200s with real data given the same cookie-warm-up-then-`Referer` pattern
used everywhere else in this project - so this isn't a Python-specific
trick (session behavior, TLS fingerprint, etc.), just the right current
URL. This whole surface is realistically implementable in `jugaad-rs`; it
just hasn't been designed/built yet.

## Four live endpoints implemented so far, confirmed to need no bot workaround

Separately from the per-symbol surface above, these four back
`NseLiveMarket` and were confirmed live to need nothing special - no
cookie warm-up at all (unlike `historicalOR`/`foCPV` in `history.rs`),
and no bot-wall issue of any kind:

- `api/marketStatus` - open/closed per segment
- `api/allIndices` - live snapshot of every index
- `api/market-turnover` - market-wide volume/value/OI by segment
- `api/liveEquity-derivatives?index=nse50_fut` - live NIFTY F&O snapshot -
  note this is a distinct endpoint from anything in current `jugaad-data`
  (its own `live_fno()` now just calls `live_index("SECURITIES IN F&O")`
  instead), found independently while investigating this area; it works
  and stays as-is.

These four back `NseLiveMarket` in
[live.rs](../crates/jugaad-core/src/nse/live.rs).

## `marketStatus`'s `marketState` array has no consistent shape

Each entry in `api/marketStatus`'s `marketState` array is a genuinely
different shape depending on which segment it describes - not just
optional fields, but different field *names* for the same concept:

- The four named segments (Capital Market, Currency, Commodity, Debt) use
  `"variation"` for the change value; a fifth entry describing a
  USD-adjusted NIFTY figure uses `"change"` instead, and doesn't have a
  `market`/`marketStatus`/`tradeDate` key at all.
- `"last"` (and `"variation"`/`"change"`/`"percentChange"`) can be a real
  JSON number, a numeric JSON string (e.g. `"95.9600"` for a
  `currencyfuture` pseudo-segment), or an empty string meaning "not
  applicable" while that segment is closed - all three, confirmed live,
  depending on which entry.

`jugaad-rs` deserializes into a fully-optional intermediate shape first
(`RawSegment`), then keeps only the entries that actually name a `market`
(mapping into the public `MarketSegmentStatus`) - so `market_status_raw`
returns exactly the four real segments, not the extra blurbs NSE tacks
onto the same array. The numeric-ish fields are kept as `Option<String>`
rather than parsed to `f64`, since a single field can arrive in three
different JSON shapes across entries - not worth a bespoke multi-shape
number parser for what are ultimately just display values.


## `liveEquity-derivatives` only accepts one `index` value

`api/liveEquity-derivatives` takes an `index` query parameter that looks
like it should accept any of several buckets NSE's own UI seems to
reference (`banknifty_fut`, `niftyit_fut`, ...). Tested live: every value
tried other than `nse50_fut` returns HTTP 500. `live_fo_snapshot_raw`
therefore takes no parameter at all and hardcodes `index=nse50_fut`
internally, rather than exposing a query string argument that's wrong
most of the time.

Also confirmed live: the response's `value`, `totalTurnover` and
`premiumTurnOver` fields are always identical for every row - `LiveFoRow`
keeps only one (`turnover`) rather than three redundant copies.

## Two `NseQuote` bugs caught only by running the live integration tests, not the design-time curl samples

Both of these slipped past the curl-based design verification because the
one sample response captured for each endpoint happened not to exhibit
them - a reminder that a single captured sample confirms a shape is
*possible*, not that it's the *only* shape NSE sends.

- **`getSymbolDerivativesData`'s `openInterest`/`changeinOpenInterest`
  are inconsistently typed.** Most contracts in a response send them as
  plain JSON integers, but some send the identical value as a JSON float
  (e.g. `90670.0` instead of `90670`) within the *same* response. A
  strict `u64`/`i64` field fails to deserialize the float form outright.
  Fixed with `deserialize_lenient_u64`/`deserialize_lenient_i64` in
  [quote.rs](../crates/jugaad-core/src/nse/quote.rs), which parse through
  `f64` first and round - confirmed live against NIFTY's full 905-contract
  response, which contains both forms.
- **`option-chain-v3` legs with no real contract still send a `CE`/`PE`
  object, not an omitted key - but with `identifier: null`.** Confirmed
  live on deep SBIN equity strikes (e.g. strike 1190's put side): every
  numeric field in the placeholder leg is a valid `0`, but `identifier`
  (and the unused `expiryDate`/`underlying` fields) are `null`. A
  non-`Option` `identifier: String` fails to deserialize. Fixed by making
  `OptionLeg`/`CurrencyOptionLeg.identifier` an `Option<String>`, and
  CSV export (`option_chain_csv`/`currency_option_chain_csv`) skips a leg
  entirely when its `identifier` is `None`, rather than writing a
  mostly-empty row for a contract that doesn't actually exist.

Both were only caught because the live `#[ignore]`d integration tests
were actually run against real NSE data (a 905-row response for NIFTY F&O,
a 41-strike SBIN equity chain) rather than trusting the smaller hand-picked
samples used to design the types - a good example of why this project
treats those tests as load-bearing rather than a formality.

## CSV shapes for nested live-quote data

Two of the five `NseQuote` types don't have a natural one-to-one CSV
row shape, since CSV can't represent nested structures:

- **`StockQuote`'s 5-level order book** has no nested-array equivalent in
  CSV, so `stock_quote_csv` flattens it into 20 numbered columns
  (`buy_price_1`/`sell_price_1` through `buy_price_5`/`sell_price_5`)
  rather than dropping the depth data or writing 5 separate rows for one
  quote.
- **`OptionChainRow`/`CurrencyOptionChainRow`'s `call`/`put` legs** are
  "melted" into separate rows tagged by an `option_type` column (`"CE"`/
  `"PE"`), matching the one-row-per-contract convention
  `DerivativeQuoteRow`/`LiveFoRow` already use elsewhere in this crate,
  rather than writing one wide row per strike with both legs side by
  side. Legs with no real contract (see above) are skipped rather than
  written as an empty row.

## Block deals: two separate session lists, flattened into one tagged `Vec`

`NseLiveMarket::block_deal_session_raw` (`NextApi/apiClient/GetQuoteApi`
with `functionName=getBlockDealSession`, the same generic endpoint
`NseQuote` uses for stock/derivative quotes) returns today's block deals
split into `session1` (the pre-open negotiated-deal window) and
`session2` (the mid-day window):

```json
{"data": {"session1": [...], "session2": [...]}}
```

Confirmed live: at the time this was tested, `session1` was empty and
`session2` had two deals - so `session1`'s real per-deal shape hasn't
actually been observed populated, only inferred to match `session2`'s
(same field set, same generic-quote-shaped endpoint). Each deal also
repeats the `PChange`/`pChange` duplicate-field pattern seen in
`option-chain-v3` - only `pChange` is kept, same convention as elsewhere.

Rather than expose two separate lists (which has the same nested-shape
problem as everything else in this findings doc), `block_deal_session_raw`
flattens both into one `Vec<BlockDealRow>` tagged by a `session` field
(`"session1"`/`"session2"`), matching the one-row-per-record convention
used throughout this crate. `status`/`exDate`/`purpose` were `null` in
every deal observed - kept as `Option<String>` rather than dropped, since
they read like fields meant for corporate-action-linked deals this
crate hasn't seen an example of yet.

## Real bug: multi-month ranges came back in a "sawtooth" order, not chronological

Found by a user actually looking at exported CSV output, not by
reasoning about the code up front - `stock_history_raw`,
`derivatives_history_raw`, `index_history_raw`, `index_pe_history_raw`
and `index_tri_history_raw` all split a multi-month range into
calendar-month chunks (`break_into_month_chunks`), fetch them
concurrently, then concatenate the results in the order chunks were
*pushed* (ascending chronological order: earliest month first). The
concatenation code's own comment claimed this "keeps rows in
chronological order" - that assumption was never actually verified and
turned out to be wrong.

Confirmed live (both curl and Python's `requests`, ruling out a
client-specific quirk): NSE's `historicalOR` endpoints and niftyindices'
history endpoint both return each **single chunk's** rows in **descending**
date order internally (newest first), not ascending. So a 5-month request
split into 5 chunks and concatenated in push order produced a "sawtooth"
result - descending *within* each month, but ascending *across* months
(April's rows newest-to-oldest, then May's newest-to-oldest, then June's,
...) - rather than one consistently ordered sequence in either direction.

Fixed by no longer trusting either NSE's per-chunk order or the
concatenation order at all: `sort_by_date_desc` (in `dates.rs`) explicitly
sorts the fully-concatenated result by date, descending, after every
chunked fetch. This is correct regardless of how many chunks there were,
what order they completed in, or what order NSE happens to return within
a chunk - it doesn't depend on inferring NSE's internal ordering ever
again.

## `corporates-financial-results` returns a genuinely different schema per segment

`GET /api/corporates-financial-results` (NSE's older, pre-Integrated-Filing
"Regulation 33 Financial Results" filing type - the only source for
machine-readable financials before SEBI's Integrated Filing framework
existed, roughly FY2024-25 on) takes an `index` query parameter for the
listed-entity segment (`equities`, `sme`, `reitsinvits`, `insurance`,
`debt`). Confirmed live across all five: **the response shape itself
differs by segment**, not just field values - this isn't the usual
"some fields are sometimes null" pattern seen elsewhere in this crate.

- **`equities` and `sme`** share the shape `FinancialResultRow` models:
  `symbol`/`companyName`/`isin`/`consolidated`/`audited`/`fromDate`/
  `toDate`/`filingDate`/`xbrl`/etc.
- **`insurance`** has a different set of fields entirely: no `isin`, no
  `fromDate`/`toDate` (has `periodEnd` instead, a single date not a
  range), no `filingDate` - and adds `insuranceType`, `naAttach`, `ixbrl`.
- **`reitsinvits`** is different again, and its `xbrl` filenames literally
  contain the string `INTEGRATED_FILING` - this segment appears to be
  serving data from SEBI's *newer* Integrated Filing framework through
  this same older endpoint URL, with field names like `auditedUnaudited`/
  `consNoncons`/`submissionDate`/`typeOfSubmission` instead of the
  Regulation-33 names.
- **`debt`** returned zero rows in every query tried, including the
  unfiltered-bulk-pull below (which found real data immediately for every
  other segment) - genuinely untested/unconfirmed, not silently assumed
  to work.

`NseCorporateResults::financial_results_raw` therefore only supports
`equities`/`sme`; passing `insurance`/`reitsinvits`/`debt` will either
fail to deserialize or (for `debt`) just return nothing.

### The `issuer` parameter looks like a filter but is silently ignored

Confirmed live: `issuer=TCS` does not filter anything - it returns every
company's filings (30,543 rows across 2,381 distinct symbols for one
query), with a normal `200` status, not an error. The correct filter
parameter is `symbol`. `financial_results_raw` doesn't expose `issuer` as
a parameter at all, specifically to avoid this footgun.

This same "ignore the filter, return everything" behavior turned out to
be a useful tool for verification: passing a symbol that doesn't exist as
`issuer` reliably dumps every record for a segment/period, which is how
the cross-segment field-value distributions below were checked against
~49,000 real records instead of one company's ~26.

### Two fields spell the same value differently depending on segment

Across ~49,000 equities+sme records, `consolidated` and `audited` each
have exactly two values with no third - but `audited`'s "not audited"
value is spelled `"Un-Audited"` for equities and `"Unaudited"` (no
hyphen) for sme. Modeled as one `AuditStatus` enum with
`#[serde(rename = "Un-Audited", alias = "Unaudited")]` so callers don't
need to know which segment uses which spelling. `consolidated`'s two
values (`"Consolidated"`/`"Non-Consolidated"`) were spelled consistently
across every segment checked.

`cumulative` (`"Cumulative"`/`"Non-cumulative"` for equities,
`"Cumulative"`/`"Non-Cumulative"` for sme - yet another casing
inconsistency) turned out to be 100% redundant with `period` across every
one of the ~49,000 records checked (`Annual` always paired with
`Cumulative`, `Quarterly` always with the non-cumulative spelling) - not
modeled at all, rather than adding a field that duplicates `period` and
adds its own casing inconsistency on top.

### The `xbrl` field's placeholder value, and its fallback

`xbrl` is always present as a string (never missing, never JSON `null`,
confirmed across ~49,000 records) - but for filings from before real
XBRL existed, NSE sends a placeholder URL ending in `/-`
(`https://nsearchives.nseindia.com/corporate/xbrl/-`) instead of a real
file. For TCS specifically, real XBRL starts at FY2018-19 annual (filed
Apr-2019); every earlier annual filing back to FY2012-13 has the
placeholder. `FinancialResultRow::xbrl_url` normalizes the placeholder to
`None` rather than treating it as a real, downloadable URL.

`resultDetailedDataLink` (an HTML filing-detail page, not structured
data) is the fallback for the placeholder-XBRL era: confirmed live, it's
populated for 7,872 of 9,604 placeholder-XBRL annual records and *never*
for a real-XBRL record - a genuine, mostly-but-not-always-available
substitute, not noise. `resultDescription` by contrast was `null` in
every one of the ~49,000 records checked and isn't modeled at all.

Because the two download URLs come from genuinely different eras with
different content types (XML vs HTML), `NseCorporateResults` exposes them
as two separate, explicitly-named methods (`download_xbrl_raw`,
`download_result_html_raw`) rather than one URL-agnostic downloader, even
though the underlying fetch mechanics are identical (same host, no cookie
needed for either, confirmed live) - the separate names make it obvious
which era of filing a caller is meant to use each one for.

## `eq_derivative_turnover`: two top-20 leaderboards, one response, no cookie

`GET /api/equity-stock?index=allcontracts` (Python: `NSELive.eq_derivative_turnover`)
returns two parallel top-20 lists in one response - `value` (ranked by
premium turnover) and `volume` (ranked by contracts traded) - under top-level
keys `value`/`volume` (not the usual `data` envelope seen elsewhere in this
crate), plus `val_timestamp`/`vol_timestamp` companions that aren't modeled
(response-level metadata, not per-row data). The same contract can and does
appear in both lists. Confirmed live: no cookie warm-up needed, same as
every other `NseLiveMarket` endpoint.

`index=allcontracts` is the only value confirmed live; Python's method
signature defaults to it (`type="allcontracts"`) but allows overriding, so
other values may exist - untested here, and not exposed as a parameter
rather than guessing.

Two things repeat findings already on record for very similar endpoints,
confirmed again here rather than assumed to carry over:

- **`optionType` uses a third vocabulary.** `"Call"`/`"Put"`/`"-"` here,
  vs. `"CE"`/`"PE"`/`"XX"` everywhere else in this crate
  (`DerivativeHistoryRow`, `LiveFoRow`, `DerivativeQuoteRow`). Kept as a
  raw string, not forced into the shared `OptionType` enum (which has no
  "not an option" variant).
- **Count fields can be JSON floats.** `numberOfContractsTraded` and
  `openInterest` showed the same int-vs-float inconsistency already
  documented for `DerivativeQuoteRow` in `quote.rs` - reused the same
  `deserialize_lenient_u64` helper (made `pub(super)` to share it) rather
  than duplicating the fix.

Unlike `LiveFoRow`'s three redundant turnover fields, `totalTurnover` and
`premiumTurnover` here are genuinely different values (confirmed by
comparing magnitudes across several rows) - both kept.

## The market-hours retest, done live on 2026-09-21 (NSE genuinely open)

Every earlier entry in this doc that said "verified only while the market
was closed" got checked again with the market actually open (confirmed via
`marketStatus`: Capital Market `Open`, 21-Sep-2026 10:22 IST). Results,
one by one:

- **`chart_data`/`tick_data` (Python: `NSELive.chart_data`) is not gated
  by market hours at all - that hypothesis was wrong.** Confirmed with
  the market genuinely open, via both plain curl and Python's own current
  `jugaad-data` library: `GET /api/chart-databyindex` still returns the
  identical empty placeholder (`{"closePrice":0,"grapthData":[],
  "identifier":null,"name":null}`) for both an equity (`SBIN`) and an
  index (`NIFTY 50`, `indices=true`). Since Python's real library gets
  the same empty result live, this isn't a jugaad-rs bug or a timing
  issue - the endpoint itself appears broken or needs a parameter neither
  client is sending. Root cause still unknown; not designable until it
  is.
- **`stock_quote_raw`'s order book depth does populate live - confirmed.**
  SBIN's top-of-book during the open market: `buyPrice1: 993.8,
  buyQuantity1: 1176, sellPrice1: 994, sellQuantity1: 11` - real resting
  orders, not the all-zero placeholder seen every time this was tested
  with the market closed.
- **`market-turnover`'s `today` object stays empty even with the market
  open.** Confirmed: `Equities.today` is still `{}` and `Total.today`
  still has every field `null` during live trading, not just when
  closed. This makes the earlier decision to drop `today` from
  `MarketTurnoverRow` look even more correct than it did at the time -
  it may simply never populate through this endpoint, closed market or
  not.
- **`marketState`'s Currency/Commodity/Debt segments still send empty
  `last`/`variation` while `Open`.** Confirmed: all three segments
  reported `marketStatus: "Open"` with `last`/`variation` still empty
  strings. This isn't a closed-market artifact either - these three
  segments apparently never carry a snapshot value through this
  endpoint, matching Capital Market's numeric fields only ever actually
  populating for Capital Market. **Update, same day:** Commodity and Debt
  really do have no alternate source anywhere in this response - checked
  every entry in `marketState` plus the top-level keys
  (`marketcap`/`indicativenifty50`/`giftnifty`/`niftyusd`), nothing names
  either segment again. Currency is different: `marketState` also
  contains a `market: "currencyfuture"` row with a real `last` value
  (e.g. `"95.8225"`, USDINR future) - and since it names a `market`, it
  already survives `RawSegment::into_named` and comes back as its own
  entry in `market_status_raw`'s `Vec<MarketSegmentStatus>` today, no
  code change needed. A caller who wants live currency data just needs to
  look for the `currencyfuture` entry instead of `Currency`'s own (always
  empty) fields - now called out in the `live_market` example, which
  prints `last`/`change` for every segment.
- **`live_fo_snapshot_raw` still returns exactly 3 rows, all `FUTIDX`,
  during live trading** - no NIFTY options appeared in this bucket even
  with the market open, matching what was seen closed.
- **`index_snapshot_raw`'s values do move intraday** - NIFTY 50's `last`
  changed between the closed-market baseline (23346.4) and this
  live-market check (23375.85), confirming the field reflects genuine
  live movement rather than a frozen snapshot.

Net effect: everything above was already handled correctly by this
crate's design (the `today`/Currency-Commodity-Debt/live-fo behavior
needed no code change, since the types already tolerate empty/absent
values) - except `chart_data`/`tick_data`, which remains genuinely
unsolved and is no longer attributed to market hours.

## `chart_data`/`tick_data` resolved (2026-09-21): same root cause as the per-symbol quotes

`chart-databyindex` really is permanently dead - but it turns out to be
the same story as `stock_quote_raw`/`derivative_quote_raw` earlier in this
doc: NSE moved chart data behind the NextApi endpoint too, and the
currently-installed `jugaad-data` package (`jugaad_data/nse/live.py`) has
already been updated with a method for it that the library's own
`chart_data`/`tick_data` names never got pointed at:

```python
def symbol_chart_data(self, symbol, series="EQ", days="1D"):
    return self._get_nextapi("getSymbolChartData", symbol=symbol + series + "N", days=days)
```

Confirmed live via curl with the market open (21-Sep-2026, ~10:40 IST):

```
GET /api/NextApi/apiClient/GetQuoteApi?functionName=getSymbolChartData&symbol=SBINEQN&days=1D
```

returns real intraday data - `{"identifier":"SBINEQN","name":"SBIN",
"grapthData":[[1789981259000,996,"PO","-0.2","-0.02"],...],"closePrice":996.2}`
(`grapthData` is NSE's own misspelling, not a typo introduced here). This
is now implemented as `NseQuote::stock_chart_data_raw(symbol, period)`.

Additional findings while building it:

- **`days` only accepts five values.** `1D`, `1W`, `1M`, `1Y` and `5Y` all
  return real data - the other period buttons NSE's own chart shows
  (`3M`, `6M`, `3Y`, `ALL`) return a 500 with
  `"status":"NULL_POINTER"` (a raw Java `ResultSet.next()` NPE leaking
  through), not a clean error or empty result. Modeled as an enum
  (`ChartPeriod`) rather than a free-form string so a caller can't hit
  this.
- **Only `1D` populates `change`/`percent_change`.** Every other window
  returns `null` for both on every point (e.g. `[1789689600000,996.2,
  "NM",null,null]` for `days=1W`) - only daily closes, no intraday
  change. `session` (`"PO"`/`"NM"`) stays populated in every window,
  always `"NM"` outside `1D`.
- **This is per-symbol only - index charts don't work through this
  endpoint.** Tried `symbol=NIFTY 50`, `symbol=NIFTY 50N`, and the old
  endpoint's `indices=true` flag; all three return
  `{"error":"Unexpected end of JSON input"}` (NSE's backend itself failed
  to produce valid JSON). `NseQuote::stock_chart_data_raw` is scoped to
  stocks only until an index variant is found.
- **An unknown symbol is a clean 404**, unlike the old endpoint's silent
  empty placeholder - surfaces as `Error::NotFound`, matching
  `stock_quote_raw`'s existing convention.
- **Another `CH_TIMESTAMP`-shaped bug: each point's epoch is built from
  IST digits, not a real UTC instant.** Confirmed by comparing a
  freshly-fetched point's timestamp against `stock_quote_raw`'s
  `last_update_time` (genuine IST) at the same moment: decoding the epoch
  as UTC read exactly ~5:30 ahead of the real UTC clock - the wall-clock
  *digits* were right, just labeled as the wrong timezone. Same root
  cause as `mTIMESTAMP`/`CH_TIMESTAMP` in `dates.rs`, different endpoint.
  `ChartDataPoint::timestamp` reads the UTC digits back out as the IST
  value they actually represent (`ist_timestamp_from_millis` in
  `quote.rs`) rather than trusting the instant.
- **`tick_data` was never a separate endpoint** - Python's own
  `tick_data` is just `return self.chart_data(symbol, indices)`, a plain
  alias. Not modeled separately here.
- **`top_stocks`/`getTopTenStock` looked resolvable the same way, but
  turned out to be a dead end - see the next section.** It reaches NSE
  fine and returns a 200, but only its `topGainers` field ever came back
  populated; the other 7 fields (`topLoosers`,
  `mostActiveValue`/`mostActiveVolume`, `volumeSpurtsValue`,
  `etfWatchValue`, `fiftyTwoWeekHigh`/`fiftyTwoWeekLow`) were empty across
  three separate live checks a minute+ apart, market genuinely open.

## `top_stocks` implemented via `live-analysis-variations`, not `getTopTenStock`

Python's `top_stocks()` calls `getTopTenStock` via NextApi (see above),
but live testing showed that endpoint only reliably returns
`topGainers` - every other field it claims to have was empty on every
check, market open, not a timing fluke. Checking NSE's own live "Top
Gainers/Losers" page's network requests turned up the endpoint it
actually uses instead:

```
GET /api/live-analysis-variations?index=gainers
GET /api/live-analysis-variations?index=loosers
```

(`loosers` is NSE's own spelling on the wire, not a typo introduced
here). Confirmed live: at the exact moment `getTopTenStock.topLoosers`
was an empty array, this endpoint returned real losers data (e.g.
`BHARTIARTL -2.97%`). This crate now implements `top_stocks` around this
endpoint instead, as `NseLiveMarket::market_movers_raw` -
`getTopTenStock` isn't used anywhere.

Shape and quirks, confirmed live:

- **One request returns all seven "buckets" for one direction, not one
  bucket at a time.** The response has no `data` key - instead, seven
  top-level keys keyed exactly as `legends` names them: `NIFTY`,
  `BANKNIFTY`, `NIFTYNEXT50`, `SecGtr20` ("Securities > Rs 20"),
  `SecLwr20` ("Securities < Rs 20"), `FOSec` ("F&O Securities"), `allSec`
  ("All Securities") - each `{"data": [...], "timestamp": "..."}`. Two
  requests (one per direction) are needed to get both gainers and
  losers; `market_movers_raw` does both and flattens all 14
  bucket/direction combinations into one `Vec` tagged by `scope`/
  `direction`, the same convention used for block deals' two sessions
  and the eq-turnover leaderboards.
- **`net_price` and `perChange` are NOT the same field**, despite
  matching in most rows - confirmed by scanning full responses: e.g.
  NIFTYNEXT50's `BAJAJHLDNG` had `net_price: 1.54` (absolute price
  change) vs `perChange: 0.96` (percent change) in the same row. Unlike
  `LiveFoRow`'s genuinely-redundant turnover fields, both are kept here.
  `perChange` is also the one field NSE spells in camelCase - every
  other field on this row is `snake_case`.
- **`ca_ex_dt`/`ca_purpose` use `"-"` as their "no corporate action"
  placeholder**, not `null` or an empty string (confirmed live: 13 of
  127 rows in one gainers sample) - modeled as `Option<String>` via a
  small `deserialize_dash_as_none` helper, the same "normalize a
  placeholder to `None` at the boundary" approach used for
  `market-turnover`'s `-` and other endpoints' empty strings elsewhere in
  this crate.
- **An invalid `index` value doesn't 404 or error - it changes the whole
  response shape.** `?index=notreal` returns HTTP 200 with
  `{"data":"Missing index or key."}` (a bare string, not the seven-bucket
  object). Since `index` is only ever one of two values this crate
  controls internally (not user input), this doesn't need handling - it
  just fails to deserialize into the expected shape, same rationale as
  `ChartPeriod` staying an enum.
- **NIFTY's own losers bucket had 17 rows, not 20** at the moment this
  was checked (NIFTY was up overall that day, so fewer than 20 of its 50
  constituents were down) - the "top 20" cap is a maximum, not a
  guarantee, confirmed live rather than assumed.

## `corporate_announcements`: seven segments, two schemas, and several dead/redundant fields

`GET /api/corporate-announcements?index={segment}&from_date=DD-MM-YYYY&to_date=DD-MM-YYYY&symbol=...`
(`symbol`/date-range params optional). NSE's real corporate-filings page
(`/companies-listing/corporate-filings-announcements`) uses seven `index`
values, found by inspecting its network requests: `equities`, `sme`,
`debt`, `mf`, `invitsreits`, `municipalBond`, `sse`. All seven are real,
supported segments - all modeled, across two types.

Checked all seven live, 9,771+ real rows across a 3-week equities pull
plus samples of the other six (253 real `sse` rows over a 9-month
window):

- **Six share one schema; `sse` has a different one - but it's real
  data, not empty.** `equities`/`sme`/`debt`/`mf`/`invitsreits`/
  `municipalBond` all return the identical field set, modeled as
  `CorporateAnnouncementRow`. `sse` (Social Stock Exchange, NSE's segment
  for registered social enterprises/NPOs) returns a completely different
  one through the same endpoint - `an_attach`/`an_desc`/`ann_Date`/
  `ann_date`/`ann_tstamp`/`bm_Date`/`comp_name`/... instead of
  `attchmntFile`/`desc`/`an_dt`/`sm_name`/... - but it's genuinely
  populated (real social enterprises like "Sewa International", some
  with their own `-SE`-suffixed symbols like "EF-SE"), not an edge case
  to exclude. Modeled separately as `SseAnnouncementRow` via
  `sse_announcements_raw` - same reasoning as giving genuinely different
  schemas their own type elsewhere in this crate, rather than excluding
  them outright (unlike `insurance`/`reitsinvits` on
  `corporates-financial-results`, which were excluded because they
  lacked fields the modeled shape actually needs, not just because the
  shape differed).
  <br><br>
  **Correction:** an earlier version of this entry claimed `sse` returns
  the same `{"data":[],"msg":"no data found"}` envelope as an invalid
  segment name. That was never actually verified for `sse` specifically -
  it was wrongly inferred by association with `reitsinvits`/`insurance`,
  which turned out to be *wrong guesses at segment names* (the real one
  is `invitsreits`, word order swapped), not evidence about `sse`. `sse`
  with a real date range returns a normal, fully-populated JSON array
  like every other valid segment.
- **`symbol`/`isin` are only both populated for `equities`** (within
  `CorporateAnnouncementRow`'s six segments). Confirmed
  across every non-equities segment sampled: `debt`/`municipalBond`
  (bonds) leave both `null`; `sme`/`mf`/`invitsreits` have a `symbol`
  but leave `isin` `null`. Both modeled as `Option<String>`.
- **`bflag`/`csvName`/`old_new`/`orgid` are always `null`** - checked
  across all 9,771+ equities rows and every other segment sampled, zero
  non-null values anywhere. Dropped entirely.
- **`attFileSize` is byte-for-byte identical to `fileSize`** in every
  row checked (0 mismatches/9,771) - dropped the duplicate, kept
  `fileSize`.
- **`exchdisstime`/`difference` are redundant, not real data.**
  `exchdisstime` never differs from `an_dt` by more than ~5 seconds
  across 9,771 rows (pure exchange-processing latency), and `difference`
  is 100% derivable from the two timestamps (`exchdisstime - an_dt`,
  verified exactly on every row) - both dropped, keeping only `an_dt` as
  `announcement_time`.
- **`an_dt`/`dt`/`sort_date` are three encodings of the same instant -
  but `sort_date` is unreliable.** `sort_date` (already
  `"YYYY-MM-DD HH:MM:SS"`, easiest to parse) is `null` for every `debt`/
  `municipalBond` row sampled, while `an_dt`/`dt` are never null on any
  segment. Parsing `an_dt` instead, despite needing a month-name format
  (`"%d-%b-%Y %H:%M:%S"`).
- **Date-format casing is inconsistent by segment**: `debt`/
  `municipalBond` send `"21-SEP-2026"` (uppercase month); every other
  segment sends `"21-Sep-2026"`. Confirmed chrono's `%b` parses both
  identically, so no special-casing was needed once `an_dt` was chosen
  over `sort_date`.
- **`smIndustry` uses two different "no value" placeholders depending on
  segment**: `null` for most segments, but the literal string `"-"` for
  `mf` - confirmed live. Normalized to `None` for both via a small
  `deserialize_dash_or_null_as_none` helper.
- **An unrecognized `index` value doesn't error - it changes the
  response envelope.** A valid segment with results returns a plain
  JSON array; a genuinely invalid segment name returns HTTP 200 with
  `{"data":[],"msg":"no data found"}` instead - confirmed with
  `reitsinvits`/`insurance`/`equitiesnonperiodic`, three wrong guesses
  made while looking for the real segment list (none of the three are
  valid segments on this endpoint at all; the real name is `invitsreits`,
  word order swapped from the first guess). Both cases are treated as
  "no rows" (a `#[serde(untagged)]` enum handles either shape) rather
  than distinguished, since there's no reliable way to tell "invalid
  segment" apart from "valid segment, genuinely zero matches" - both
  return the same HTTP 200.
- **An explicit date range returns everything in that window, not
  capped at a small "recent" count** - omitting both dates (untested
  here, matches Python's own default) returns a short recent-activity
  list, but `from_date=to_date=today` for `equities` alone returned 446
  rows. This crate always requires an explicit range (like every other
  history endpoint) rather than replicating Python's "both dates or
  neither" runtime check - passing `from_date == to_date` gets the same
  "just today" result with no invalid state to guard against.
- **Every attachment URL seen (both `CorporateAnnouncementRow` and
  `SseAnnouncementRow`) is a real PDF** - confirmed live by downloading
  and checking one (`file` reports "PDF document, version 1.5, 9
  page(s)"). `download_attachment_raw`/`_save` download it as-is rather
  than assuming the format, same as `NseCorporateResults`'s
  `download_xbrl_raw`/`download_result_html_raw` - this is the first
  data source in this crate whose downloaded files are actually PDFs
  (checked: `download-xbrl` is XML, `download-result-html` is HTML,
  `daily-report`'s files are mostly CSV/zip/`.DAT`, though at least one
  real file - the commodity segment's deposit-percentage report - is
  also a `.pdf`, confirmed live; `download_report_save` already saves it
  correctly since it's a generic byte-passthrough like this one).
- **`download_bytes`/`filename_from_url` promoted to `live.rs`, shared
  with `corporate_results.rs`** - both modules needed the exact same
  "GET a URL, save under NSE's own filename" logic
  (`download_xbrl_raw`/`download_result_html_raw` there,
  `download_attachment_raw` here), so the second real consumer triggered
  promoting it out of `corporate_results.rs`'s private methods, matching
  the same "extract once a second consumer appears" convention already
  used for `write_csv`/`NEXTAPI_URL`/`deserialize_lenient_u64`.

## `top_stocks`'s remaining categories: most-active equities, volume gainers, 52-week high/low, large deals

`market_movers_raw` only ever covered gainers/losers. The other four
categories from the original `top_stocks`/`getTopTenStock` list turned
out to live on NSE's site as four more "Live Analysis" pages, each with
its own dedicated endpoint - found the same way `live-analysis-variations`
was found for gainers/losers: opening the real page and reading its
network requests.

- **Most Active Equities**: `GET /api/live-analysis-most-active-securities?index=value|volume`
  (top-20 by traded value, top-20 by traded volume - two requests,
  flattened into one `Vec` tagged by `ranking`, same convention as
  `eq_derivative_turnover_raw`). `closePrice` is confirmed live to
  always be `0` (checked both rankings) - a dead placeholder, dropped.
  `exDate` uses `"-"` as its no-corporate-action placeholder (same
  convention as `MarketMoverRow::ca_ex_date`); `purpose` is plain
  `null`/string.
- **Volume Gainers**: `GET /api/live-analysis-volume-gainers` - stocks
  trading well above their 1-week/2-week average volume. Clean shape,
  no nulls or type quirks found in the sample checked.
- **52-Week High/Low**: two entirely separate endpoints, not one
  endpoint with a direction parameter - `GET
  /api/live-analysis-data-52weekhighstock` and `.../...52weeklowstock`.
  Flattened into one `Vec` tagged by `direction`. Two real bugs caught
  by testing live (not by inspecting a single captured sample):
  - `comapnyName` - NSE's own typo (missing an "n") - confirmed to be
    spelled this way on **every** row, both directions; not a one-off
    typo in a single record.
  - `prevClose` is sent as a JSON string on this endpoint, unlike every
    numeric sibling field on the same row (plain JSON numbers).
  - **`prevHLDate` can be the literal string `"-"` instead of a real
    date**, for a recently-listed stock with no genuine previous
    52-week extreme yet (confirmed live: 3 of 125 high rows, 3 of 44
    low rows on one check, always the same handful of newly-listed
    symbols, always paired with `prev52WHL: 0`). This was modeled as a
    required `NaiveDate` at first and broke live within the same
    session - the unit tests built from one hand-picked sample record
    didn't catch it because that sample happened to have a real date.
    Fixed by making the field `Option<NaiveDate>` and adding a second
    test built from the actual failing live response. Lesson: a single
    captured sample proves a shape parses, not that every row will -
    still worth running the real live integration test before calling
    a feature done, even after the unit tests pass.
  - The top-level `high`/`low` counts the raw response also carries are
    dropped - confirmed live, both are just `data.len()`.
- **Large Deals**: `GET /api/snapshot-capital-market-largedeal` - a
  single response carrying three parallel lists (`BULK_DEALS_DATA`,
  `SHORT_DEALS_DATA`, `BLOCK_DEALS_DATA`), all with the identical row
  shape, flattened into one `Vec` tagged by `deal_type`. A genuinely
  different, simpler view of deals than `NseArchives::bulk_deals_raw`/
  `NseLiveMarket::block_deal_session_raw` (client identity and buy/sell
  side instead of live OHLC-style pricing) - and the only source in this
  crate for short deals at all. Confirmed live: `buySell`/`clientName`/
  `remarks`/`watp` are **always** `null` for short deals specifically
  (133 of 133 rows checked) - short-sale counterparty/side/price isn't
  disclosed through this endpoint, unlike bulk/block deals, where only
  `remarks` is sometimes null. `qty`/`watp` are numeric-looking JSON
  strings, not plain numbers - parsed with small dedicated
  deserializers. The top-level `BULK_DEALS`/`SHORT_DEALS`/`BLOCK_DEALS`
  counts are dropped for the same reason as 52-week high/low's `high`/
  `low` - just `data.len()`.
