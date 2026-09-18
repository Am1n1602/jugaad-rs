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
