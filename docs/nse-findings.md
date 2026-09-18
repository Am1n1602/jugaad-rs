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

There's no format migration here (one shape covers every date the
endpoint serves), but there is a hard cutoff on how far back it goes -
2018 returns 404, unlike the other bhavcopy variants which go back
years further. Python's comment about this endpoint timing out for
pre-2020 dates could not be reproduced; today it just 404s immediately.

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
