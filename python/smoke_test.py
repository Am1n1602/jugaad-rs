"""Offline end-to-end check of an installed sauda wheel: runs the bundled
`jugaad-rpc` command as a server, connects the Client to it, round-trips RPCs
that fail validation before they would reach NSE, and checks the config and the
protobuf -> dict conversion. Needs no network:

    python smoke_test.py
"""

import asyncio
import datetime
import os
import socket
import subprocess
import sysconfig
import tempfile
import time
from contextlib import contextmanager


# --- a server for the Client to connect to: the bundled command, run directly
def server_binary() -> str:
    exe = "jugaad-rpc" + (".exe" if os.name == "nt" else "")
    path = os.path.join(sysconfig.get_path("scripts"), exe)
    assert os.path.isfile(path), f"the wheel did not install {exe} to {path}"
    return path


@contextmanager
def running_server(addr):
    output = tempfile.TemporaryFile()
    proc = subprocess.Popen(
        [server_binary()],
        stdout=output,
        stderr=subprocess.STDOUT,
        env={**os.environ, "JUGAAD_RPC_ADDR": addr},
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    host, port = addr.rsplit(":", 1)
    deadline = time.monotonic() + 10
    while True:
        if proc.poll() is not None:
            # A negative code is the signal that killed it (-4 SIGILL, -11 SIGSEGV).
            output.seek(0)
            text = output.read().decode(errors="replace")
            raise AssertionError(
                f"the server exited during startup (code {proc.returncode}): {text!r}"
            )
        try:
            socket.create_connection((host, int(port)), timeout=0.2).close()
            break
        except OSError:
            assert time.monotonic() < deadline, "the server never started listening"
            time.sleep(0.05)
    try:
        yield proc
    finally:
        proc.kill()
        proc.wait()
        output.close()


def free_addr() -> str:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return f"127.0.0.1:{s.getsockname()[1]}"


# The bundled server must start before any gRPC code is loaded, which keeps
# "the binary is broken" apart from anything gRPC does to a process that
# spawns children: Python 3.9 forks, and gRPC's fork handlers then run.
with running_server(free_addr()):
    pass

# This harness spawns servers from a process that already runs gRPC threads, so
# every spawn runs gRPC's fork handlers. In CI a server once died at startup
# right after one logged "epoll_wait error: Bad file descriptor"; the children
# here exec immediately, so the handlers have nothing to protect.
os.environ.setdefault("GRPC_ENABLE_FORK_SUPPORT", "0")

import grpc  # noqa: E402
from grpc import aio  # noqa: E402

from sauda import DEFAULT_ADDR, Client, ConfigBuilder, _open_channel, _to_py  # noqa: E402
from sauda._proto import jugaad_pb2 as pb  # noqa: E402
from sauda._proto import jugaad_pb2_grpc  # noqa: E402

# --- protobuf -> dict conversion
q = pb.StockQuote(symbol="X", total_traded_volume=2**40, delivery_pct=1.5)
q.order_book.levels.add(buy_price=1.0)
d = _to_py(q)
assert d["total_traded_volume"] == 2**40 and isinstance(d["total_traded_volume"], int)
assert d["delivery_pct"] == 1.5
assert d["delivery_quantity"] is None and d["total_market_cap"] is None
assert d["order_book"]["levels"][0]["buy_price"] == 1.0
assert _to_py(pb.StockQuote())["order_book"] is None


# --- ConfigBuilder
@contextmanager
def env(**values):
    saved = {k: os.environ.get(k) for k in values}
    os.environ.update(values)
    try:
        yield
    finally:
        for k, v in saved.items():
            os.environ.pop(k, None) if v is None else os.environ.__setitem__(k, v)


def raises(exc, fn):
    try:
        fn()
    except exc:
        return
    raise AssertionError(f"expected {exc.__name__}")


assert DEFAULT_ADDR == "127.0.0.1:50051"
default = ConfigBuilder().build()
assert (default.addr, default.connect_timeout) == (None, 15.0)
cfg = ConfigBuilder().addr("127.0.0.1:1").connect_timeout(3).build()
assert (cfg.addr, cfg.connect_timeout) == ("127.0.0.1:1", 3)
raises(ValueError, lambda: ConfigBuilder().connect_timeout(0).build())
raises(ValueError, lambda: ConfigBuilder().addr(""))

with env(SAUDA_ADDR="127.0.0.1:2", SAUDA_CONNECT_TIMEOUT="2.5"):
    cfg = ConfigBuilder().from_env()
    assert (cfg.addr, cfg.connect_timeout) == ("127.0.0.1:2", 2.5)
    assert ConfigBuilder().addr("overridden").from_env().addr == "127.0.0.1:2"
with env(SAUDA_ADDR="", SAUDA_CONNECT_TIMEOUT=""):
    cfg = ConfigBuilder().addr("kept").from_env()
    assert (cfg.addr, cfg.connect_timeout) == ("kept", 15.0)
with env(SAUDA_CONNECT_TIMEOUT="soon"):
    raises(ValueError, lambda: ConfigBuilder().from_env())


async def expect_invalid(awaitable):
    try:
        await awaitable
    except grpc.RpcError as e:
        assert e.code() == grpc.StatusCode.INVALID_ARGUMENT, e
    else:
        raise AssertionError("expected INVALID_ARGUMENT")


async def expect(exc, awaitable):
    try:
        await awaitable
    except exc:
        return
    raise AssertionError(f"expected {exc.__name__}")


async def validation_checks(c):
    # Each of these is rejected on its date argument before reaching NSE.
    await expect_invalid(c.stock_history("SBIN", "not-a-date", "2026-01-01"))
    await expect_invalid(c.index_history("NIFTY 50", "not-a-date", "2026-01-01"))
    await expect_invalid(c.corporate_announcements("2026-01-01", "not-a-date"))
    await expect_invalid(c.option_chain("NIFTY", "index", expiry="not-a-date"))
    await expect(ValueError, c.option_chain("NIFTY", "bogus"))
    await expect_invalid(c.bhavcopy("not-a-date"))
    await expect_invalid(c.fo_bhavcopy("not-a-date"))
    contract = ("NIFTY", "2026-01-01", "2026-01-02", "2026-01-29")
    await expect_invalid(c.derivatives_history("NIFTY", "not-a-date", *contract[2:], "fut-idx"))
    # An option needs a strike and a side, and futures carry neither.
    await expect_invalid(c.derivatives_history(*contract, "opt-idx"))
    await expect_invalid(c.derivatives_history(*contract, "opt-idx", strike_price=100))
    await expect_invalid(c.derivatives_history(*contract, "fut-idx", strike_price=100))
    await expect(ValueError, c.derivatives_history(*contract, "swap"))
    await expect(ValueError, c.stock_chart("SBIN", "3m"))
    await expect(ValueError, c.index_chart("NIFTY 50", "3y"))


async def lifecycle():
    addr = free_addr()
    cfg = ConfigBuilder().addr(addr).connect_timeout(10).build()
    short = ConfigBuilder().addr(addr).connect_timeout(1).build()

    # With no server running, the client does not start one: connect times out.
    c = Client()
    await expect(RuntimeError, c.stock_quote("SBIN"))
    raises(RuntimeError, lambda: c.stub)
    await c.disconnect()  # a no-op when never connected
    try:
        await Client().connect(short)
    except TimeoutError as e:
        assert "jugaad-rpc" in str(e), e
    else:
        raise AssertionError("expected TimeoutError")

    with running_server(addr) as server:
        await c.connect(cfg)
        await expect(RuntimeError, c.connect())
        await validation_checks(c)
        results = await asyncio.gather(
            *(c.stock_history("SBIN", "not-a-date", "x") for _ in range(5)),
            return_exceptions=True,
        )
        assert all(
            isinstance(r, grpc.RpcError)
            and r.code() == grpc.StatusCode.INVALID_ARGUMENT
            for r in results
        ), results
        await c.disconnect()
        await c.disconnect()
        await expect(RuntimeError, c.stock_quote("SBIN"))
        assert server.poll() is None, "disconnect must leave the server running"

        await c.connect(cfg)  # reconnecting works, and so does a second client
        async with Client(cfg) as other:
            await validation_checks(other)
        await c.disconnect()

    # The server is gone, and nothing in the client brings it back.
    await expect(TimeoutError, Client().connect(short))


# A response over gRPC's default 4 MiB cap must still arrive: a local fake
# server returns ~5 MB; a default channel rejects it, sauda's channel does not.
class BigServer(jugaad_pb2_grpc.JugaadServicer):
    async def GetStockHistory(self, request, context):
        row = pb.StockHistoryRow(symbol="X" * 100)
        return pb.StockHistoryResponse(rows=[row] * 50_000)


async def big_response():
    fake = aio.server()
    jugaad_pb2_grpc.add_JugaadServicer_to_server(BigServer(), fake)
    port = fake.add_insecure_port("127.0.0.1:0")
    await fake.start()
    request = pb.StockHistoryRequest(
        symbol="X", from_date="2026-01-01", to_date="2026-01-02"
    )
    try:
        default_channel = aio.insecure_channel(f"127.0.0.1:{port}")
        try:
            await jugaad_pb2_grpc.JugaadStub(default_channel).GetStockHistory(request)
        except grpc.RpcError as e:
            assert e.code() == grpc.StatusCode.RESOURCE_EXHAUSTED, e
        else:
            raise AssertionError("expected the default 4 MiB cap to reject the response")
        finally:
            await default_channel.close()
        channel = _open_channel(f"127.0.0.1:{port}")
        try:
            response = await jugaad_pb2_grpc.JugaadStub(channel).GetStockHistory(request)
            assert len(response.rows) == 50_000
        finally:
            await channel.close()
    finally:
        await fake.stop(None)


# The wrappers for the bhavcopy, derivatives and chart RPCs, against a fake
# server that records what it was asked and returns canned answers.
CSV = "SYMBOL,SERIES,PRICE,\nABC,EQ,1.5,\nNESTLÉ ₹,EQ,2,\n"  # old format: trailing commas


def _is_utf8(piece: bytes) -> bool:
    try:
        piece.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


# Pieces of the file as the server would stream it. The size is chosen so that
# some piece cuts a multi-byte character in half, which is what decoding only
# after reassembly has to survive; the assert keeps this test honest.
PIECES = [CSV.encode()[i : i + 6] for i in range(0, len(CSV.encode()), 6)]
assert not all(_is_utf8(p) for p in PIECES), "no test piece splits a character"


class BatchServer(jugaad_pb2_grpc.JugaadServicer):
    def __init__(self):
        self.requests = {}

    async def GetBhavcopy(self, request, context):
        self.requests["bhavcopy"] = request
        return pb.BhavcopyResponse(csv=CSV)

    async def GetFoBhavcopy(self, request, context):
        if request.date == "2000-01-01":  # 6 MiB in all, over the default message cap
            for _ in range(6):
                yield pb.CsvChunk(data=b"x" * (1 << 20))
            return
        for piece in PIECES:
            yield pb.CsvChunk(data=piece)

    async def GetDerivativesHistory(self, request, context):
        self.requests["derivatives"] = request
        row = pb.DerivativesHistoryRow(symbol=request.symbol, underlying_value=1.5)
        return pb.DerivativesHistoryResponse(rows=[row])

    async def GetStockChart(self, request, context):
        self.requests["stock_chart"] = request
        points = [
            pb.StockChartPoint(timestamp="2026-01-01T09:15:00", price=1.0, change=0.5),
            pb.StockChartPoint(timestamp="2026-01-02T00:00:00", price=2.0),
        ]
        return pb.StockChart(name=request.symbol, points=points)

    async def GetIndexChart(self, request, context):
        self.requests["index_chart"] = request
        return pb.IndexChart(name=request.name, points=[pb.IndexChartPoint(price=1.0)])


async def batch_methods():
    servicer = BatchServer()
    fake = aio.server()
    jugaad_pb2_grpc.add_JugaadServicer_to_server(servicer, fake)
    port = fake.add_insecure_port("127.0.0.1:0")
    await fake.start()
    try:
        async with Client(ConfigBuilder().addr(f"127.0.0.1:{port}").build()) as c:
            want = [
                {"SYMBOL": "ABC", "SERIES": "EQ", "PRICE": "1.5"},
                {"SYMBOL": "NESTLÉ ₹", "SERIES": "EQ", "PRICE": "2"},
            ]
            # No empty-named column from the trailing commas; a date object works.
            assert await c.bhavcopy(datetime.date(2026, 10, 1)) == want
            assert servicer.requests["bhavcopy"].date == "2026-10-01"
            assert await c.fo_bhavcopy("2026-10-01") == want  # chunks reassembled, then decoded

            rows = await c.derivatives_history(
                "NIFTY",
                datetime.date(2026, 8, 1),
                "2026-10-01",
                datetime.date(2026, 10, 27),
                "OPT-IDX",
                strike_price=24000,
                option_type="pe",
            )
            req = servicer.requests["derivatives"]
            assert (req.symbol, req.from_date, req.to_date, req.expiry) == (
                "NIFTY", "2026-08-01", "2026-10-01", "2026-10-27"
            )
            assert req.instrument == pb.INSTRUMENT_OPT_IDX and req.strike_price == 24000
            assert req.option_type == pb.OPTION_TYPE_PUT
            assert rows[0]["symbol"] == "NIFTY" and rows[0]["underlying_value"] == 1.5
            assert rows[0]["strike_price"] == 0.0 and isinstance(rows[0]["volume"], int)
            await c.derivatives_history("RELIANCE", *contract_dates(), "fut-stk")
            req = servicer.requests["derivatives"]
            assert req.instrument == pb.INSTRUMENT_FUT_STK
            assert not req.HasField("strike_price") and req.option_type == pb.OPTION_TYPE_UNSPECIFIED

            chart = await c.stock_chart("SBIN", "5Y")
            assert servicer.requests["stock_chart"].period == pb.CHART_PERIOD_FIVE_YEARS
            assert chart["name"] == "SBIN" and len(chart["points"]) == 2
            assert chart["points"][0]["change"] == 0.5 and chart["points"][1]["change"] is None
            await c.stock_chart("SBIN")
            assert servicer.requests["stock_chart"].period == pb.CHART_PERIOD_ONE_DAY
            index = await c.index_chart("NIFTY 50", "6m")
            assert servicer.requests["index_chart"].period == pb.INDEX_CHART_PERIOD_SIX_MONTHS
            assert index["name"] == "NIFTY 50" and index["points"][0]["change"] == 0.0

            await validation_checks_client_side(c)

        # A stream bigger than the default 4 MiB message cap needs no raised
        # limits, because it arrives in chunks: this is what other languages see.
        default_channel = aio.insecure_channel(f"127.0.0.1:{port}")
        try:
            stub = jugaad_pb2_grpc.JugaadStub(default_channel)
            call = stub.GetFoBhavcopy(pb.BhavcopyRequest(date="2000-01-01"))
            total = sum([len(chunk.data) async for chunk in call])
        finally:
            await default_channel.close()
        assert total == 6 << 20, total
    finally:
        await fake.stop(None)


def contract_dates():
    return "2026-08-01", "2026-10-01", "2026-10-27"


async def validation_checks_client_side(c):
    await expect(ValueError, c.derivatives_history("N", *contract_dates(), "swap"))
    await expect(
        ValueError, c.derivatives_history("N", *contract_dates(), "opt-idx", 1, "straddle")
    )
    await expect(ValueError, c.stock_chart("SBIN", "3m"))
    await expect(ValueError, c.index_chart("NIFTY 50", "3y"))


async def second_loop():
    addr = free_addr()
    with running_server(addr):
        async with Client(ConfigBuilder().addr(addr).build()) as c:
            await validation_checks(c)


asyncio.run(lifecycle())
asyncio.run(big_response())
asyncio.run(batch_methods())
asyncio.run(second_loop())  # a fresh event loop must work too, as in repeated notebook runs

print("ok")
