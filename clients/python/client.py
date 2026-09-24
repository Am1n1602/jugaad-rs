#!/usr/bin/env python3
"""Manual smoke test for jugaad-rpc from Python.

Start the server first (`cargo run -p jugaad-rpc`), generate the stubs
once (`./generate.sh`), then:

    python client.py SBIN
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "generated"))

import grpc
import jugaad_pb2
import jugaad_pb2_grpc


def main() -> None:
    symbol = sys.argv[1] if len(sys.argv) > 1 else "SBIN"

    with grpc.insecure_channel("localhost:50051") as channel:
        stub = jugaad_pb2_grpc.JugaadStub(channel)

        quote = stub.GetStockQuote(jugaad_pb2.StockQuoteRequest(symbol=symbol))
        print("GetStockQuote:")
        print(quote)

        print("WatchStockQuote (3 updates, 2s apart):")
        stream = stub.WatchStockQuote(
            jugaad_pb2.WatchStockQuoteRequest(symbol=symbol, interval_seconds=2)
        )
        for i, update in enumerate(stream):
            print(f"  {update.symbol} last={update.last_price} change={update.change} at {update.last_update_time}")
            if i >= 2:
                break


if __name__ == "__main__":
    main()
