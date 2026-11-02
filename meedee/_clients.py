"""One ownership facade for isolated and in-process Rust transport."""

from __future__ import annotations

import inspect
import math
from collections.abc import Callable, Iterable
from fnmatch import fnmatchcase
from fractions import Fraction
from functools import partial
from types import TracebackType
from typing import cast
from uuid import uuid4

from . import _native, _units, codecs, events, gm, messages, midi
from ._errors import CompoundSendError
from ._output_state import OutputState
from ._transport import DEFAULT_RECEIVE_BYTE_LIMIT, Context, Delivery
from .events import Clock, ReceivedPacket


class Input:
    __slots__ = ("_native", "_context")

    def __init__(
        self,
        *,
        protocol: midi.Protocol = midi.Protocol.Midi1,
        backend: midi.Backend | None = None,
        timestamps: midi.TimestampMode = midi.TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float | str = 5.0,
        receive_byte_limit: int | str = DEFAULT_RECEIVE_BYTE_LIMIT,
    ) -> None:
        timeout = _units.real(timeout, "second")
        validate_settings(protocol, backend, isolated, timeout)
        if not isinstance(timestamps, midi.TimestampMode):
            raise TypeError("timestamps must be TimestampMode")
        receive_byte_limit = _units.integer(receive_byte_limit, "byte")
        if type(receive_byte_limit) is not int:
            raise TypeError("receive_byte_limit must be a built-in integer")
        if receive_byte_limit <= 0:
            raise ValueError("receive_byte_limit must be positive")
        settings: dict[str, object] = {
            "identity": uuid4().hex,
            "protocol": protocol,
            "backend": backend,
            "timestamps": timestamps,
            "receive_byte_limit": receive_byte_limit,
        }
        self._native = None if isolated else _native.Input(**settings)
        self._context = Context(timeout, receive_byte_limit) if isolated else None
        if self._context is not None:
            self._context.request("init", [True, wire_settings(settings)])

    def ports(self) -> list[midi.Port]:
        if self._native is not None:
            return self._native.ports()
        assert self._context is not None
        return [
            port_from_wire(cast(tuple[object, ...], p))
            for p in cast(list[object], self._context.request("ports", []))
        ]

    @property
    def worker_failure(self) -> Exception | None:
        return None if self._context is None else self._context.failure_exception

    def connect(
        self, port: midi.Port, callback: Callable[[ReceivedPacket], object]
    ) -> InputConnection:
        validate_open(callback=callback)
        if callback is None:
            raise TypeError("MIDI callback must be callable")
        if not isinstance(port, midi.Port):
            raise TypeError("port must be a discovery snapshot")
        identity = uuid4().hex
        connection = InputConnection._create(self)
        if self._native is not None:
            connection._native = self._native.connect(port, callback, identity)
        else:
            assert self._context is not None
            connection._delivery = self._context.open(
                "connect", port._wire, "", callback, identity
            )
            connection._generation = self._context.generation
        connection._closed = False
        return connection

    def create_virtual(
        self, name: str, callback: Callable[[ReceivedPacket], object]
    ) -> InputConnection:
        validate_open(name, callback)
        if callback is None:
            raise TypeError("MIDI callback must be callable")
        identity = uuid4().hex
        connection = InputConnection._create(self)
        if self._native is not None:
            connection._native = self._native.create_virtual(name, callback, identity)
        else:
            assert self._context is not None
            connection._delivery = self._context.open(
                "virtual", None, name, callback, identity
            )
            connection._generation = self._context.generation
        connection._closed = False
        return connection

    def __del__(self) -> None:
        if (context := getattr(self, "_context", None)) is not None:
            context.dispose()


