# Changelog

Two release tracks, versioned independently (see [Releasing](CONTRIBUTING.md#releasing)):

- **Rust workspace** - `jugaad-core`, the `jugaad` CLI, `jugaad-rpc` and its Docker image. Tagged `vX.Y.Z`.
- **`sauda`** - the Python package on PyPI. Tagged `py-vX.Y.Z`.

Add an entry under **Unreleased** with every user-visible change; it moves under a version heading when that version is tagged.

## Unreleased

### Added

- Nine more `jugaad-rpc` RPCs, 19 in all: `GetBhavcopy`, `GetFoBhavcopy`, `GetDerivativesHistory`, `GetStockChart`, `GetIndexChart`, `GetMarketMovers`, `GetMostActiveEquities`, `GetFiftyTwoWeek` and `GetHolidayList`. The two bhavcopy RPCs return NSE's own CSV text; `GetFoBhavcopy` is server-streaming, in 1 MiB chunks, because the file (5-6 MB) is larger than gRPC's default 4 MiB message cap. `GetStockChart` and `GetIndexChart` always return their points newest first, whichever way NSE sent them (the `jugaad` command-line tool keeps NSE's own order). These reach the Docker image with the next `v*` release.

### Changed

- The workspace version is now `0.2.2`. It stayed at `0.2.0` through the `v0.2.1` and `v0.2.2` tags, so binaries from those two releases report `jugaad 0.2.0` from `--version`.

## Rust workspace

### 0.2.2 - 2026-09-28

#### Added

- `jugaad-rpc` now exposes 10 RPCs. New: `GetStockHistory`, `GetIndexHistory`, `GetIndexSnapshot`, `GetLargeDeals`, `GetMarketStatus`, `GetOptionChain`, `GetOptionExpiries`, `GetCorporateAnnouncements`.

#### Changed

- Every NSE client has a 10 second connect timeout and a 30 second request timeout, and retries transient failures (connection errors, timeouts, HTTP 408/429/5xx) up to 3 times with exponential backoff. A 403 (`Error::Blocked`) is never retried.
- `jugaad_core::Error` has a new `HttpRetry` variant for network failures that survived the retries. `Error` is not `#[non_exhaustive]`, so code that matches it exhaustively needs a new arm.

#### Tests

- The 54 inline JSON samples moved into `crates/jugaad-core/tests/fixtures/`.
- New regression tests deserialize full real responses for the option chain, 52-week highs/lows and Integrated Filing endpoints, which are the three that previously hid bugs a small sample missed.

### 0.2.1 - 2026-09-25

#### Added

- `jugaad-rpc`, a gRPC server over `jugaad-core`, with `GetStockQuote` and a streaming `WatchStockQuote`.
- Docker image published to `ghcr.io/am1n1602/jugaad-rpc` on every `v*` tag.
- Python and Node.js example clients in `clients/`.

### 0.2.0 - 2026-09-24

First tagged release.

#### Added

- `jugaad-core` and the `jugaad` CLI (47 commands) covering bhavcopy (old and new formats, F&O, full), stock, index and derivatives history, index P/E, P/B and Total Return, generic daily reports, live market data, per-symbol quotes, charts and option chains, block and bulk deals, financial results with XBRL/HTML download, corporate announcements, Social Stock Exchange announcements and SEBI Integrated Filing entries.
- Optional `dataframe` feature that converts any row type into a `polars::DataFrame`.
- Prebuilt CLI binaries for Linux, macOS (Intel and Apple Silicon) and Windows on every release.

#### Fixed

- Multi-month history ranges no longer return dates in a sawtooth order.
- Live chart and session datapoint parsing, and option-chain parsing.

## `sauda`

The wheel bundles the `jugaad-rpc` server as built from the tagged commit, so server changes reach Python users only with a new `py-v*` release.

### 0.2.1 - 2026-10-04

#### Added

- Nine methods for the new server RPCs: `bhavcopy`, `fo_bhavcopy`, `derivatives_history`, `stock_chart`, `index_chart`, `market_movers`, `most_active_equities`, `fifty_two_week` and `holiday_list`.
- The bhavcopy methods return NSE's CSV columns as dicts of strings, in either file format (the one before 2024-07-08 and the one after). `fo_bhavcopy` receives a streamed file of tens of thousands of rows and reassembles it.
- `stock_chart` and `index_chart` return their points newest first for every period, like `derivatives_history`.
- These methods need the server bundled in this release or later; a server from 0.2.0 or the current Docker image answers `UNIMPLEMENTED`.

#### Changed

- The package metadata now names its author, Am1n1602 (Aman Gautam), which PyPI showed as None for earlier releases.

### 0.2.0 - 2026-10-03

Breaking: the client is async only and no longer starts a server. See the [upgrade notes](python/MANUAL.md#upgrading-from-01x); pin `sauda<0.2` to stay on 0.1.x.

#### Changed

- Every `Client` method is a coroutine, and `watch_stock_quote` is an async generator. There is no sync client.
- The client only connects to a server that is already running; starting and stopping one is up to you, with the `jugaad-rpc` command that the wheel installs or with the Docker image. Creating a `Client` no longer launches a process, and `disconnect()` leaves the server running. Connecting is explicit: `await client.connect(config)` and `await client.disconnect()`, or `async with`.
- The default address is now a fixed `127.0.0.1:50051` instead of a server on a random port.
- `close()` and `Client(startup_timeout=...)` are gone; use `disconnect()` and `ConfigBuilder().connect_timeout(...)`.
- Connecting is more than three times faster (0.5s to 0.14s) when the server has only just started: the channel retries its first connection quickly instead of backing off for a second.

#### Added

- `ConfigBuilder` and `Config`: `addr` and `connect_timeout`, plus `from_env()` reading `SAUDA_ADDR` and `SAUDA_CONNECT_TIMEOUT`.
- When no server is listening, `connect()` raises a `TimeoutError` that says how to start one.
- A source distribution (sdist) is now published alongside the wheels, as PyPI's packaging guide recommends. On a platform without a wheel (Intel Mac, Linux ARM, musl) pip builds the server from it, which needs a Rust toolchain.

#### Removed

- The bundled server no longer has the `JUGAAD_RPC_EXIT_ON_STDIN_CLOSE` option that 0.1.x used to stop the server it launched. Nothing launches it any more.

### 0.1.3 - 2026-10-03

#### Fixed

- Responses larger than gRPC's default 4 MiB receive limit no longer fail with `RESOURCE_EXHAUSTED`. `corporate_announcements` over the whole segment hit this for any range longer than about two weeks (a month is roughly 6 MB, three months 20 MB).

### 0.1.2 - 2026-10-02

#### Added

- Methods for the remaining RPCs: `index_history`, `index_snapshot`, `large_deals`, `market_status`, `option_chain`, `option_expiries`, `corporate_announcements`, and a `watch_stock_quote` generator.
- A full user manual, [`python/MANUAL.md`](python/MANUAL.md).

### 0.1.1 - 2026-10-02

First release on PyPI.

#### Fixed

- The README link to `jugaad-rpc` is absolute, so it works on the PyPI page.

### 0.1.0 - 2026-10-02

TestPyPI only; never on PyPI.

#### Added

- `Client`, which starts the bundled `jugaad-rpc` server on a loopback port and stops it on close, or when the Python process dies.
- `stock_quote` and `stock_history`, plus `Client.stub` for raw access to every RPC.
- Wheels for Windows x64, Linux x86_64 and macOS Apple Silicon.
