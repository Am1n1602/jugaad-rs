# sauda

NSE market data for Python. A thin client over [`jugaad-rpc`](../crates/jugaad-rpc), the Rust gRPC server from the [jugaad-rs](https://github.com/Am1n1602/jugaad-rs) monorepo - the wheel bundles the server binary, so there is nothing else to install or run.

```python
from sauda import Client

with Client() as c:                      # starts the bundled server on a loopback port
    print(c.stock_quote("SBIN")["last_price"])
    rows = c.stock_history("SBIN", "2026-01-01", "2026-01-31")
```

The server stops when the client is closed, and also when your Python process dies (including a hard kill or a notebook kernel restart). `Client.stub` is the raw generated gRPC stub, which exposes every RPC the server offers.

## Development

```bash
pip install -r requirements-gen.txt maturin
python generate.py           # generate src/sauda/_proto from the shared .proto
maturin develop --release    # build the server + install in the current venv
python smoke_test.py         # offline end-to-end check
```
