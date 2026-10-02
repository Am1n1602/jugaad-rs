# sauda

NSE market data for Python. A thin client over [`jugaad-rpc`](https://github.com/Am1n1602/jugaad-rs/tree/main/crates/jugaad-rpc), the Rust gRPC server from the [jugaad-rs](https://github.com/Am1n1602/jugaad-rs) monorepo - the wheel bundles the server binary, so there is nothing else to install or run.

```bash
pip install sauda
```

```python
from sauda import Client

with Client() as c:                      # starts the bundled server on a loopback port
    print(c.stock_quote("SBIN")["last_price"])
    rows = c.stock_history("SBIN", "2026-01-01", "2026-01-31")
    for q in c.watch_stock_quote("SBIN", interval=5):
        print(q["last_price"])
        break
```

| Method | Returns |
|---|---|
| `stock_quote(symbol)` | live quote with a 5-level order book |
| `watch_stock_quote(symbol, interval=3)` | iterator of live quotes |
| `stock_history(symbol, from_date, to_date)` | daily OHLC, volume and delivery rows |
| `index_history(name, from_date, to_date)` | daily index OHLC |
| `index_snapshot()` | every NSE index, live |
| `market_status()` | open/closed per market segment, holiday-aware |
| `large_deals()` | bulk, short and block deals |
| `option_chain(symbol, kind="index", expiry=None)` | option chain rows with call and put legs |
| `option_expiries(symbol)` | available expiry dates |
| `corporate_announcements(from_date, to_date, segment="equities", symbol=None)` | exchange disclosures |

Results are plain dicts and lists of dicts, so `pandas.DataFrame(rows)` works directly. The server stops when the client is closed, and also when your Python process dies (including a hard kill or a notebook kernel restart). `Client.stub` is the raw generated gRPC stub for anything not wrapped above.

**Full reference, error handling and troubleshooting: [the manual](https://github.com/Am1n1602/jugaad-rs/blob/main/python/MANUAL.md).**

Wheels: Windows x64, Linux x86_64 (glibc 2.28+), macOS Apple Silicon. Python 3.9+.

## Development

```bash
pip install -r requirements-gen.txt maturin
python generate.py           # generate src/sauda/_proto from the shared .proto
maturin develop --release    # build the server + install in the current venv
python smoke_test.py         # offline end-to-end check
```
