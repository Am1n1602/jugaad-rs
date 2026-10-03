# sauda manual

`sauda` gives Python access to NSE market data: live quotes, history, option chains, index snapshots, large deals, market status and corporate announcements. It is an async client for a Rust server, `jugaad-rpc`, which does the actual NSE work. `sauda` only connects to that server: you run it, and the client assumes it is up. The wheel bundles the server as the `jugaad-rpc` command so you do not need Docker or Rust to run it.

- [Install](#install)
- [Quickstart](#quickstart)
- [Running the server](#running-the-server)
- [Connecting and configuration](#connecting-and-configuration)
- [How it works](#how-it-works)
- [Conventions](#conventions)
- [Reference](#reference)
- [Errors](#errors)
- [Troubleshooting](#troubleshooting)
- [Upgrading from 0.1.x](#upgrading-from-01x)

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

On any other platform (Intel Mac, Linux ARM, Alpine/musl) there is no wheel, because a wheel carries a prebuilt server binary. pip then falls back to the source distribution and builds the server from source. That needs a Rust toolchain, Rust 1.88 or newer (from [rustup.rs](https://rustup.rs)), and takes a few minutes (about three on a typical machine). Dependencies are `grpcio>=1.71.2` and `protobuf>=5.29`, with no upper bounds.

## Quickstart

First start the server, once, in its own terminal (more options under [Running the server](#running-the-server)):

```bash
JUGAAD_RPC_ADDR=127.0.0.1:50051 jugaad-rpc
```

Then connect:

```python
import asyncio
from sauda import Client

async def main():
    client = Client()
    await client.connect()                          # connects to 127.0.0.1:50051

    quote = await client.stock_quote("SBIN")
    print(quote["last_price"], quote["percent_change"])

    rows = await client.stock_history("SBIN", "2026-09-01", "2026-09-30")
    print(rows[0]["date"], rows[0]["close"])        # newest row first

    async for q in client.watch_stock_quote("SBIN", interval=5):
        print(q["last_price"])
        break

    await client.disconnect()                       # the server keeps running

asyncio.run(main())
```

You decide when to connect and when to disconnect. In a notebook, skip `asyncio.run` and `await` directly in cells: connect in one cell, use the client in the cells after it, and disconnect at the end. For a short script, `async with` does both for you and disconnects even if an exception is raised:

```python
async with Client() as c:
    quote = await c.stock_quote("SBIN")
```

Tabular results are lists of dicts, so they drop straight into pandas: `pandas.DataFrame(rows)`.

## Running the server

The server is a separate program that keeps running while your code uses it. Start it in a terminal, as a service, or in Docker, and stop it yourself when you are done.

**The `jugaad-rpc` command** (installed with the wheel). In a terminal, bash style:

```bash
JUGAAD_RPC_ADDR=127.0.0.1:50051 jugaad-rpc
```

or PowerShell:

```powershell
$env:JUGAAD_RPC_ADDR = "127.0.0.1:50051"; jugaad-rpc
```

**Docker** (no Python or Rust needed on the server side), publishing the port on loopback only:

```bash
docker run --rm -p 127.0.0.1:50051:50051 ghcr.io/am1n1602/jugaad-rpc:latest
```

Stop either with Ctrl+C (or `docker stop`). If you build from a checkout of the repository, `cargo run -p jugaad-rpc` runs the same server.

The server listens where `JUGAAD_RPC_ADDR` says, and **by default on all interfaces** (`0.0.0.0:50051`) with no authentication, which would expose it to your network. That is why the commands above set it to `127.0.0.1` (and Docker's `-p` to loopback). The connection is also unencrypted, so only connect to a server on your own machine or a trusted network.

## Connecting and configuration

`Client()` does nothing until you connect, and it never starts or stops a server. You decide when it connects and when it disconnects:

```python
from sauda import Client, ConfigBuilder

config = ConfigBuilder().addr("127.0.0.1:50051").build()    # or ConfigBuilder().from_env()

client = Client()
await client.connect(config)
# ... await client.stock_quote("SBIN") and so on ...
await client.disconnect()                                   # the server keeps running
```

For a short script, `async with Client(config) as c:` connects on entry and disconnects on exit, even if an exception is raised.

`ConfigBuilder` methods, all chainable except the last two, which return the finished `Config`:

| Method | Meaning |
|---|---|
| `addr(target)` | the server to connect to, e.g. `"127.0.0.1:50051"` (default) or `"localhost:50051"` |
| `connect_timeout(seconds)` | how long `connect()` waits for the server (default 15) |
| `build()` | returns the `Config` |
| `from_env()` | applies the environment variables below over anything already set, then builds |

| Environment variable | Sets |
|---|---|
| `SAUDA_ADDR` | `addr` |
| `SAUDA_CONNECT_TIMEOUT` | `connect_timeout`, in seconds |

Unset or empty variables are ignored. `Client(config)` remembers a config for `connect()` (and for `async with`); `connect(config)` overrides it, and with neither the defaults apply. `connect()` waits up to `connect_timeout` seconds for the server to accept connections, then raises `TimeoutError`. It can be called again after `disconnect()`, but not twice in a row.

## How it works

The client talks to the server over gRPC; it does not start, stop or manage the server in any way.

- **Event loop:** a connected client belongs to the event loop it connected in. Connect inside the loop you will use; to use another loop (for example a second `asyncio.run`), create a new `Client` there.
- **Concurrency:** calls can overlap. `await asyncio.gather(c.stock_quote("SBIN"), c.stock_quote("TCS"))` runs both at once. Several clients can share one server.
- **Response size:** the client accepts responses of any size. gRPC's default 4 MiB cap would reject, for example, a month of whole-segment corporate announcements (about 6 MB), so `sauda` lifts it on its own connection; only your memory bounds it. This applies from 0.1.3; earlier versions raise `RESOURCE_EXHAUSTED` on such calls.
- **Network behavior:** the server uses 10 second connect and 30 second request timeouts, and retries transient failures (connection errors, timeouts, HTTP 408/429/5xx) up to 3 times with backoff. NSE's own 403 "blocked" response is deliberately never retried.

## Conventions

- **Everything is a coroutine.** `await` every method except `watch_stock_quote`, which is an async generator you iterate with `async for`.
- **Dates** are `datetime.date` objects or `"YYYY-MM-DD"` strings. Dates in results are `"YYYY-MM-DD"` strings.
- **Rows** are plain dicts keyed by the field names listed below; collections are lists of them.
- **Optional fields** come back as `None` when NSE does not provide them (for example `delivery_pct`, or `pe` on an index without a P/E). A `None` is different from `0`.
- **Numbers:** prices are floats; quantities, volumes and counts are ints.
- **Empty results are not always errors.** An unknown symbol or index in `stock_history`, `index_history` or `option_expiries`, or an unknown `segment` in `corporate_announcements`, returns an empty list rather than raising. Check for `[]` when a typo is possible. `stock_quote`, `watch_stock_quote` and `option_chain` raise `NOT_FOUND` for an unknown symbol instead.

## Reference

### `await Client.stock_quote(symbol) -> dict`

Live quote for one stock, e.g. `await c.stock_quote("SBIN")`.

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

### `Client.watch_stock_quote(symbol, interval=3)` -> async iterator of dicts

Yields the same dict as `stock_quote` every `interval` whole seconds, starting immediately; an `interval` of 0 uses the server default of 3 seconds. A failure raises `grpc.RpcError` from the loop.

```python
async for q in c.watch_stock_quote("SBIN", interval=5):
    print(q["last_price"])
```

Leaving the loop with `break` leaves the stream open until the generator is closed or garbage-collected. To end it right away, close it explicitly:

```python
gen = c.watch_stock_quote("SBIN")
async for q in gen:
    if q["last_price"] > 1000:
        break
await gen.aclose()
```

If you call `disconnect()` while another task is still iterating a stream, the stream is cancelled and that task sees `asyncio.CancelledError`. Close your streams first, or let that task end as cancelled.

### `await Client.stock_history(symbol, from_date, to_date, series=None) -> list[dict]`

Daily rows, newest first. `series` defaults to `"ALL"`, which NSE needs for complete results (`"EQ"` alone is incomplete).

Fields: `symbol`, `series`, `date`, `open`, `high`, `low`, `previous_close`, `last_traded_price`, `close`, `vwap`, `volume`, `value`, `trades` (`None` possible), `delivery_quantity` (`None` possible), `delivery_pct` (`None` possible).

### `await Client.index_history(name, from_date, to_date) -> list[dict]`

Daily OHLC for an index such as `"NIFTY 50"` or `"NIFTY BANK"`, newest first. Fields: `index_name`, `date`, `open`, `high`, `low`, `close`.

### `await Client.index_snapshot() -> list[dict]`

Live snapshot of every NSE index in one call (well over a hundred rows), including NIFTY 50, NIFTY BANK and India VIX.

Fields: `category`, `name`, `symbol`, `last`, `change`, `percent_change`, `open`, `high`, `low`, `previous_close`, `year_high`, `year_low`, and the optional `pe`, `pb`, `div_yield`, `advances`, `declines`, `unchanged`.

### `await Client.market_status() -> list[dict]`

Open or closed status per market segment, aware of exchange holidays (unlike a clock-based check).

Fields: `market` (for example `"Capital Market"`, `"Currency"`, `"Commodity"`, `"Debt"`), `status` (for example `"Close"`), `trade_date`, `status_message`, and the optional strings `index`, `last`, `change`, `percent_change`.

### `await Client.large_deals() -> list[dict]`

Bulk, short and block deals for NSE's most recent trading day.

Fields: `deal_type` (`"bulk"`, `"short"` or `"block"`), `symbol`, `company_name`, `quantity`, `date`, and the optional `client_name`, `buy_sell`, `weighted_avg_price`, `remarks`.

### `await Client.option_chain(symbol, kind="index", expiry=None) -> list[dict]`

Option chain for an index or a stock, one row per strike.

- `kind` is `"index"` (the default, for example `"NIFTY"`) or `"equity"` (for example `"SBIN"`). It maps to a query parameter NSE accepts; NSE currently returns identical data for either value, so passing `"equity"` for stocks only matches NSE's own convention in case that changes. Anything other than those two raises `ValueError`.
- `expiry` defaults to the nearest expiry. Use `option_expiries` to list the others.
- An unknown symbol raises `NOT_FOUND`.

Each row: `strike_price`, `expiry`, `call`, `put`. `call` and `put` can each be `None` when NSE lists no contract for that strike and side, so check before indexing into them. When present, a leg has: `identifier`, `last_price`, `change`, `percent_change`, `open_interest`, `change_in_open_interest`, `percent_change_in_open_interest`, `total_traded_volume`, `implied_volatility`, `buy_price`, `buy_quantity`, `sell_price`, `sell_quantity`, `total_buy_quantity`, `total_sell_quantity`, `underlying_value`.

### `await Client.option_expiries(symbol) -> list[str]`

Every available expiry as `"YYYY-MM-DD"`, nearest first, from the symbol's option chain listing.

### `await Client.corporate_announcements(from_date, to_date, segment="equities", symbol=None) -> list[dict]`

Exchange disclosures (board meetings, credit ratings, press releases and similar). `segment` is one of `equities`, `sme`, `debt`, `mf`, `invitsreits` or `municipalBond`. Leave `symbol` out for the whole segment, which is large: about 1,200 rows per trading day, so roughly 17,000 rows for a month and 55,000 for three months (a few seconds to fetch).

Fields: `company_name`, `category`, `description`, `has_xbrl`, `announcement_time` (ISO 8601), `sequence_id`, and the optional `symbol`, `isin`, `industry`, `attachment_url`, `file_size`.

### Raw access

`Client.stub` is the generated async gRPC stub and exposes every RPC the server offers, returning raw protobuf messages; it raises `RuntimeError` until you connect. The request and response classes are in `sauda._proto.jugaad_pb2`; that path is internal and may change, so prefer the methods above where one exists. `Client.stub` already allows large responses; if you build your own gRPC channel to the server, set `grpc.max_receive_message_length` yourself (see [Errors](#errors)).

## Errors

| You see | Meaning |
|---|---|
| `grpc.RpcError` with `.code()` = `INVALID_ARGUMENT` | A date was not `YYYY-MM-DD`. `.details()` names the argument. |
| `grpc.RpcError` with `NOT_FOUND` | `stock_quote`, `watch_stock_quote` or `option_chain` could not find the symbol, or NSE had no data for it. |
| `grpc.RpcError` with `UNAVAILABLE` | NSE has blocked the session (its bot protection), or the server went away while a call was in flight. Wait a while before retrying, and check the server is still running. |
| `grpc.RpcError` with `RESOURCE_EXHAUSTED` | A response was larger than the receive limit ("Received message larger than max"). `sauda` 0.1.3 and later never hits this on its own connection; upgrade if you see it. On a gRPC channel you built yourself, set `grpc.max_receive_message_length` (to `-1` for unlimited). |
| `grpc.RpcError` with `INTERNAL` | Any other upstream failure (network error, unexpected NSE response). `.details()` has the message. |
| `RuntimeError` | A method was called before `connect()` or after `disconnect()`, or `connect()` was called twice in a row. |
| `TimeoutError` | No server became ready at the address within `connect_timeout`: nothing is listening there. The message suggests how to start one. |
| `ValueError` | `option_chain` was given a `kind` other than `"index"` or `"equity"`, or a config value was invalid (an empty `addr`, a timeout that is not positive, a non-numeric `SAUDA_CONNECT_TIMEOUT`). |
| `asyncio.CancelledError` | `disconnect()` was called while this task was still iterating a watch stream. |

```python
import grpc

try:
    await c.stock_quote("NOSUCHSYM")
except grpc.RpcError as e:
    if e.code() == grpc.StatusCode.NOT_FOUND:
        ...
```

## Troubleshooting

**`pip` takes minutes, or fails with a build error.** On a platform without a wheel (see [Install](#install)) pip builds the server from the source distribution with Cargo, which needs Rust 1.88 or newer. Install it from [rustup.rs](https://rustup.rs) and retry. If pip reports that no version matches instead, your Python is older than 3.9.

**`TimeoutError: no server became ready at ...`.** Nothing is listening at that address. Start the server (see [Running the server](#running-the-server)) and check that the address matches: the client's default is `127.0.0.1:50051`, so a server started on another address needs `ConfigBuilder().addr(...)` or `SAUDA_ADDR`. For Docker, the port must be published (`-p 127.0.0.1:50051:50051`).

**`RuntimeError: not connected`.** Call `await client.connect()` first, or use `async with Client() as c:`.

**`jugaad-rpc: command not found`.** The wheel installs the command into your environment's scripts directory. Activate the virtual environment you installed into, or find the directory with `python -c "import sysconfig; print(sysconfig.get_path('scripts'))"` and run the command from there. With `pip install --user` the scripts directory may not be on `PATH`.

**`UNAVAILABLE` errors.** NSE flags clients that make too many requests too quickly. Slow your polling (use a larger `interval`) and wait before retrying. The library does not retry these on purpose. If it happens right after the server was stopped or restarted, reconnect.

**Empty list instead of data.** See [Conventions](#conventions): check the symbol or index name, and the date range (non-trading days return nothing).

**Dependency conflicts with `protobuf` or `grpcio`.** `sauda` needs `protobuf>=5.29` and `grpcio>=1.71.2` and works with protobuf 5.x, 6.x and 7.x. If another package pins protobuf below 5.29, install `sauda` in a separate environment.

## Upgrading from 0.1.x

0.2.0 is a breaking change: the client is async only, and it no longer starts a server.

```python
# 0.1.x: the client started its own server
with Client() as c:
    quote = c.stock_quote("SBIN")

# 0.2.0: run the server yourself (see "Running the server"), then connect
async with Client() as c:
    quote = await c.stock_quote("SBIN")
```

- Every method is a coroutine, so `await` it; `watch_stock_quote` is an async generator (`async for`). There is no sync client; from synchronous code, wrap the work in `asyncio.run(...)`.
- `Client()` never starts a server. Run `jugaad-rpc` or the Docker image yourself and leave it running; any number of clients can share it.
- The client connects to a fixed default address, `127.0.0.1:50051`, instead of a server on a random port; change it with `ConfigBuilder().addr(...)`.
- `Client(startup_timeout=...)` becomes `ConfigBuilder().connect_timeout(...)`.
- `client.close()` becomes `await client.disconnect()`.
- Error types are unchanged: failures are still `grpc.RpcError` subclasses with the same status codes.

Pin `sauda<0.2` if you are not ready to move.