class Output:
    """Discover MIDI destinations and open message/packet output connections."""

    __slots__ = (
        "_native",
        "_context",
        "_protocol",
        "_channel",
        "_port",
        "_connection",
        "_beat_period",
    )

    def __init__(
        self,
        *,
        channel: int = 1,
        bpm: int | float | None = None,
        port: str | None = None,
        protocol: midi.Protocol = midi.Protocol.Midi1,
        backend: midi.Backend | None = None,
        isolated: bool = False,
        timeout: float | str = 5.0,
    ) -> None:
        timeout = _units.real(timeout, "second")
        validate_settings(protocol, backend, isolated, timeout)
        messages.NoteOn(0, channel=channel)
        if channel is None:
            raise TypeError("output channel must be an integer")
        if port is not None and not isinstance(port, str):
            raise TypeError("port must be a name pattern or None")
        if bpm is not None:
            if type(bpm) not in (int, float):
                raise TypeError("bpm must be a number")
            if not math.isfinite(bpm) or bpm <= 0:
                raise ValueError("bpm must be positive and finite")
        self._beat_period = None if bpm is None else Fraction(60) / Fraction(str(bpm))
        self._channel = channel
        self._port = port
        self._connection: OutputConnection | None = None
        self._protocol = protocol
        settings: dict[str, object] = {
            "identity": uuid4().hex,
            "protocol": protocol,
            "backend": backend,
        }
        self._native = None if isolated else _native.Output(**settings)
        self._context = Context(timeout) if isolated else None
        if self._context is not None:
            self._context.request("init", [False, wire_settings(settings)])

    def ports(self) -> list[midi.Port]:
        if self._native is not None:
            return self._native.ports()
        assert self._context is not None
        return [
            port_from_wire(cast(tuple[object, ...], p))
            for p in cast(list[object], self._context.request("ports", []))
        ]

    @property
    def worker_failure(self) -> Exception | None:
        return None if self._context is None else self._context.failure_exception

    def connect(self, port: midi.Port) -> OutputConnection:
        _native.check_thread()
        if not isinstance(port, midi.Port):
            raise TypeError("port must be a discovery snapshot")
        connection = OutputConnection._create(self)
        if self._native is not None:
            connection._native = self._native.connect(port)
        else:
            assert self._context is not None
            self._context.open("connect", port._wire, "", None, "")
            connection._generation = self._context.generation
        connection._closed = False
        return connection

    def create_virtual(self, name: str) -> OutputConnection:
        validate_open(name)
        connection = OutputConnection._create(self)
        if self._native is not None:
            connection._native = self._native.create_virtual(name)
        else:
            assert self._context is not None
            self._context.open("virtual", None, name, None, "")
            connection._generation = self._context.generation
        connection._closed = False
        return connection

    def __enter__(self) -> OutputConnection:
        if self._connection is not None:
            raise _native.StateError("output context is already open")
        ports = self.ports()
        if self._port is not None:
            pattern = self._port
            ports = [
                p for p in ports if fnmatchcase(p.name, pattern) or pattern in p.name
            ]
            if len(ports) != 1:
                raise ValueError(
                    f"Port pattern {pattern!r} matched {len(ports)} outputs; "
                    "expected one"
                )
        if not ports:
            raise ValueError("No MIDI output ports available")
        self._connection = self.connect(ports[0])
        return self._connection

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        connection = self._connection
        self._connection = None
        if connection is not None:
            close_context(connection.close, exc_value)

    def __del__(self) -> None:
        if (context := getattr(self, "_context", None)) is not None:
            context.dispose()


class InputConnection:
    __slots__ = (
        "_client",
        "_native",
        "_delivery",
        "_closed",
        "_generation",
        "__weakref__",
    )

    def __init__(self) -> None:
        raise TypeError("MIDI connection handles cannot be constructed directly")

    @classmethod
    def _create(cls, client: Input) -> InputConnection:
        connection = object.__new__(cls)
        connection._client = client
        connection._native = cast(_native.InputConnection | None, None)
        connection._delivery = cast(Delivery | None, None)
        connection._closed = True
        connection._generation = (
            0 if client._context is None else client._context.generation
        )
        return connection

    @property
    def closed(self) -> bool:
        if self._native is not None:
            return self._native.closed
        assert self._client._context is not None
        return self._closed or bool(self._client._context.failure)

    @property
    def clock(self) -> Clock:
        if self._native is not None:
            return self._native.clock
        assert self._delivery is not None and self._delivery.clock is not None
        return self._delivery.clock

    @property
    def dropped_packets(self) -> int:
        if self._native is not None:
            return self._native.dropped_packets
        assert self._delivery is not None
        return (
            self._delivery.dropped
            + self._delivery.worker_dropped
            + self._delivery.bridge.discarded_packets
        )

    @property
    def worker_failure(self) -> Exception | None:
        return self._client.worker_failure

    @property
    def last_message_time(self) -> float | None:
        if self._native is not None:
            return self._native.last_message_time
        return (
            None if self._delivery is None else self._delivery.bridge.last_message_time
        )

    def close(self) -> None:
        _native.check_thread()
        if self._native is not None:
            self._native.close()
        elif not self._closed:
            assert self._client._context is not None and self._delivery is not None
            self._delivery.retire()
            try:
                self._client._context.close(self._generation)
                self._closed = True
            finally:
                self._delivery.drain()

    def __enter__(self) -> InputConnection:
        if self._native is not None:
            self._native.__enter__()
        elif self.closed:
            raise _native.StateError("MIDI connection is closed or failed")
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        close_context(self.close, exc_value)

    def __del__(self) -> None:
        if (delivery := getattr(self, "_delivery", None)) is not None:
            delivery.retire()
            if not self._closed and self._client._context is not None:
                self._client._context.defer_close(self._generation)


