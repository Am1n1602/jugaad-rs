# jugaad-rpc client examples

Manual smoke-test clients proving `jugaad-rpc` (see
[`../crates/jugaad-rpc`](../crates/jugaad-rpc)) is actually usable from
languages other than Rust, not just self-consistent within one. Both call
the same two RPCs - `GetStockQuote` (unary) and `WatchStockQuote`
(server-streaming) - against `crates/jugaad-rpc/proto/jugaad.proto`, the
single source of truth both clients build from.

Start the server first. Quickest way - pull the published image, no Rust
toolchain and no local build needed:

```bash
docker pull ghcr.io/am1n1602/jugaad-rpc:latest
docker run --rm -p 50051:50051 ghcr.io/am1n1602/jugaad-rpc:latest
```

Or build it yourself from source (needed if you've changed
`jugaad-rpc`'s own code - the published image won't reflect local
edits):

```bash
docker build -f crates/jugaad-rpc/Dockerfile -t jugaad-rpc .
docker run --rm -p 50051:50051 jugaad-rpc
```

Or run it directly with Rust installed, no Docker at all:

```bash
cargo run -p jugaad-rpc
```

Either way you still need this repo cloned for the Python/Node steps
below - they read `crates/jugaad-rpc/proto/jugaad.proto` directly, since
that stays the single source of truth for both clients rather than a
separately-published copy.

## Large responses

gRPC clients reject responses over 4 MiB by default, but some RPCs can
return more than that: `GetCorporateAnnouncements` for a whole segment is
about 6 MB for a month and 20 MB for three months. The server sends them
fine; the failure is on the client, as `RESOURCE_EXHAUSTED: Received
message larger than max (... vs. 4194304)`. Raise the receive limit on
your channel (verified with both examples' libraries):

```python
channel = grpc.insecure_channel(
    "localhost:50051", options=[("grpc.max_receive_message_length", -1)]
)
```

```js
const client = new jugaad.Jugaad("localhost:50051", grpc.credentials.createInsecure(), {
  "grpc.max_receive_message_length": -1,
});
```

(The `sauda` Python package already does this.) The two small examples
below don't need it.

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
