# NSE API findings

Undocumented NSE behavior discovered through live testing. NSE's endpoints
have no official documentation.

## Bhavcopy

- UDiff (Unified Distilled File Format) bhavcopy took effect on
  **2024-07-08**. Dates on or after it use a different URL and column set
  than earlier dates.
- Old-format equity bhavcopy: `GET https://www.nseindia.com/api/reports`
  (a different host than the UDiff path, which uses
  `nsearchives.nseindia.com`), with a fixed, non-date-dependent `archives`
  query parameter:
  `[{"name": "CM - Bhavcopy(csv)", "type": "daily-reports", "category": "capital-market", "section": "equities"}]`
  and a `date` parameter in `%d-%b-%Y` format (e.g. `"01-Jan-2020"`). Its
  CSV has an `ISIN` column the UDiff format doesn't.
- A weekend date and a post-cutover date requested through the old
  endpoint both return 404.
- The old-format endpoint occasionally fails once and succeeds on retry
  moments later with no request change - a real, observed flakiness in
  the endpoint itself.
- F&O bhavcopy switched to UDiff on the same date, 2024-07-08. Modern URL:
  `https://nsearchives.nseindia.com/content/fo/BhavCopy_NSE_FO_0_0_0_{yyyymmdd}_F_0000.csv.zip`.
  Old URL:
  `https://nsearchives.nseindia.com/content/historical/DERIVATIVES/{yyyy}/{MMM}/fo{dd}{MMM}{yyyy}bhav.csv.zip`
  (uppercase month, e.g. `JAN`).
- "Full" bhavcopy is a separate report:
  `https://nsearchives.nseindia.com/products/content/sec_bhavdata_full_{ddmmyyyy}.csv`
  (no separators, numeric month not abbreviated). Plain CSV, no zip
  wrapper. Covers every series (government securities under `"GS"`,
  trade-for-trade under `"BE"`, etc.) and adds delivery quantity/
  percentage columns. Those delivery columns use a literal `"-"` as
  their missing-value sentinel, not an empty field or JSON `null`.
- Bulk deals has no date parameter at all:
  `https://nsearchives.nseindia.com/content/equities/bulk.csv` always
  serves the current snapshot. No cookies needed, plain CSV. Returns 404
  for dates before 2018.

## niftyindices.com (index history)

Served from a separate host, niftyindices.com, with its own request
formats:

- **History-style endpoints** (`getHistoricaldatatabletoString` for
  OHLC, `getpepbHistoricaldataDBtoString` for P/E, `getTotalReturnIndexString`
  for TRI) take a body like `{"cinfo": "..."}` where `"..."` is a *string*
  containing a single-quoted Python-dict-literal, e.g.
  `"{'name': 'NIFTY 50', 'startDate': '01-Aug-2024', 'endDate': '05-Aug-2024', 'indexName': 'NIFTY 50'}"`.
  Sending proper nested JSON is rejected (redirects to an error page).
- `index_subtype_list` (`gethistoricaltypeSubindexdata`) takes proper
  nested JSON: `{"cinfo": {"indextype": ..., "indexgroup": ...}}`.
- `index_name_list` (`gethistoricaltypeindexdata`) takes a
  form-urlencoded body with PHP-style array field names:
  `cinfo[indextype]=...&cinfo[indexgroup]=...`.
- The three history-style endpoints each use a different date-field key
  name: `"HistoricalDate"` for OHLC, `"DATE"` for P/E, `"Date"` for TRI.
- Numbers are sent as JSON strings, e.g. `"OPEN":"24302.85"`.
- Date format: `"05 Aug 2024"` (space-separated) - different from stock
  history's `"05-Aug-2024"`.
- For the TRI endpoint's "strategy" indices, `name` is a short internal
  code while `indexName` is the display name; every other index uses the
  same value for both.
- An unknown index name and a trading-day-free date range both return
  `[]` with HTTP 200.
- `gethistoricaltypedata1` (index type list) needs no real body, but a
  genuinely zero-byte body (no `Content-Length` header sent at all) gets
  `411 Length Required`. A 1-byte body succeeds. The endpoint doesn't
  care about body content, only that `Content-Length` is present.
