"""NSE market data for Python.

An async client over a Rust gRPC server (`jugaad-rpc`). The client only
connects: it assumes a server is already running, and starting or stopping
one is up to you (the `jugaad-rpc` command or the Docker image).
"""

from __future__ import annotations

import asyncio
import csv
import io
import os
import time
from collections.abc import AsyncIterator
from dataclasses import dataclass

from grpc import aio

from ._proto import jugaad_pb2 as pb
from ._proto import jugaad_pb2_grpc

__all__ = ["Client", "Config", "ConfigBuilder"]

DEFAULT_ADDR = "127.0.0.1:50051"
_DEFAULT_CONNECT_TIMEOUT = 15.0


@dataclass(frozen=True)
class Config:
    addr: str | None = None
    connect_timeout: float = _DEFAULT_CONNECT_TIMEOUT


class ConfigBuilder:
    """Builds a `Config`:

    - `addr`: the server to connect to (default 127.0.0.1:50051)
    - `connect_timeout`: seconds `Client.connect()` waits for the server
    """

    def __init__(self) -> None:
        self._addr: str | None = None
        self._connect_timeout = _DEFAULT_CONNECT_TIMEOUT

    def addr(self, addr: str) -> ConfigBuilder:
        if not addr:
            raise ValueError("addr must not be empty")
        self._addr = addr
        return self

    def connect_timeout(self, seconds: float) -> ConfigBuilder:
        self._connect_timeout = seconds
        return self

    def build(self) -> Config:
        if not self._connect_timeout > 0:
            raise ValueError("connect_timeout must be a positive number of seconds")
        return Config(self._addr, self._connect_timeout)

    def from_env(self) -> Config:
        """Applies SAUDA_ADDR and SAUDA_CONNECT_TIMEOUT (seconds) over anything
        already set, then builds. Unset or empty variables are ignored."""
        env = os.environ
        if env.get("SAUDA_ADDR"):
            self.addr(env["SAUDA_ADDR"])
        raw = env.get("SAUDA_CONNECT_TIMEOUT")
        if raw:
            try:
                self.connect_timeout(float(raw))
            except ValueError:
                raise ValueError(
                    f"SAUDA_CONNECT_TIMEOUT must be a number of seconds, got {raw!r}"
                ) from None
        return self.build()


def _open_channel(address: str) -> aio.Channel:
    # gRPC's default 4 MiB receive cap rejects big responses (a month of
    # corporate announcements is ~6 MB); the server is trusted, so only memory
    # bounds this. The short reconnect backoff keeps connecting quick when the
    # server has only just started listening (default 1s backoff otherwise).
    return aio.insecure_channel(
        address,
        options=[
            ("grpc.max_receive_message_length", -1),
            ("grpc.initial_reconnect_backoff_ms", 100),
            ("grpc.min_reconnect_backoff_ms", 100),
        ],
    )


async def _wait_ready(channel: aio.Channel, timeout: float, target: str) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            await asyncio.wait_for(channel.channel_ready(), 0.25)
            return
        except asyncio.TimeoutError:
            pass
    raise TimeoutError(f"no server became ready at {target} within {timeout}s")


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


def _rows(response) -> list[dict]:
    return [_to_py(row) for row in response.rows]


_OPTION_CHAIN_KINDS = {
    "index": pb.OPTION_CHAIN_KIND_INDEX,
    "equity": pb.OPTION_CHAIN_KIND_EQUITY,
}

_INSTRUMENTS = {
    "futidx": pb.INSTRUMENT_FUT_IDX,
    "futstk": pb.INSTRUMENT_FUT_STK,
    "optidx": pb.INSTRUMENT_OPT_IDX,
    "optstk": pb.INSTRUMENT_OPT_STK,
}

_OPTION_TYPES = {
    "call": pb.OPTION_TYPE_CALL,
    "ce": pb.OPTION_TYPE_CALL,
    "put": pb.OPTION_TYPE_PUT,
    "pe": pb.OPTION_TYPE_PUT,
}

_CHART_PERIODS = {
    "1d": pb.CHART_PERIOD_ONE_DAY,
    "1w": pb.CHART_PERIOD_ONE_WEEK,
    "1m": pb.CHART_PERIOD_ONE_MONTH,
    "1y": pb.CHART_PERIOD_ONE_YEAR,
    "5y": pb.CHART_PERIOD_FIVE_YEARS,
}

_INDEX_CHART_PERIODS = {
    "1d": pb.INDEX_CHART_PERIOD_ONE_DAY,
    "1w": pb.INDEX_CHART_PERIOD_ONE_WEEK,
    "1m": pb.INDEX_CHART_PERIOD_ONE_MONTH,
    "3m": pb.INDEX_CHART_PERIOD_THREE_MONTHS,
    "6m": pb.INDEX_CHART_PERIOD_SIX_MONTHS,
    "1y": pb.INDEX_CHART_PERIOD_ONE_YEAR,
    "5y": pb.INDEX_CHART_PERIOD_FIVE_YEARS,
}


