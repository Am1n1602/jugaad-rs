#!/usr/bin/env node
// Manual smoke test for jugaad-rpc from Node.js. Loads the .proto file
// directly at runtime (no codegen step) - jugaad-rpc's own .proto stays
// the single source of truth.
//
// Start the server first (`cargo run -p jugaad-rpc`), then:
//
//     node client.js SBIN

const path = require("path");
const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");

const PROTO_PATH = path.join(
  __dirname,
  "..",
  "..",
  "crates",
  "jugaad-rpc",
  "proto",
  "jugaad.proto",
);

const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
});
const jugaad = grpc.loadPackageDefinition(packageDefinition).jugaad;

function main() {
  const symbol = process.argv[2] || "SBIN";
  const client = new jugaad.Jugaad(
    "localhost:50051",
    grpc.credentials.createInsecure(),
  );

  client.getStockQuote({ symbol }, (err, quote) => {
    if (err) throw err;
    console.log("GetStockQuote:");
    console.log(quote);

    console.log("\nWatchStockQuote (3 updates, 2s apart):");
    const stream = client.watchStockQuote({ symbol, interval_seconds: 2 });
    let count = 0;
    stream.on("data", (update) => {
      console.log(
        `  ${update.symbol} last=${update.lastPrice} change=${update.change} at ${update.lastUpdateTime}`,
      );
      count += 1;
      if (count >= 3) {
        stream.cancel();
        client.close();
      }
    });
    stream.on("error", (streamErr) => {
      // A cancel() call surfaces here too - that's expected, not a failure.
      if (streamErr.code !== grpc.status.CANCELLED) throw streamErr;
    });
  });
}

main();