- Whole-market index bhavcopy: `https://www.niftyindices.com/Daily_Snapshot/ind_close_all_{dd}{MMM}{yyyy}.csv`
  (uppercase month, e.g. `ind_close_all_18SEP2026.csv`) returns HTTP
  **200** with an HTML error page as the body, not a real 404. The
  current, working format uses a zero-padded numeric month:
  `https://niftyindices.com/Daily_Snapshot/ind_close_all_{ddmmyyyy}.csv`
  (e.g. `ind_close_all_18092026.csv`, no `www.` needed). The reliable
  "is there data" signal is the `Content-Type` header:
  `application/octet-stream` for a real file, `text/html; charset=utf-8`
  for the same-status "not found" page.

## Stock/derivatives history (nseindia.com)

- `GET /api/historicalOR/generateSecurityWiseHistoricalData` requires
  cookies from a prior visit to a normal HTML page on nseindia.com (e.g.
  `GET /report-detail/eq_security`). Without them, the request is
  blocked.
- Two date fields for the same row: `mTIMESTAMP` (plain trading date,
  e.g. `"05-Aug-2024"`) and `CH_TIMESTAMP` (a UTC instant, e.g.
  `"2024-08-04T18:30:00.000Z"`) - always exactly 18:30 UTC (midnight
  IST) on the day *before* `mTIMESTAMP`'s date.
- `CH_TOTAL_TRADES`, `COP_DELIV_QTY`, `COP_DELIV_PERC` can be JSON
  `null` (seen on records from 2010, before NSE tracked total trade
  counts).
- A weekend-only date range and a nonexistent symbol both return
  `{"data": []}` with HTTP 200.
- The default equity series requires `series=ALL`, not `series=EQ`
  (passing `EQ` gives incomplete results) - and `ALL` returns every
  series active for that symbol/date, not just the default one. Example:
  SBIN on 2024-08-19 has two rows for the same date, one `CH_SERIES:
  "EQ"` (volume ~10.1M) and one `CH_SERIES: "T0"` (volume 1, a same-day-
  settlement trial NSE ran on some symbols in 2024).
- `GET /api/historicalOR/foCPV` (F&O price/open-interest history) lives
  on the same host, needs the same cookie, and has the same two-
  date-fields pattern: `FH_TIMESTAMP` ("05-Dec-2024") is the plain date,
  `FH_TIMESTAMP_ORDER` ("2024-12-04T18:30:00.000Z") is the same
  UTC-instant-on-the-previous-day encoding. Numbers here are real JSON
  numbers, not strings.
  - Futures and options share one response shape; futures rows have
    `FH_STRIKE_PRICE: 0` and `FH_OPTION_TYPE: "XX"` as sentinel values.
  - `FH_CHANGE_IN_OI` can be negative.
  - `FH_UNDERLYING_VALUE` is `null` for index instruments (NIFTY
    futures/options) but populated for stock instruments (confirmed
    with RELIANCE futures).
  - The expiry date must be sent uppercased (`"26-DEC-2024"`).
  - An expiry date that doesn't exist for a symbol returns
    `{"data": []}` with HTTP 200.
- Multi-month range chunking: NSE's `historicalOR` endpoints and
  niftyindices' history endpoint both return each **single chunk's**
  rows in **descending** date order internally (newest first), not
  ascending - applies to stock, derivatives, index, index-P/E, and
  index-TRI history alike.

## Generic daily reports

- `GET https://www.nseindia.com/api/daily-reports?key={segment}` lists
  every report NSE currently has for a segment - 39 report types
  confirmed for `"CM"`. Each entry gives `fileKey`, `filePath`, and
  `fileActlName`; concatenating `filePath` + `fileActlName` gives a
  working download URL. `filePath` includes an odd double slash after
  the domain, e.g. `"https://nsearchives.nseindia.com//content/equities/"`.
- No cookie warm-up needed.
- The response only ever has `CurrentDay` and `PreviousDay` entries
  (plus an always-empty `FutureDay`) - no older data is reachable this
  way.
- The same `fileKey` can appear in both `CurrentDay` and `PreviousDay`
  with different `tradingDate`s.