class OutputConnection:
    """An open destination for messages, encoded packets and timed playback."""

    __slots__ = (
        "_client",
        "_native",
        "_closed",
        "_generation",
        "_state",
        "__weakref__",
    )

    def __init__(self) -> None:
        raise TypeError("MIDI connection handles cannot be constructed directly")

    @classmethod
    def _create(cls, client: Output) -> OutputConnection:
        connection = object.__new__(cls)
        connection._client = client
        connection._state = OutputState()
        connection._native = cast(_native.OutputConnection | None, None)
        connection._closed = True
        connection._generation = (
            0 if client._context is None else client._context.generation
        )
        return connection

    def play(
        self,
        source: Iterable[object],
        *,
        origin: events.Timestamp,
        mappings: list[events.ClockMapping]
        | tuple[events.ClockMapping, ...]
        | None = None,
        cancel: events.Cancellation | None = None,
        allow_rounding: bool = False,
        metrics: bool = False,
    ) -> events.PlaybackReport:
        """Play timestamped events on the caller's thread, stopping on failure."""
        return _native.play(
            self,
            source,
            origin=origin,
            mappings=mappings,
            cancel=cancel,
            allow_rounding=allow_rounding,
            metrics=metrics,
        )

    def __call__(
        self,
        *values: messages.Note
        | messages.Rest
        | messages.Message
        | bytes
        | bytearray
        | memoryview
        | midi.UmpPacket
        | int
        | float
        | str,
    ) -> None:
        """Schedule notes, rests, messages and delays against one monotonic origin."""
        _native.check_thread()
        if self.closed:
            raise _native.StateError("MIDI connection is closed or failed")
        elapsed = Fraction(0)
        timeline: list[tuple[int, object]] = []
        for v in values:
            deadline = round(elapsed * 1_000_000_000)
            if isinstance(v, (messages.Note, messages.Rest, int, float, str)):
                duration = (
                    v.duration if isinstance(v, (messages.Note, messages.Rest)) else v
                )
                elapsed += _units.sequence_duration(duration, self._client._beat_period)
                end = round(elapsed * 1_000_000_000)
                if end > 2**64 - 1:
                    raise ValueError("delay exceeds scheduler range")
                if isinstance(v, messages.Note):
                    channel = self._client._channel if v.channel is None else v.channel
                    attack = messages.NoteOn(v.note, v.velocity, channel)
                    release = messages.NoteOff(v.note, channel=channel)
                    timeline.append(
                        (
                            deadline,
                            _native.prepare_output(self._client._protocol, attack),
                        )
                    )
                    timeline.append(
                        (end, _native.prepare_output(self._client._protocol, release))
                    )
            else:
                if (
                    isinstance(
                        v,
                        (
                            gm.BankProgramSelect,
                            messages.NoteOn,
                            messages.NoteOff,
                            messages.PolyPressure,
                            messages.ControlChange,
                            messages.ProgramChange,
                            messages.ChannelPressure,
                            messages.PitchBend,
                            messages.ControlChange14,
                            messages.RpnSet,
                            messages.NrpnSet,
                            messages.RpnIncrement,
                            messages.RpnDecrement,
                            messages.NrpnIncrement,
                            messages.NrpnDecrement,
                        ),
                    )
                    and v.channel is None
                ):
                    v = v.replace(channel=self._client._channel)
                prepared = _native.prepare_output(self._client._protocol, v)
                timeline.append(
                    (deadline, v if isinstance(prepared, tuple) else prepared)
                )
        timeline.append((round(elapsed * 1_000_000_000), None))
        _native.output_sequence(self._send, timeline)

    def _send(self, value: object) -> None:
        """Send a message, compound command or encoded packet immediately.

        Compound operations serialize their entire expansion on this connection.
        CompoundSendError reports known progress when part of an operation fails.
        Protocol conversion is never implicit.
        """
        _native.check_thread()
        prepared = _native.prepare_output(self._client._protocol, value)
        if isinstance(prepared, tuple):
            total = prepared[2]
            packets = [
                codecs.encode(m)
                for m in cast(
                    Callable[[object], list[messages.Message]], codecs.expand
                )(value)
            ]
            try:
                if self._native is not None:
                    self._native.send_command(value)
                else:
                    if self.closed:
                        raise _native.StateError("MIDI connection is closed or failed")
                    assert self._client._context is not None
                    self._client._context.request(
                        "send_command", [prepared, self._generation], serial=True
                    )
            except CompoundSendError as error:
                for p in packets[: error.completed_messages or 0]:
                    self._state.feed(p)
                raise
            except (
                _native.WorkerError,
                _native.WorkerTimeoutError,
                _native.NativePanicError,
            ) as error:
                raise CompoundSendError(str(error), None, total) from error
            for p in packets:
                self._state.feed(p)
        elif self._native is not None:
            self._native.send(prepared)
        else:
            if self.closed:
                raise _native.StateError("MIDI connection is closed or failed")
            assert self._client._context is not None
            self._client._context.request(
                "send",
                [
                    prepared.words
                    if isinstance(prepared, midi.UmpPacket)
                    else prepared,
                    self._generation,
                ],
                serial=True,
            )

        if isinstance(prepared, bytes):
            self._state.feed(prepared)

    @property
    def closed(self) -> bool:
        if self._native is not None:
            return self._native.closed
        assert self._client._context is not None
        return self._closed or bool(self._client._context.failure)

    @property
    def worker_failure(self) -> Exception | None:
        return self._client.worker_failure

    def close(self) -> None:
        _native.check_thread()
        if self.closed:
            self._close_transport()
            return
        try:
            for m in self._state.cleanup():
                self._send(m)
        except BaseException as error:
            close_context(self._close_transport, error)
            raise
        self._close_transport()

    def _close_transport(self) -> None:
        if self._native is not None:
            self._native.close()
        elif not self._closed:
            assert self._client._context is not None
            self._client._context.close(self._generation)
            self._closed = True

    def __enter__(self) -> OutputConnection:
        if self._native is not None:
            self._native.__enter__()
        elif self.closed:
            raise _native.StateError("MIDI connection is closed or failed")
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        close_context(self.close, exc_value)

    def __del__(self) -> None:
        if hasattr(self, "_client") and self._native is None and not self._closed:
            assert self._client._context is not None
            self._client._context.defer_close(self._generation)


