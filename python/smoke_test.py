"""Offline end-to-end check of an installed sauda wheel: spawns the bundled
server, round-trips an RPC that fails validation before it would reach NSE,
and checks the protobuf -> dict conversion. Needs no network:

    python smoke_test.py
"""

import grpc

from sauda import Client, _to_py
from sauda._proto import jugaad_pb2 as pb

q = pb.StockQuote(symbol="X", total_traded_volume=2**40, delivery_pct=1.5)
q.order_book.levels.add(buy_price=1.0)
d = _to_py(q)
assert d["total_traded_volume"] == 2**40 and isinstance(d["total_traded_volume"], int)
assert d["delivery_pct"] == 1.5
assert d["delivery_quantity"] is None and d["total_market_cap"] is None
assert d["order_book"]["levels"][0]["buy_price"] == 1.0
assert _to_py(pb.StockQuote())["order_book"] is None

with Client() as c:
    try:
        c.stock_history("SBIN", "not-a-date", "2026-01-01")
    except grpc.RpcError as e:
        assert e.code() == grpc.StatusCode.INVALID_ARGUMENT, e
    else:
        raise AssertionError("expected INVALID_ARGUMENT")

print("ok")