- An unknown `segment` value returns a different JSON shape entirely:
  `{"data":[],"msg":"no data found"}` instead of the normal
  `{"PreviousDay":[...],"CurrentDay":[...],...}` shape.
- Downloaded files vary in format: CSV, zip, proprietary `.DAT` files,
  and at least one `.pdf` (the commodity segment's deposit-percentage
  report).

## Per-symbol live endpoints (moved URLs)

A set of endpoints previously documented at different URLs have moved:

- Equity quote/trade info:
  `GET /api/NextApi/apiClient/GetQuoteApi?functionName=getSymbolData&marketType=N&series=EQ&symbol=SBIN`,
  returning `{"equityResponse": [...]}`.
- F&O quote for a symbol: same NextApi URL, with
  `functionName=getSymbolDerivativesData&symbol=...`.
- Single index live value: `GET /api/equity-stock-indices?index=NIFTY 50`.
- Option chains: `GET /api/option-chain-v3?type=Indices&symbol=NIFTY&expiry=...`,
  with the expiry normally looked up first via
  `GET /api/option-chain-contract-info?symbol=NIFTY`. Currency option
  chains: `GET /api/option-chain-currency`.
- Chart/tick data: see the chart-data section below.
- Market-wide derivative turnover: `GET /api/equity-stock?index=allcontracts`.
- Block deals: NextApi `functionName=getBlockDealSession`.
- Top gainers/losers: see the top_stocks section below (not
  `getTopTenStock`).

`NextApi/apiClient/GetQuoteApi` and `equity-stock-indices` return normal
200s with real data given a cookie warm-up plus `Referer` header.

Four endpoints need no cookie warm-up and no bot workaround at all:
- `api/marketStatus` - open/closed per segment
- `api/allIndices` - live snapshot of every index
- `api/market-turnover` - market-wide volume/value/OI by segment
- `api/liveEquity-derivatives?index=nse50_fut` - live NIFTY F&O snapshot

## `marketStatus`

- Each entry in the `marketState` array has a different shape depending
  on the segment: the four named segments (Capital Market, Currency,
  Commodity, Debt) use `"variation"` for the change value; a fifth entry
  describing a USD-adjusted NIFTY figure uses `"change"` instead and has
  no `market`/`marketStatus`/`tradeDate` key at all.
- `"last"` (and `"variation"`/`"change"`/`"percentChange"`) can be a
  real JSON number, a numeric JSON string (e.g. `"95.9600"` for a
  `currencyfuture` pseudo-segment), or an empty string meaning "not
  applicable" while that segment is closed.
- Market open (10:22 IST, 2026-09-21): Currency/Commodity/Debt segments
  still send empty `last`/`variation` even while `marketStatus: "Open"`.
  Commodity and Debt have no alternate source anywhere in the response.
  Currency does have a separate entry in `marketState`,
  `market: "currencyfuture"`, carrying a real `last` value (e.g.
  `"95.8225"`, USDINR future).

## `liveEquity-derivatives`

- Only `index=nse50_fut` works; every other value tried returns HTTP
  500.
- `value`, `totalTurnover`, and `premiumTurnOver` are always identical
  for every row.
- Returns exactly 3 rows, all `FUTIDX`, both with the market closed and
  with it open.

## `NextApi` inconsistent numeric typing

- `getSymbolDerivativesData`'s `openInterest`/`changeinOpenInterest`:
  most contracts send plain JSON integers, but some send the identical
  value as a JSON float (e.g. `90670.0` instead of `90670`) within the
  same response.
- `option-chain-v3` legs with no real contract for a strike/side still
  send a `CE`/`PE` object (not an omitted key), with `identifier: null`
  and every numeric field `0` (confirmed on deep SBIN equity strikes).
- On the NIFTY index option chain specifically, at least one strike's
  `changeinOpenInterest` came back as a JSON float
  (`48608.769230769234`) rather than an integer - only observed on this
  endpoint/field combination within the full ~128-row response, not in
  smaller samples.

## Block deals (live session)

`NextApi` `functionName=getBlockDealSession` returns:

```json
{"data": {"session1": [...], "session2": [...]}}
```