def _choice(table: dict, value, name: str, options: str):
    try:
        return table[str(value).lower().replace("-", "").replace("_", "")]
    except KeyError:
        raise ValueError(f"{name} must be {options}, got {value!r}") from None


def _csv_rows(text: str) -> list[dict]:
    # NSE's old-format files end every line with a comma, which csv reads as an
    # extra column with an empty name; drop it.
    return [
        {k: v for k, v in row.items() if k} for row in csv.DictReader(io.StringIO(text))
    ]


class Client:
    """Async client for a `jugaad-rpc` server that is already running at
    `config.addr` (default 127.0.0.1:50051). It never starts or stops a
    server. Call `await connect()` before anything else and
    `await disconnect()` when done, or use `async with`. A connected client
    belongs to the event loop it connected in."""

    def __init__(self, config: Config | None = None) -> None:
        self._config = config
        self._channel: aio.Channel | None = None
        self._stub = None

    @property
    def stub(self):
        """The raw generated async gRPC stub, exposing every RPC."""
        if self._stub is None:
            raise RuntimeError("not connected: call `await client.connect()` first")
        return self._stub

    async def connect(self, config: Config | None = None) -> None:
        """Uses `config`, else the one given to `Client(...)`, else the defaults."""
        if self._channel is not None:
            raise RuntimeError(
                "already connected: call `await client.disconnect()` first"
            )
        config = config or self._config or ConfigBuilder().build()
        addr = config.addr or DEFAULT_ADDR
        channel = _open_channel(addr)
        try:
            await _wait_ready(channel, config.connect_timeout, addr)
        except BaseException as e:
            await channel.close()
            if isinstance(e, TimeoutError):
                raise TimeoutError(
                    f"{e}; start one with the jugaad-rpc command or the Docker image"
                ) from None
            raise
        self._channel = channel
        self._stub = jugaad_pb2_grpc.JugaadStub(channel)

    async def disconnect(self) -> None:
        """Closes the connection; the server keeps running."""
        if self._channel is None:
            return
        channel = self._channel
        self._channel = self._stub = None
        await channel.close()

    async def __aenter__(self) -> Client:
        await self.connect()
        return self

    async def __aexit__(self, *exc) -> None:
        await self.disconnect()

    async def stock_quote(self, symbol: str) -> dict:
        return _to_py(
            await self.stub.GetStockQuote(pb.StockQuoteRequest(symbol=symbol))
        )

    async def watch_stock_quote(
        self, symbol: str, interval: int = 3
    ) -> AsyncIterator[dict]:
        """Yields a fresh quote every `interval` whole seconds until the
        generator is closed (`aclose()`, or `disconnect()` which ends every
        stream)."""
        call = self.stub.WatchStockQuote(
            pb.WatchStockQuoteRequest(symbol=symbol, interval_seconds=interval)
        )
        try:
            async for quote in call:
                yield _to_py(quote)
        finally:
            call.cancel()

    # Dates are `datetime.date` or "YYYY-MM-DD" everywhere below.

    async def stock_history(
        self, symbol: str, from_date, to_date, series: str | None = None
    ) -> list[dict]:
        """Daily OHLC/volume/delivery rows."""
        request = pb.StockHistoryRequest(
            symbol=symbol, from_date=str(from_date), to_date=str(to_date), series=series
        )
        return _rows(await self.stub.GetStockHistory(request))

    async def index_history(self, name: str, from_date, to_date) -> list[dict]:
        """Daily OHLC for an index, e.g. "NIFTY 50"."""
        request = pb.IndexHistoryRequest(
            name=name, from_date=str(from_date), to_date=str(to_date)
        )
        return _rows(await self.stub.GetIndexHistory(request))

    async def index_snapshot(self) -> list[dict]:
        """Live snapshot of every NSE index."""
        return _rows(await self.stub.GetIndexSnapshot(pb.IndexSnapshotRequest()))

    async def large_deals(self) -> list[dict]:
        """Today's bulk, short and block deals."""
        return _rows(await self.stub.GetLargeDeals(pb.LargeDealsRequest()))

    async def market_status(self) -> list[dict]:
        """Open/closed status per market segment, holiday-aware."""
        response = await self.stub.GetMarketStatus(pb.MarketStatusRequest())
        return [_to_py(s) for s in response.segments]

    async def option_chain(
        self, symbol: str, kind: str = "index", expiry=None
    ) -> list[dict]:
        """`kind` ("index" or "equity") is NSE's own query parameter; it
        currently returns identical data for either value, so pass "equity"
        for stocks only to match NSE's convention. `expiry` defaults to the
        nearest."""
        try:
            kind_enum = _OPTION_CHAIN_KINDS[kind.lower()]
        except KeyError:
            raise ValueError(f'kind must be "index" or "equity", got {kind!r}') from None
        request = pb.OptionChainRequest(
            symbol=symbol,
            kind=kind_enum,
            expiry=None if expiry is None else str(expiry),
        )
        return _rows(await self.stub.GetOptionChain(request))

    async def option_expiries(self, symbol: str) -> list[str]:
        """Every available option expiry as "YYYY-MM-DD", nearest first."""
        response = await self.stub.GetOptionExpiries(
            pb.OptionExpiriesRequest(symbol=symbol)
        )
        return list(response.expiries)

    async def corporate_announcements(
        self, from_date, to_date, segment: str = "equities", symbol: str | None = None
    ) -> list[dict]:
        """Exchange disclosures. `segment` is equities, sme, debt, mf,
        invitsreits or municipalBond; omit `symbol` for the whole segment."""
        request = pb.CorporateAnnouncementsRequest(
            segment=segment,
            symbol=symbol,
            from_date=str(from_date),
            to_date=str(to_date),
        )
        return _rows(await self.stub.GetCorporateAnnouncements(request))

    async def bhavcopy(self, date) -> list[dict]:
        """Whole-market cash bhavcopy for one trading day. Rows are NSE's own
        CSV columns with every value a string; the columns differ before and
        after 2024-07-08, when NSE changed the file format."""
        response = await self.stub.GetBhavcopy(pb.BhavcopyRequest(date=str(date)))
        return _csv_rows(response.csv)

    async def fo_bhavcopy(self, date) -> list[dict]:
        """F&O bhavcopy for one trading day, same row shape as `bhavcopy`. The
        file is 5-6 MB, which the server streams in chunks."""
        data = bytearray()
        async for chunk in self.stub.GetFoBhavcopy(pb.BhavcopyRequest(date=str(date))):
            data += chunk.data
        return _csv_rows(data.decode("utf-8"))

    async def derivatives_history(
        self,
        symbol: str,
        from_date,
        to_date,
        expiry,
        instrument: str,
        strike_price: float | None = None,
        option_type: str | None = None,
    ) -> list[dict]:
        """Daily history for one F&O contract. `instrument` is "fut-idx",
        "fut-stk", "opt-idx" or "opt-stk". Options need `strike_price` and
        `option_type` ("call" or "put"); futures must not have them."""
        request = pb.DerivativesHistoryRequest(
            symbol=symbol,
            from_date=str(from_date),
            to_date=str(to_date),
            expiry=str(expiry),
            instrument=_choice(
                _INSTRUMENTS,
                instrument,
                "instrument",
                '"fut-idx", "fut-stk", "opt-idx" or "opt-stk"',
            ),
            strike_price=strike_price,
            option_type=None
            if option_type is None
            else _choice(_OPTION_TYPES, option_type, "option_type", '"call" or "put"'),
        )
        return _rows(await self.stub.GetDerivativesHistory(request))

    async def stock_chart(self, symbol: str, period: str = "1d") -> dict:
        """Price chart: `{"identifier", "name", "close_price", "points"}`, with
        the points newest first. `period` is "1d", "1w", "1m", "1y" or "5y"."""
        request = pb.StockChartRequest(
            symbol=symbol,
            period=_choice(
                _CHART_PERIODS, period, "period", '"1d", "1w", "1m", "1y" or "5y"'
            ),
        )
        return _to_py(await self.stub.GetStockChart(request))

    async def index_chart(self, name: str, period: str = "1d") -> dict:
        """Index price chart, same shape as `stock_chart`, points newest first.
        `period` is "1d", "1w", "1m", "3m", "6m", "1y" or "5y"."""
        request = pb.IndexChartRequest(
            name=name,
            period=_choice(
                _INDEX_CHART_PERIODS,
                period,
                "period",
                '"1d", "1w", "1m", "3m", "6m", "1y" or "5y"',
            ),
        )
        return _to_py(await self.stub.GetIndexChart(request))

    async def market_movers(self) -> list[dict]:
        """Top gainers and losers across NSE's seven index/security scopes."""
        return _rows(await self.stub.GetMarketMovers(pb.MarketMoversRequest()))

    async def most_active_equities(self) -> list[dict]:
        """Most active equities by traded value and by traded volume."""
        return _rows(
            await self.stub.GetMostActiveEquities(pb.MostActiveEquitiesRequest())
        )

    async def fifty_two_week(self) -> list[dict]:
        """Stocks at a new 52-week high or low."""
        return _rows(await self.stub.GetFiftyTwoWeek(pb.FiftyTwoWeekRequest()))

    async def holiday_list(self) -> list[dict]:
        """NSE trading holidays across every market segment."""
        return _rows(await self.stub.GetHolidayList(pb.HolidayListRequest()))
