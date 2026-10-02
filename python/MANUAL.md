# sauda manual

`sauda` gives Python access to NSE market data: live quotes, history, option chains, index snapshots, large deals, market status and corporate announcements. The wheel bundles a Rust server (`jugaad-rpc`) that does the actual NSE work; the Python side starts it for you and talks to it over gRPC on your own machine.

- [Install](#install)
- [Quickstart](#quickstart)
- [How it works](#how-it-works)
- [Conventions](#conventions)
- [Reference](#reference)
- [Errors](#errors)
- [Troubleshooting](#troubleshooting)

## Install

```bash
pip install sauda
```

Requires Python 3.9 or newer. Wheels are published for:

| Platform | Wheel |
|---|---|
| Windows x64 | `win_amd64` |
| Linux x86_64 (glibc 2.28+) | `manylinux_2_28_x86_64` |
| macOS, Apple Silicon | `macosx_11_0_arm64` |

On any other platform (Intel Mac, Linux ARM, Alpine/musl) pip reports "no matching distribution", because the wheel carries a prebuilt binary. Dependencies are `grpcio>=1.71.2` and `protobuf>=5.29`, with no upper bounds.

## Quickstart

```python
from sauda import Client

with Client() as c:
    quote = c.stock_quote("SBIN")
    print(quote["last_price"], quote["percent_change"])

    rows = c.stock_history("SBIN", "2026-09-01", "2026-09-30")
    print(rows[0]["date"], rows[0]["close"])        # newest row first

    for q in c.watch_stock_quote("SBIN", interval=5):
        print(q["last_price"])
        break                                       # leaving the loop stops the stream
```

Tabular results are lists of dicts, so they drop straight into pandas: `pandas.DataFrame(rows)`.

## How it works

Creating a `Client` starts the bundled `jugaad-rpc` server as a child process, listening on `127.0.0.1` on a free random port, and connects to it. Nothing is reachable from outside your machine.

- **Shutdown:** `close()`, leaving a `with` block, and normal interpreter exit all stop the server. It also stops when your Python process dies abruptly (a hard kill, or a notebook kernel restart), so it does not leave stray processes behind.
- **One server per `Client`.** Create one `Client` and reuse it rather than making one per request.
- **Startup:** `Client(startup_timeout=15.0)` waits up to that many seconds for the server to accept connections.
- **Network behavior:** the server uses 10 second connect and 30 second request timeouts, and retries transient failures (connection errors, timeouts, HTTP 408/429/5xx) up to 3 times with backoff. NSE's own 403 "blocked" response is deliberately never retried.

## Conventions

- **Dates** are `datetime.date` objects or `"YYYY-MM-DD"` strings. Dates in results are `"YYYY-MM-DD"` strings.
- **Rows** are plain dicts keyed by the field names listed below; collections are lists of them.
- **Optional fields** come back as `None` when NSE does not provide them (for example `delivery_pct`, or `pe` on an index without a P/E). A `None` is different from `0`.
- **Numbers:** prices are floats; quantities, volumes and counts are ints.
- **Empty results are not always errors.** An unknown symbol or index in `stock_history`, `index_history` or `option_expiries`, or an unknown `segment` in `corporate_announcements`, returns an empty list rather than raising. Check for `[]` when a typo is possible. `stock_quote`, `watch_stock_quote` and `option_chain` raise `NOT_FOUND` for an unknown symbol instead.

## Reference

### `Client.stock_quote(symbol) -> dict`

Live quote for one stock, e.g. `c.stock_quote("SBIN")`.

| Field | Notes |
|---|---|
| `symbol`, `company_name`, `series` | |
| `open`, `day_high`, `day_low`, `previous_close`, `last_price` | |
| `change`, `percent_change` | versus previous close |
| `year_high`, `year_low` | 52-week range |
| `total_traded_volume`, `total_traded_value` | |
| `total_market_cap` | `None` if unavailable |
| `face_value` | |
| `delivery_quantity`, `delivery_pct` | `None` if unavailable |
| `order_book` | `{"levels": [...], "total_buy_quantity", "total_sell_quantity"}`; `levels` always has 5 entries, best price first, each `{"buy_price", "buy_quantity", "sell_price", "sell_quantity"}` |
| `last_update_time` | string, as reported by NSE |

### `Client.watch_stock_quote(symbol, interval=3)` -> iterator of dicts

Yields the same dict as `stock_quote` every `interval` whole seconds, starting immediately. It runs until you leave the loop or call `.close()` on the generator, which also stops the server polling NSE for you. An `interval` of 0 uses the server default of 3 seconds. A failure raises `grpc.RpcError` from the loop.

### `Client.stock_history(symbol, from_date, to_date, series=None) -> list[dict]`

Daily rows, newest first. `series` defaults to `"ALL"`, which NSE needs for complete results (`"EQ"` alone is incomplete).

Fields: `symbol`, `series`, `date`, `open`, `high`, `low`, `previous_close`, `last_traded_price`, `close`, `vwap`, `volume`, `value`, `trades` (`None` possible), `delivery_quantity` (`None` possible), `delivery_pct` (`None` possible).

### `Client.index_history(name, from_date, to_date) -> list[dict]`

Daily OHLC for an index such as `"NIFTY 50"` or `"NIFTY BANK"`, newest first. Fields: `index_name`, `date`, `open`, `high`, `low`, `close`.

### `Client.index_snapshot() -> list[dict]`

Live snapshot of every NSE index in one call (well over a hundred rows), including NIFTY 50, NIFTY BANK and India VIX.

Fields: `category`, `name`, `symbol`, `last`, `change`, `percent_change`, `open`, `high`, `low`, `previous_close`, `year_high`, `year_low`, and the optional `pe`, `pb`, `div_yield`, `advances`, `declines`, `unchanged`.

### `Client.market_status() -> list[dict]`

Open or closed status per market segment, aware of exchange holidays (unlike a clock-based check).

Fields: `market` (for example `"Capital Market"`, `"Currency"`, `"Commodity"`, `"Debt"`), `status` (for example `"Close"`), `trade_date`, `status_message`, and the optional strings `index`, `last`, `change`, `percent_change`.

### `Client.large_deals() -> list[dict]`

Bulk, short and block deals for NSE's most recent trading day.

Fields: `deal_type` (`"bulk"`, `"short"` or `"block"`), `symbol`, `company_name`, `quantity`, `date`, and the optional `client_name`, `buy_sell`, `weighted_avg_price`, `remarks`.

### `Client.option_chain(symbol, kind, expiry=None) -> list[dict]`

Option chain for an index or a stock, one row per strike.

- `kind` is `"index"` (the default, for example `"NIFTY"`) or `"equity"` (for example `"SBIN"`). It maps to a query parameter NSE accepts; NSE currently returns identical data for either value, so passing `"equity"` for stocks only matches NSE's own convention in case that changes. Anything other than those two raises `ValueError`.
- `expiry` defaults to the nearest expiry. Use `option_expiries` to list the others.
- An unknown symbol raises `NOT_FOUND`.

Each row: `strike_price`, `expiry`, `call`, `put`. `call` and `put` can each be `None` when NSE lists no contract for that strike and side, so check before indexing into them. When present, a leg has: `identifier`, `last_price`, `change`, `percent_change`, `open_interest`, `change_in_open_interest`, `percent_change_in_open_interest`, `total_traded_volume`, `implied_volatility`, `buy_price`, `buy_quantity`, `sell_price`, `sell_quantity`, `total_buy_quantity`, `total_sell_quantity`, `underlying_value`.

### `Client.option_expiries(symbol) -> list[str]`

Every available expiry as `"YYYY-MM-DD"`, nearest first, from the symbol's option chain listing.

### `Client.corporate_announcements(from_date, to_date, segment="equities", symbol=None) -> list[dict]`

Exchange disclosures (board meetings, credit ratings, press releases and similar). `segment` is one of `equities`, `sme`, `debt`, `mf`, `invitsreits` or `municipalBond`. Leave `symbol` out for the whole segment, which can be thousands of rows for even a couple of days.

Fields: `company_name`, `category`, `description`, `has_xbrl`, `announcement_time` (ISO 8601), `sequence_id`, and the optional `symbol`, `isin`, `industry`, `attachment_url`, `file_size`.

### Raw access

`Client.stub` is the generated gRPC stub and exposes every RPC the server offers, returning raw protobuf messages. The request and response classes are in `sauda._proto.jugaad_pb2`; that path is internal and may change, so prefer the methods above where one exists.

## Errors

| You see | Meaning |
|---|---|
| `grpc.RpcError` with `.code()` = `INVALID_ARGUMENT` | A date was not `YYYY-MM-DD`. `.details()` names the argument. |
| `grpc.RpcError` with `NOT_FOUND` | `stock_quote`, `watch_stock_quote` or `option_chain` could not find the symbol, or NSE had no data for it. |
| `grpc.RpcError` with `UNAVAILABLE` | NSE has blocked the session (its bot protection). Wait a while before retrying. |
| `grpc.RpcError` with `INTERNAL` | Any other upstream failure (network error, unexpected NSE response). `.details()` has the message. |
| `ValueError` | `option_chain` was given a `kind` other than `"index"` or `"equity"`. |
| `FileNotFoundError` | The bundled server binary was not found (see Troubleshooting). |
| `TimeoutError` / `RuntimeError` | The server did not become ready in time, or exited while starting. |

```python
import grpc

try:
    c.stock_quote("NOSUCHSYM")
except grpc.RpcError as e:
    if e.code() == grpc.StatusCode.NOT_FOUND:
        ...
```

## Troubleshooting

**`pip` says no matching distribution.** Your platform or Python is outside the table under [Install](#install), or your Python is older than 3.9.

**`FileNotFoundError: bundled jugaad-rpc not found`.** The binary is installed into the scripts directory of your environment. This happens when the wheel was installed with `--target`, or into a `--user` location whose scripts directory is not on `PATH`. Reinstall into a virtual environment.

**`UNAVAILABLE` errors.** NSE flags clients that make too many requests too quickly. Slow your polling (use a larger `interval`) and wait before retrying. The library does not retry these on purpose.

**Empty list instead of data.** See [Conventions](#conventions): check the symbol or index name, and the date range (non-trading days return nothing).

**Dependency conflicts with `protobuf` or `grpcio`.** `sauda` needs `protobuf>=5.29` and `grpcio>=1.71.2` and works with protobuf 5.x, 6.x and 7.x. If another package pins protobuf below 5.29, install `sauda` in a separate environment.

**Stray `jugaad-rpc` process after a crash.** This should not happen, because the server exits when its parent process goes away. If you find one anyway, it is safe to kill.