`session1` is the pre-open negotiated-deal window, `session2` is the
mid-day window. At one check, `session1` was empty and `session2` had
two deals. Each deal repeats a `PChange`/`pChange` duplicate-field pair.
`status`/`exDate`/`purpose` were `null` in every deal observed.

## `corporates-financial-results`

`GET /api/corporates-financial-results?index={segment}` takes segments
`equities`, `sme`, `reitsinvits`, `insurance`, `debt`. The response
**shape** differs by segment, not just field values:

- `equities`/`sme` share one shape: `symbol`/`companyName`/`isin`/
  `consolidated`/`audited`/`fromDate`/`toDate`/`filingDate`/`xbrl`/etc.
- `insurance` has no `isin`, no `fromDate`/`toDate` (has `periodEnd`
  instead, a single date), no `filingDate` - adds `insuranceType`,
  `naAttach`, `ixbrl`.
- `reitsinvits` is different again, and its `xbrl` filenames contain the
  string `INTEGRATED_FILING` - it serves data from SEBI's newer
  Integrated Filing framework through this older endpoint URL, with
  field names `auditedUnaudited`/`consNoncons`/`submissionDate`/
  `typeOfSubmission`.
- `debt` returned zero rows in every query tried, including an
  unfiltered bulk pull that found real data immediately for every other
  segment.
- The `issuer` parameter doesn't filter anything: `issuer=TCS` returns
  every company's filings (30,543 rows across 2,381 distinct symbols for
  one query), HTTP 200. The correct filter parameter is `symbol`.
- `audited`'s "not audited" value is spelled `"Un-Audited"` for
  `equities` and `"Unaudited"` (no hyphen) for `sme`.
  `consolidated`'s values (`"Consolidated"`/`"Non-Consolidated"`) are
  spelled consistently across segments.
- `cumulative` (`"Cumulative"`/`"Non-cumulative"` for `equities`,
  `"Cumulative"`/`"Non-Cumulative"` for `sme`) is 100% redundant with
  `period` across ~49,000 records checked (`Annual` always paired with
  `Cumulative`, `Quarterly` always with the non-cumulative spelling).
- `xbrl` is always present as a string, but for filings from before real
  XBRL existed, it's a placeholder URL ending in `/-`
  (`https://nsearchives.nseindia.com/corporate/xbrl/-`). For TCS, real
  XBRL starts at FY2018-19 annual (filed Apr-2019); every earlier annual
  filing back to FY2012-13 has the placeholder.
- `resultDetailedDataLink` is populated for 7,872 of 9,604
  placeholder-XBRL annual records and never for a real-XBRL record.
  `resultDescription` was `null` in every one of ~49,000 records
  checked.

## `eq_derivative_turnover`

`GET /api/equity-stock?index=allcontracts`:

- Returns two parallel top-20 lists under top-level keys `value`
  (ranked by premium turnover) and `volume` (ranked by contracts
  traded) - not the usual `data` envelope - plus `val_timestamp`/
  `vol_timestamp`. The same contract can appear in both lists.
- No cookie warm-up needed. `index=allcontracts` is the only confirmed
  value.
- `optionType` here uses `"Call"`/`"Put"`/`"-"`, a third vocabulary
  compared to `"CE"`/`"PE"`/`"XX"` elsewhere.
- `numberOfContractsTraded` and `openInterest` show the same
  int-vs-float inconsistency documented above.
- `totalTurnover` and `premiumTurnover` are genuinely different values
  here (unlike other endpoints' redundant turnover fields).

## Market-hours retest (2026-09-21, market genuinely open)

- `chart-databyindex` (`GET /api/chart-databyindex`) returns the
  identical empty placeholder
  (`{"closePrice":0,"grapthData":[],"identifier":null,"name":null}`)
  for both an equity (`SBIN`) and an index (`NIFTY 50`, `indices=true`)
  regardless of market hours - confirmed dead via both curl and
  Python's `jugaad-data`.
- Stock quote order book depth does populate live: SBIN's top-of-book
  during the open market showed `buyPrice1: 993.8, buyQuantity1: 1176,
  sellPrice1: 994, sellQuantity1: 11` - real resting orders, not the
  all-zero placeholder seen with the market closed.
- `market-turnover`'s `today` object stays empty (`Equities.today: {}`,
  `Total.today` every field `null`) even with the market open.
