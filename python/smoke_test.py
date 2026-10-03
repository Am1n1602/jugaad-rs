"""Offline end-to-end check of an installed sauda wheel: runs the bundled
`jugaad-rpc` command as a server, connects the Client to it, round-trips RPCs
that fail validation before they would reach NSE, and checks the config and the
protobuf -> dict conversion. Needs no network:

    python smoke_test.py
"""

import asyncio
import os
import socket
import subprocess
import sysconfig
import time
from contextlib import contextmanager

import grpc
from grpc import aio

from sauda import DEFAULT_ADDR, Client, ConfigBuilder, _open_channel, _to_py
from sauda._proto import jugaad_pb2 as pb
from sauda._proto import jugaad_pb2_grpc

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


# --- a server for the Client to connect to: the bundled command, run directly
def server_binary() -> str:
    exe = "jugaad-rpc" + (".exe" if os.name == "nt" else "")
    path = os.path.join(sysconfig.get_path("scripts"), exe)
    assert os.path.isfile(path), f"the wheel did not install {exe} to {path}"
    return path


@contextmanager
def running_server(addr):
    proc = subprocess.Popen(
        [server_binary()],
        stdout=subprocess.DEVNULL,
        env={**os.environ, "JUGAAD_RPC_ADDR": addr},
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    host, port = addr.rsplit(":", 1)
    deadline = time.monotonic() + 10
    while True:
        assert proc.poll() is None, "the server exited during startup"
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


def free_addr() -> str:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return f"127.0.0.1:{s.getsockname()[1]}"


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


async def second_loop():
    addr = free_addr()
    with running_server(addr):
        async with Client(ConfigBuilder().addr(addr).build()) as c:
            await validation_checks(c)


asyncio.run(lifecycle())
asyncio.run(big_response())
asyncio.run(second_loop())  # a fresh event loop must work too, as in repeated notebook runs

print("ok")