def port_from_wire(wire: tuple[object, ...]) -> midi.Port:
    owner, snapshot, index, name, backend, backend_id, input_port = wire
    return midi.Port._from_wire(
        cast(str, owner),
        cast(int, snapshot),
        cast(int, index),
        cast(str, name),
        getattr(midi.Backend, cast(str, backend)),
        cast(int, backend_id),
        cast(bool, input_port),
    )


def wire_settings(settings: dict[str, object]) -> dict[str, object]:
    return {
        k: str(v).rpartition(".")[2]
        if isinstance(v, (midi.Protocol, midi.Backend, midi.TimestampMode))
        else v
        for k, v in settings.items()
    }


def close_context(close: Callable[[], None], body_error: BaseException | None) -> None:
    try:
        close()
    except BaseException as close_error:
        if body_error is None:
            raise
        raise BaseExceptionGroup(
            "MIDI context body and close both failed", [body_error, close_error]
        ) from None


def validate_settings(
    protocol: midi.Protocol,
    backend: midi.Backend | None,
    isolated: bool,
    timeout: float,
) -> None:
    _native.check_thread()
    if not isinstance(protocol, midi.Protocol):
        raise TypeError("protocol must be Protocol")
    if backend is not None and not isinstance(backend, midi.Backend):
        raise TypeError("backend must be Backend or None")
    if type(isolated) is not bool:
        raise TypeError("isolated must be a boolean")
    if not isinstance(timeout, (int, float)) or isinstance(timeout, bool):
        raise TypeError("timeout must be a number")
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("timeout must be positive and finite")


def validate_open(
    name: str = "", callback: Callable[..., object] | None = None
) -> None:
    _native.check_thread()
    if not isinstance(name, str):
        raise TypeError("virtual endpoint names must be strings")
    if "\0" in name:
        raise ValueError("virtual endpoint names must not contain NUL")
    if callback is None:
        return
    if not callable(callback):
        raise TypeError("MIDI callback must be callable")
    target = callback
    while isinstance(target, partial):
        target = target.func
    for c in (target, target.__call__):
        if (
            inspect.iscoroutinefunction(c)
            or inspect.isgeneratorfunction(c)
            or inspect.isasyncgenfunction(c)
        ):
            raise TypeError("MIDI callback must be synchronous and not a generator")
    signature_callback = callback
    if isinstance(inspect.getattr_static(type(target), "__call__", None), staticmethod):
        signature_callback = target.__call__
        if isinstance(callback, partial):
            signature_callback = partial(
                signature_callback, *callback.args, **callback.keywords
            )
    try:
        signature = inspect.signature(signature_callback)
    except (TypeError, ValueError):
        return
    try:
        signature.bind(object())
    except TypeError as error:
        raise TypeError(
            "MIDI callback must accept one positional ReceivedPacket"
        ) from error
