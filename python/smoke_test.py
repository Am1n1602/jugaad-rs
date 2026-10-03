"""Offline end-to-end check of an installed sauda wheel: spawns the bundled
server, round-trips an RPC that fails validation before it would reach NSE,
and checks the protobuf -> dict conversion. Needs no network:

    python smoke_test.py
"""

from concurrent import futures

import grpc

from sauda import Client, _open_channel, _to_py
from sauda._proto import jugaad_pb2 as pb
from sauda._proto import jugaad_pb2_grpc

q = pb.StockQuote(symbol="X", total_traded_volume=2**40, delivery_pct=1.5)
q.order_book.levels.add(buy_price=1.0)
d = _to_py(q)
assert d["total_traded_volume"] == 2**40 and isinstance(d["total_traded_volume"], int)
assert d["delivery_pct"] == 1.5
assert d["delivery_quantity"] is None and d["total_market_cap"] is None
assert d["order_book"]["levels"][0]["buy_price"] == 1.0
assert _to_py(pb.StockQuote())["order_book"] is None

def expect_invalid(call):
    try:
        call()
    except grpc.RpcError as e:
        assert e.code() == grpc.StatusCode.INVALID_ARGUMENT, e
    else:
        raise AssertionError("expected INVALID_ARGUMENT")


with Client() as c:
    # Each of these is rejected on its date argument before reaching NSE.
    expect_invalid(lambda: c.stock_history("SBIN", "not-a-date", "2026-01-01"))
    expect_invalid(lambda: c.index_history("NIFTY 50", "not-a-date", "2026-01-01"))
    expect_invalid(lambda: c.corporate_announcements("2026-01-01", "not-a-date"))
    expect_invalid(lambda: c.option_chain("NIFTY", "index", expiry="not-a-date"))
    try:
        c.option_chain("NIFTY", "bogus")
    except ValueError:
        pass
    else:
        raise AssertionError("expected ValueError for an unknown option chain kind")


# A response over gRPC's default 4 MiB cap must still arrive: a local fake
# server returns ~5 MB; a default channel rejects it, sauda's channel does not.
class BigServer(jugaad_pb2_grpc.JugaadServicer):
    def GetStockHistory(self, request, context):
        row = pb.StockHistoryRow(symbol="X" * 100)
        return pb.StockHistoryResponse(rows=[row] * 50_000)


fake = grpc.server(futures.ThreadPoolExecutor(max_workers=1))
jugaad_pb2_grpc.add_JugaadServicer_to_server(BigServer(), fake)
port = fake.add_insecure_port("127.0.0.1:0")
fake.start()
try:
    request = pb.StockHistoryRequest(
        symbol="X", from_date="2026-01-01", to_date="2026-01-02"
    )
    with grpc.insecure_channel(f"127.0.0.1:{port}") as default_channel:
        try:
            jugaad_pb2_grpc.JugaadStub(default_channel).GetStockHistory(request)
        except grpc.RpcError as e:
            assert e.code() == grpc.StatusCode.RESOURCE_EXHAUSTED, e
        else:
            raise AssertionError("expected the default 4 MiB cap to reject the response")
    with _open_channel(f"127.0.0.1:{port}") as channel:
        response = jugaad_pb2_grpc.JugaadStub(channel).GetStockHistory(request)
        assert len(response.rows) == 50_000
finally:
    fake.stop(None)

print("ok")