- `live_fo_snapshot` still returns exactly 3 rows, all `FUTIDX`, with
  the market open.
- `index_snapshot`'s values move intraday: NIFTY 50's `last` changed
  from 23346.4 (closed-market baseline) to 23375.85 (live check).

## Chart/tick data

Real endpoint:
`GET /api/NextApi/apiClient/GetQuoteApi?functionName=getSymbolChartData&symbol={symbol}{series}N&days={period}`
(e.g. `symbol=SBINEQN&days=1D`), returning
`{"identifier":"SBINEQN","name":"SBIN","grapthData":[[1789981259000,996,"PO","-0.2","-0.02"],...],"closePrice":996.2}`
(`grapthData` is NSE's own misspelling of "graphData").

- `days` only accepts `1D`, `1W`, `1M`, `1Y`, `5Y`. The other period
  buttons NSE's own chart shows (`3M`, `6M`, `3Y`, `ALL`) return HTTP
  500 with `"status":"NULL_POINTER"` (a raw Java `ResultSet.next()`
  NPE).
- Only `1D` populates `change`/`percent_change` on each point; every
  other window returns `null` for both on every point. `session`
  (`"PO"`/`"NM"`) is always populated, always `"NM"` outside `1D`.
- Per-symbol only - an index name (`NIFTY 50`, `NIFTY 50N`, or the old
  endpoint's `indices=true` flag) returns
  `{"error":"Unexpected end of JSON input"}`.
- An unknown symbol returns a clean 404.
- Each point's epoch timestamp is built from IST wall-clock digits but
  labeled as a UTC instant - decoding it as UTC reads ~5:30 ahead of the
  real UTC clock (confirmed by comparing against a live quote's
  `last_update_time`, genuine IST, at the same moment).
- `tick_data` is a plain alias for `chart_data` in Python, not a
  separate endpoint.
- `getTopTenStock` (NextApi) reaches NSE fine and returns 200, but only
  its `topGainers` field ever comes back populated - the other 7 fields
  (`topLoosers`, `mostActiveValue`/`mostActiveVolume`,
  `volumeSpurtsValue`, `etfWatchValue`, `fiftyTwoWeekHigh`/
  `fiftyTwoWeekLow`) were empty across three separate checks a minute+
  apart with the market open.

## Index price charts

Real endpoint (a different, one-segment-shorter path than the stock
chart endpoint above, `/api/NextApi/apiClient` vs
`/api/NextApi/apiClient/GetQuoteApi`):
`GET /api/NextApi/apiClient?functionName=getGraphChart&type=NIFTY%2050&flag=1D`,
response wrapped one level deeper:
`{"data": {"identifier":..., "grapthData":[...], ...}}`.

- Valid periods: `1D`/`1W`/`1M`/`3M`/`6M`/`1Y`/`5Y` (7 values - `3M`/
  `6M` work here but return HTTP 500 through the stock chart endpoint).
  `3Y`/`ALL` return a clean HTTP 404 (`{"data":null,"error":{}}`).
- `change`/`percent_change` are always real JSON numbers, never a
  string or `null` - every non-1D window sends a literal `0`/`0`
  instead of the stock endpoint's `null`/`null`.
- Timestamps have the same IST-digits-labeled-as-UTC bug as stock
  charts (confirmed by comparing against `equity-stock-indices`'s
  `lastUpdateTime` at the same moment).
- An unknown index name returns a clean HTTP 404 with the same
  `{"data":null,"error":{}}` body as an invalid period.

## `top_stocks` (gainers/losers)

Real endpoints:
```
GET /api/live-analysis-variations?index=gainers
GET /api/live-analysis-variations?index=loosers
```
(`loosers` is NSE's own spelling.)

- One request returns all seven "buckets" for one direction - no `data`
  key at the top level; instead seven top-level keys: `NIFTY`,
  `BANKNIFTY`, `NIFTYNEXT50`, `SecGtr20`, `SecLwr20`, `FOSec`, `allSec`,
  each `{"data": [...], "timestamp": "..."}`.
- `net_price` (absolute price change) and `perChange` (percent change)
  are genuinely different fields, despite matching in most rows -
  confirmed e.g. NIFTYNEXT50's `BAJAJHLDNG` with `net_price: 1.54` vs
  `perChange: 0.96` in the same row. `perChange` is the one camelCase
  field on an otherwise snake_case row.
- `ca_ex_dt`/`ca_purpose` use `"-"` as their "no corporate action"
  placeholder, not `null` or an empty string.
- An invalid `index` value returns HTTP 200 with
  `{"data":"Missing index or key."}` - a bare string, not the
  seven-bucket object.
- NIFTY's losers bucket had 17 rows, not 20, at one check - "top 20" is
  a maximum, not a guaranteed count.

## `top_stocks` (remaining categories)

- Most Active Equities: `GET /api/live-analysis-most-active-securities?index=value|volume`
  (top-20 by traded value, top-20 by traded volume). `closePrice` is
  always `0` in both rankings. `exDate` uses `"-"` as its
  no-corporate-action placeholder; `purpose` is plain `null`/string.
- Volume Gainers: `GET /api/live-analysis-volume-gainers` - stocks
  trading well above their 1-week/2-week average volume.
- 52-Week High/Low: two separate endpoints, not one with a direction
  parameter - `GET /api/live-analysis-data-52weekhighstock` and
  `.../...52weeklowstock`.
  - `comapnyName` - NSE's own typo (missing an "n") - on every row,
    both directions.
  - `prevClose` is sent as a JSON string, unlike numeric sibling fields
    on the same row.
  - `prevHLDate` can be the literal string `"-"` instead of a real date,
    for a recently-listed stock with no genuine previous 52-week
    extreme yet (3 of 125 high rows, 3 of 44 low rows on one check,
    always paired with `prev52WHL: 0`).
  - The top-level `high`/`low` counts are just `data.len()`.
- Large Deals: `GET /api/snapshot-capital-market-largedeal` - three
  parallel lists (`BULK_DEALS_DATA`, `SHORT_DEALS_DATA`,
  `BLOCK_DEALS_DATA`), identical row shape. `buySell`/`clientName`/
  `remarks`/`watp` are always `null` for short deals specifically (133
  of 133 rows checked) - not disclosed for short deals, unlike
  bulk/block deals where only `remarks` is sometimes null. `qty`/`watp`
  are numeric-looking JSON strings. Top-level `BULK_DEALS`/
  `SHORT_DEALS`/`BLOCK_DEALS` counts are just `data.len()`.

## `corporate_announcements`

`GET /api/corporate-announcements?index={segment}&from_date=DD-MM-YYYY&to_date=DD-MM-YYYY&symbol=...`
(`symbol`/date-range optional). Seven real `index` values:
`equities`, `sme`, `debt`, `mf`, `invitsreits`, `municipalBond`, `sse`.

- Six segments (`equities`/`sme`/`debt`/`mf`/`invitsreits`/
  `municipalBond`) share one schema. `sse` (Social Stock Exchange, for
  registered social enterprises/NPOs) returns a completely different
  one through the same endpoint: `an_attach`/`an_desc`/`ann_Date`/
  `ann_date`/`ann_tstamp`/`bm_Date`/`comp_name`/... instead of
  `attchmntFile`/`desc`/`an_dt`/`sm_name`/... - real, populated data
  (e.g. "Sewa International", some with `-SE`-suffixed symbols like
  "EF-SE").
- `symbol`/`isin` are only both populated for `equities`. `debt`/
  `municipalBond` (bonds) leave both `null`; `sme`/`mf`/`invitsreits`
  have a `symbol` but leave `isin` `null`.
- `bflag`/`csvName`/`old_new`/`orgid` are always `null` (checked across
  9,771+ equities rows and every other segment sampled).
- `attFileSize` is byte-for-byte identical to `fileSize` in every row
  checked.
- `exchdisstime` never differs from `an_dt` by more than ~5 seconds;
  `difference` is exactly `exchdisstime - an_dt` on every row.
- `an_dt`/`dt`/`sort_date` are three encodings of the same instant, but
  `sort_date` is `null` for every `debt`/`municipalBond` row sampled.
- `debt`/`municipalBond` send an uppercase month (`"21-SEP-2026"`);
  every other segment sends title-case (`"21-Sep-2026"`).
- `smIndustry` is `null` for most segments, but the literal string
  `"-"` for `mf`.
- An unrecognized `index` value returns HTTP 200 with
  `{"data":[],"msg":"no data found"}` instead of the normal plain JSON
  array a valid segment returns.
- An explicit date range returns everything in that window, not capped
  at a small "recent" count - `from_date=to_date=today` for `equities`
  alone returned 446 rows.
- Every attachment URL seen (both schemas) is a real PDF.

## SEBI Integrated Filing

`GET /api/integrated-filing-results?type=...&symbol=...&index=...&period_ended=...&from_date=DD-MM-YYYY&to_date=DD-MM-YYYY&page=N&size=N`.

- Two real `type` values share one response shape: `"Integrated
  Filing- Financials"` and `"Integrated Filing- Governance"`.
- `symbol` and `index` (segment, e.g. `sme`) both genuinely filter.
- `issuer` does not filter anything - a garbage value returns the exact
  same `totalCount` as no filter at all.
- `period_ended` (e.g. a specific quarter-end date, or `"all"`)
  genuinely filters.
- No date range is required server-side. `size=1000` works with no
  smaller cap found. The unfiltered total is 26,000+ rows and grows
  live throughout the day.
- `pdf_attach` can be a genuine JSON `null`, a dead sentinel literally
  ending in `/null` (no real PDF exists - the majority case, 674 of
  1000 rows checked), or a real working URL (138/1000).
- `attFileSize` is the real PDF's size (up to tens of MB) when
  `pdf_attach` is real.
- `cmName`/`smName` differ in casing on about 1% of rows (e.g. `"Ghcl
  Textiles Limited"` vs `"GHCL Textiles Limited"`) - not exact
  duplicates.
- `xbrlFileSize`/`ixbrlFileSize` are `null` on a real fraction of
  Governance-type rows (14 of 36 checked over one date window).
- `audited`/`consolidated` are `null` for the Governance type, but
  always populated strings (`"Audited"`/`"Un-Audited"`,
  `"Standalone"`/`"Consolidated"`) for the Financials type.

## `pre_open_market`

`GET /api/market-data-pre-open?key=NIFTY` only has real content during
NSE's pre-open session, roughly 9:00-9:15 IST each trading day. Outside
that window: `{"data":[],"msg":"No Data Found"}`. Row shape unconfirmed.

## Holiday list

`GET /api/holiday-master?type=trading` returns a JSON object keyed by
market segment (`CM`, `FO`, `CD`, `COM`, `CBM`, `CMOT`, `EGR`, `IRD`,
`MF`, `NDM`, `NTRP`, `SLBS` - 12 segments), each holding its own holiday
list. `morning_session`/`evening_session` are `null` for most
segments/dates but real values (`"Open"`/`"Closed"`) for `COM`/`EGR`.

## Smaller per-symbol/reference endpoints

- `index_list` (NextApi `getIndexList`): a bare JSON array of index name
  strings. Empty array for an unknown symbol.
- `symbol_meta` (NextApi `getMetaData`): every `is*`/`casFlag` field
  (FNO eligibility, ETF/debt/SLB/delisted/suspended flags) is sent as
  the literal JSON string `"true"`/`"false"`, not a real JSON boolean.
  An unknown symbol doesn't 404 - it returns HTTP 200 with every field
  `null`, including `symbol` itself.
- `symbol_name` (NextApi `getSymbolName`): same no-404 pattern as
  `symbol_meta`, but the empty case is a bare `{}`.
- `reg_details` (NextApi `getRegDetails`): `regAction`/`series`/
  `regNote` were `null` for both symbols checked (SBIN, TCS). Empty
  array for an unknown symbol.
- `yearwise_data` (NextApi `getYearwiseData`): despite the name, it's
  not one row per calendar year - a single-element list holding one
  symbol's percent change over several trailing windows (yesterday
  through 5 years) alongside its benchmark index's change over the same
  windows. `one_week_date`/`index_one_week_date` use a two-digit year
  (`"16-SEP-26"`) - the only date field observed with this format.
  Empty array for an unknown symbol.
