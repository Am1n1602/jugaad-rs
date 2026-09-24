# jugaad-rpc client examples

Manual smoke-test clients proving `jugaad-rpc` (see
[`../crates/jugaad-rpc`](../crates/jugaad-rpc)) is actually usable from
languages other than Rust, not just self-consistent within one. Both call
the same two RPCs - `GetStockQuote` (unary) and `WatchStockQuote`
(server-streaming) - against `crates/jugaad-rpc/proto/jugaad.proto`, the
single source of truth both clients build from.

Start the server first, from the repo root:

```bash
cargo run -p jugaad-rpc
```

## Python

Generates real stubs via `grpcio-tools` (not dynamic loading), matching
how most production Python gRPC clients are actually built.

```bash
cd clients/python
pip install -r requirements.txt
./generate.sh
python client.py SBIN
```

## Node.js

Loads the `.proto` file directly at runtime via `@grpc/proto-loader` - no
codegen step needed.

```bash
cd clients/node
npm install
node client.js SBIN
```
