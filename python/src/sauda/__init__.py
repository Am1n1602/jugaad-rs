"""NSE market data for Python.

A thin client over a bundled Rust gRPC server (`jugaad-rpc`). Creating a
`Client` starts the server on a loopback port; closing it stops the server -
and so does this process dying, however it dies.
"""

from __future__ import annotations

import atexit
import os
import shutil
import socket
import subprocess
import sysconfig
import time

import grpc

from ._proto import jugaad_pb2 as pb
from ._proto import jugaad_pb2_grpc

__all__ = ["Client"]

_EXE = "jugaad-rpc" + (".exe" if os.name == "nt" else "")


def _find_binary() -> str:
    # Same places ruff looks: the env's scripts dir, then the --user one.
    user_scheme = (
        sysconfig.get_preferred_scheme("user")
        if hasattr(sysconfig, "get_preferred_scheme")
        else f"{os.name}_user"
    )
    for scheme in (None, user_scheme):
        try:
            scripts = (
                sysconfig.get_path("scripts", scheme)
                if scheme
                else sysconfig.get_path("scripts")
            )
        except KeyError:
            continue
        path = os.path.join(scripts, _EXE)
        if os.path.isfile(path):
            return path
    found = shutil.which(_EXE)
    if found:
        return found
    raise FileNotFoundError(f"bundled {_EXE} not found; is the sauda wheel installed?")


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _stop(proc: subprocess.Popen) -> None:
    # Closing stdin is the server's cue to exit (JUGAAD_RPC_EXIT_ON_STDIN_CLOSE).
    if proc.stdin:
        try:
            proc.stdin.close()
        except OSError:
            pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _start_server(timeout: float) -> tuple[subprocess.Popen, grpc.Channel]:
    exe = _find_binary()
    for _ in range(2):  # ponytail: free-port pick is racy, retry once; server could print its bound port instead
        port = _free_port()
        proc = subprocess.Popen(
            [exe],
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            env={
                **os.environ,
                "JUGAAD_RPC_ADDR": f"127.0.0.1:{port}",
                "JUGAAD_RPC_EXIT_ON_STDIN_CLOSE": "1",
            },
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        channel = grpc.insecure_channel(f"127.0.0.1:{port}")
        ready = grpc.channel_ready_future(channel)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and proc.poll() is None:
            try:
                ready.result(timeout=0.25)
                return proc, channel
            except grpc.FutureTimeoutError:
                pass
        ready.cancel()
        channel.close()
        if proc.poll() is None:
            _stop(proc)
            raise TimeoutError(f"jugaad-rpc not ready after {timeout}s")
    raise RuntimeError(f"jugaad-rpc exited during startup (code {proc.returncode})")


def _to_py(msg) -> dict:
    # Not MessageToDict: that renders uint64 as strings and drops unset
    # optionals, and None-vs-0 matters (e.g. delivery_pct).
    out = {}
    for f in msg.DESCRIPTOR.fields:
        value = getattr(msg, f.name)
        repeated = (
            f.is_repeated if hasattr(f, "is_repeated") else f.label == f.LABEL_REPEATED
        )
        if repeated:
            out[f.name] = [_to_py(v) if f.message_type else v for v in value]
        elif f.message_type:
            out[f.name] = _to_py(value) if msg.HasField(f.name) else None
        elif f.has_presence and not msg.HasField(f.name):
            out[f.name] = None
        else:
            out[f.name] = value
    return out


class Client:
    """Starts the bundled server on creation; use as a context manager or call
    `close()`. `stub` is the raw generated gRPC stub for every RPC."""

    def __init__(self, startup_timeout: float = 15.0) -> None:
        self._proc, self._channel = _start_server(startup_timeout)
        self.stub = jugaad_pb2_grpc.JugaadStub(self._channel)
        atexit.register(self.close)

    def close(self) -> None:
        if self._proc is None:
            return
        atexit.unregister(self.close)
        self._channel.close()
        _stop(self._proc)
        self._proc = None

    def __enter__(self) -> Client:
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def stock_quote(self, symbol: str) -> dict:
        return _to_py(self.stub.GetStockQuote(pb.StockQuoteRequest(symbol=symbol)))

    def stock_history(
        self, symbol: str, from_date, to_date, series: str | None = None
    ) -> list[dict]:
        """Daily OHLC/volume/delivery rows. Dates are `datetime.date` or "YYYY-MM-DD"."""
        request = pb.StockHistoryRequest(
            symbol=symbol, from_date=str(from_date), to_date=str(to_date), series=series
        )
        return [_to_py(row) for row in self.stub.GetStockHistory(request).rows]
