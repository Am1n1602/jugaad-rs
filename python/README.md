# sauda

NSE market data for Python. An async client for [`jugaad-rpc`](https://github.com/Am1n1602/jugaad-rs/tree/main/crates/jugaad-rpc), the Rust gRPC server from the [jugaad-rs](https://github.com/Am1n1602/jugaad-rs) monorepo. `sauda` only connects: you run the server (the wheel installs it as the `jugaad-rpc` command, or use the Docker image) and the client takes care of the connection.

```bash
pip install sauda
```

Start the server in its own terminal and leave it running:

```bash
JUGAAD_RPC_ADDR=127.0.0.1:50051 jugaad-rpc
```

Then connect from Python:

```python
import asyncio
from sauda import Client

async def main():
    client = Client()
    await client.connect()                          # connects to 127.0.0.1:50051

    print((await client.stock_quote("SBIN"))["last_price"])
    rows = await client.stock_history("SBIN", "2026-01-01", "2026-01-31")
    async for q in client.watch_stock_quote("SBIN", interval=5):
        print(q["last_price"])
        break

    await client.disconnect()                       # the server keeps running

asyncio.run(main())
```

You decide when to connect and disconnect (in a notebook, connect in one cell and use the client in the cells after it). For a short script, `async with Client() as c:` does both for you. For a server at another address:

```python
from sauda import Client, ConfigBuilder

config = ConfigBuilder().addr("127.0.0.1:50077").build()    # or ConfigBuilder().from_env()
client = Client()
await client.connect(config)
```

| Method (all `await`ed except the watch) | Returns |
|---|---|
| `stock_quote(symbol)` | live quote with a 5-level order book |
| `watch_stock_quote(symbol, interval=3)` | async iterator of live quotes |
| `stock_history(symbol, from_date, to_date)` | daily OHLC, volume and delivery rows |
| `index_history(name, from_date, to_date)` | daily index OHLC |
| `index_snapshot()` | every NSE index, live |
| `market_status()` | open/closed per market segment, holiday-aware |
| `large_deals()` | bulk, short and block deals |
| `option_chain(symbol, kind="index", expiry=None)` | option chain rows with call and put legs |
| `option_expiries(symbol)` | available expiry dates |
| `corporate_announcements(from_date, to_date, segment="equities", symbol=None)` | exchange disclosures |

Results are plain dicts and lists of dicts, so `pandas.DataFrame(rows)` works directly. `Client.stub` is the raw generated async gRPC stub for anything not wrapped above.

**Full reference, running the server (including Docker), configuration, error handling and troubleshooting: [the manual](https://github.com/Am1n1602/jugaad-rs/blob/main/python/MANUAL.md).** Upgrading from 0.1.x? 0.2.0 is async only and no longer starts a server for you, so see [the upgrade notes](https://github.com/Am1n1602/jugaad-rs/blob/main/python/MANUAL.md#upgrading-from-01x).

Wheels: Windows x64, Linux x86_64 (glibc 2.28+), macOS Apple Silicon. Other platforms build from the source distribution, which needs Rust 1.88+. Python 3.9+.

## Development

```bash
pip install -r requirements-gen.txt maturin
python generate.py           # generate src/sauda/_proto from the shared .proto
maturin develop --release    # build the server + install in the current venv
python smoke_test.py         # offline end-to-end check
```
