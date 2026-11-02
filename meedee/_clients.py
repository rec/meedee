"""One ownership facade for isolated and in-process Rust transport."""

from __future__ import annotations

import inspect
import math
import sys
from _thread import LockType
from collections.abc import Callable, Generator, Iterable, Iterator
from fnmatch import fnmatchcase
from fractions import Fraction
from functools import partial
from os import PathLike
from queue import Empty, Full
from threading import Lock
from time import monotonic
from types import TracebackType
from typing import cast
from uuid import uuid4

from . import _native, _units, archive, codecs, events, gm, messages, midi
from ._errors import CompoundSendError
from ._output_state import OutputState
from ._transport import DEFAULT_RECEIVE_BYTE_LIMIT, Context, Delivery, ReceiveQueue
from .events import Clock


class Input[T = events.Event | events.StreamDiagnostic]:
    __slots__ = (
        "_native",
        "_context",
        "_protocol",
        "_raw_mode",
        "_event_limit",
        "_sysex_limit",
        "_byte_limit",
        "_channels",
        "_connection",
    )

    def __init__(
        self,
        *,
        raw: bool = False,
        channel: int | Iterable[int] | None = None,
        event_limit: int = 128,
        sysex_limit: int | str = 1_048_576,
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
        if type(raw) is not bool:
            raise TypeError("raw must be a boolean")
        sysex_limit = _units.integer(sysex_limit, "byte")
        for n in (event_limit, sysex_limit):
            if type(n) is not int:
                raise TypeError("event_limit and sysex_limit must be built-in integers")
            if n <= 0:
                raise ValueError("event_limit and sysex_limit must be positive")
        if channel is None:
            channels = None
        else:
            values = [channel] if isinstance(channel, int) else list(channel)
            for c in values:
                if type(c) is not int:
                    raise TypeError("channels must be built-in integers")
                if not 1 <= c <= 16:
                    raise ValueError("channels must be in 1..16")
            channels = frozenset(values)
        if raw and channels is not None:
            raise ValueError("channel filtering requires decoded input (raw=False)")
        self._channels = channels
        self._connection: InputConnection[T] | None = None
        self._protocol = protocol
        self._raw_mode = raw
        self._event_limit = event_limit
        self._sysex_limit = sysex_limit
        self._byte_limit = receive_byte_limit
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
        self, port: midi.Port, callback: Callable[[T], object] | None = None
    ) -> InputConnection[T]:
        return self._open(port, None, callback)

    def create_virtual(
        self, name: str, callback: Callable[[T], object] | None = None
    ) -> InputConnection[T]:
        validate_open(name, callback)
        return self._open(None, name, callback)

    def _open(
        self,
        port: midi.Port | None,
        name: str | None,
        callback: Callable[[T], object] | None,
    ) -> InputConnection[T]:
        validate_open(callback=callback)
        if name is None and not isinstance(port, midi.Port):
            raise TypeError("port must be a discovery snapshot")
        identity = uuid4().hex
        connection = InputConnection[T]._create(self, callback)
        transport = _InputTransport[T]._create(self)
        if self._native is not None:
            transport._native = (
                self._native.connect(
                    cast(midi.Port, port), connection._deliver, identity
                )
                if name is None
                else self._native.create_virtual(name, connection._deliver, identity)
            )
        else:
            assert self._context is not None
            transport._delivery = self._context.open(
                "connect" if name is None else "virtual",
                None if port is None else port._wire,
                name or "",
                connection._deliver,
                identity,
            )
            transport._generation = self._context.generation
        transport._closed = False
        connection._raw = transport
        return connection

    def __enter__(self) -> InputConnection[T]:
        if self._connection is not None:
            raise midi.StateError("input context is already open")
        ports = self.ports()
        if not ports:
            raise midi.ConnectError("No MIDI input ports available")
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

    def __iter__(self) -> Iterator[messages.Message | events.ReceivedPacket]:
        for item in self.with_errors():
            if isinstance(item, (messages.Message, events.ReceivedPacket)):
                yield item
            else:
                print(item, file=sys.stderr)

    def with_errors(
        self,
    ) -> Generator[
        messages.Message | events.ReceivedPacket | events.StreamDiagnostic | Exception,
    ]:
        if self._connection is not None:
            yield from self._connection.with_errors()
            return
        connection = None
        try:
            connection = self.__enter__()
            yield from connection.with_errors()
        except INPUT_ERRORS as error:
            yield error
        finally:
            self._connection = None
            if connection is not None:
                connection.close()

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


class _InputTransport[T]:
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
    def _create(cls, client: Input[T]) -> _InputTransport[T]:
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


class InputConnection[T = events.Event | events.StreamDiagnostic]:
    _raw: _InputTransport[T] | None
    _channels: frozenset[int] | None
    _stream: str
    _decoder: events.PacketDecoder | None
    _bridge: _native.CallbackBridge | None
    _queue: ReceiveQueue[T]
    _lock: LockType
    _closing: bool
    _failure: Exception | None
    _recorder: archive.Recorder | None
    _recording_starting: bool
    _known_lost: int
    _ignored_on_close: int
    _recording_report: archive.RecordingReport | None

    def __init__(self) -> None:
        raise TypeError("connection handles are created by Input")

    @classmethod
    def _create(
        cls,
        client: Input[T],
        callback: Callable[[T], object] | None,
    ) -> InputConnection[T]:
        connection = object.__new__(cls)
        connection._raw = None
        connection._channels = client._channels
        connection._stream = uuid4().hex
        connection._decoder = (
            None
            if client._raw_mode
            else events.PacketDecoder(
                stream=connection._stream,
                protocol=client._protocol,
                sysex_limit=client._sysex_limit,
            )
        )
        connection._bridge = (
            None if callback is None else _native.CallbackBridge(callback)
        )
        connection._queue = ReceiveQueue[T](
            client._byte_limit, capacity=client._event_limit, size_of=input_size
        )
        connection._lock = Lock()
        connection._closing = False
        connection._failure = None
        connection._recorder = None
        connection._recording_starting = False
        connection._known_lost = 0
        connection._ignored_on_close = 0
        connection._recording_report = None
        return connection

    @property
    def stream(self) -> str:
        return self._stream

    @property
    def clock(self) -> events.Clock:
        if self._raw is None:
            raise midi.StateError("input is not open yet")
        return self._raw.clock

    @property
    def closed(self) -> bool:
        return self._raw is None or self._raw.closed

    @property
    def dropped_packets(self) -> int:
        """Observed raw receive drops, including drops followed by silence."""
        return 0 if self._raw is None else self._raw.dropped_packets

    @property
    def failure(self) -> Exception | None:
        return self._failure or (
            None if self._raw is None else self._raw.worker_failure
        )

    @property
    def worker_failure(self) -> Exception | None:
        return None if self._raw is None else self._raw.worker_failure

    @property
    def last_message_time(self) -> float | None:
        if self._bridge is not None:
            return self._bridge.last_message_time
        return None if self._raw is None else self._raw.last_message_time

    @property
    def recording_report(self) -> archive.RecordingReport | None:
        return self._recording_report

    def _accept(
        self,
        items: list[events.Event | events.StreamDiagnostic | events.ReceivedPacket],
    ) -> None:
        for item in items:
            if isinstance(item, events.StreamDiagnostic) and item.kind == "loss":
                self._known_lost += item.lost
            if self._recorder is not None and not isinstance(
                item, events.ReceivedPacket
            ):
                self._recorder.record(item)
            if self._bridge is None and self._failure is None:
                try:
                    self._queue.put_nowait(cast(T, item))
                except Full:
                    self._failure = midi.ResourceError(
                        "input receive queue overflowed; "
                        "recording has independent admission"
                    )

    def _deliver(self, packet: events.ReceivedPacket) -> None:
        with self._lock:
            if self._closing:
                self._ignored_on_close += 1
                return
            items: list[
                events.Event | events.StreamDiagnostic | events.ReceivedPacket
            ] = [packet] if self._decoder is None else list(self._decoder.feed(packet))
            if self._channels is not None:
                items = [
                    i
                    for i in items
                    if not isinstance(i, events.Event)
                    or getattr(i.message, "channel", None) in self._channels
                ]
            self._accept(items)
        if self._bridge is not None:
            for item in items:
                if isinstance(item, events.ReceivedPacket):
                    self._bridge.deliver(item)
                else:
                    self._bridge.deliver_typed(item)

    def __iter__(self) -> Iterator[messages.Message | events.ReceivedPacket]:
        for item in self.with_errors():
            if isinstance(item, (messages.Message, events.ReceivedPacket)):
                yield item
            else:
                print(item, file=sys.stderr)

    def with_errors(
        self,
    ) -> Generator[
        messages.Message | events.ReceivedPacket | events.StreamDiagnostic | Exception,
    ]:
        if self._bridge is not None:
            raise midi.StateError("iteration is unavailable in callback mode")
        reported_lost = 0
        while True:
            try:
                item = self.receive(timeout=0.05)
            except INPUT_ERRORS as error:
                if self.failure is not None or not self.closed:
                    yield error
                return
            if isinstance(item, events.Event):
                yield item.message
            elif isinstance(item, (events.StreamDiagnostic, events.ReceivedPacket)):
                yield item
            lost = self.dropped_packets
            if lost > max(reported_lost, self._known_lost):
                yield midi.ResourceError(
                    f"Input lost {lost - max(reported_lost, self._known_lost)} packets"
                )
            reported_lost = max(reported_lost, lost)

    def receive(
        self,
        *,
        timeout: float | str | None = None,
        cancel: events.Cancellation | None = None,
    ) -> T | None:
        _native.check_thread()
        if self._bridge is not None:
            raise midi.StateError("receive is unavailable in callback mode")
        if timeout is not None:
            timeout = _units.real(timeout, "second")
        if timeout is not None and (
            isinstance(timeout, bool)
            or not isinstance(timeout, (int, float))
            or not math.isfinite(timeout)
            or timeout < 0
        ):
            raise ValueError("timeout must be None or a finite nonnegative number")
        deadline = None if timeout is None else monotonic() + timeout
        while True:
            if cancel is not None and cancel.cancelled:
                raise InterruptedError("MIDI receive cancelled")
            try:
                return self._queue.get_nowait()
            except Empty:
                pass
            if (failure := self.failure) is not None:
                raise failure
            if self._closing or self.closed:
                raise midi.StateError("input is closed")
            remaining = 0.05 if deadline is None else min(0.05, deadline - monotonic())
            if remaining <= 0:
                return None
            try:
                return self._queue.get(timeout=remaining)
            except Empty:
                continue

    def start_recording(
        self,
        destination: str | PathLike[str],
        *,
        label: str = "",
        best_effort: bool = False,
        payload_limit: int | str = 1_048_576,
        record_limit: int = 128,
        byte_limit: int | str = 8_388_608,
        metrics: bool = False,
    ) -> archive.Recorder:
        if self._decoder is None:
            raise midi.StateError("recording requires decoded input (raw=False)")
        _native.check_thread()
        with self._lock:
            if (
                self._closing
                or self.closed
                or self._recorder is not None
                or self._recording_starting
            ):
                raise midi.StateError("recording is already active or input is closed")
            self._recording_starting = True
        try:
            recorder = archive.Recorder(
                destination,
                clock=self.clock,
                stream=self._stream,
                label=label,
                best_effort=best_effort,
                payload_limit=payload_limit,
                record_limit=record_limit,
                byte_limit=byte_limit,
                metrics=metrics,
            )
            with self._lock:
                closed = self._closing
                if not closed:
                    self._recorder = recorder
            if closed:
                recorder.fail("input closed while recording started")
                recorder.stop()
                raise midi.StateError("input closed while recording started")
            return recorder
        finally:
            with self._lock:
                self._recording_starting = False

    def _final_loss(self) -> None:
        assert self._raw is not None
        if self._decoder is None:
            return
        lost = (
            max(0, self._raw.dropped_packets - self._known_lost)
            + self._ignored_on_close
        )
        if lost:
            self._accept(
                [
                    self._decoder.loss(
                        lost=lost, reason="known input loss at recording/close boundary"
                    )
                ]
            )
        if (
            failure := self._raw.worker_failure
        ) is not None and self._recorder is not None:
            self._recorder.fail(str(failure))
        self._ignored_on_close = 0

    def stop_recording(self, *, timeout: float | str = 5.0) -> archive.RecordingReport:
        _native.check_thread()
        with self._lock:
            self._final_loss()
            recorder = self._recorder
            self._recorder = None
        if recorder is None:
            raise midi.StateError("no recording is attached")
        if self._decoder is not None and self._decoder.pending:
            recorder.fail("recording ended with an unfinished message")
        self._recording_report = recorder.stop(timeout=timeout)
        return self._recording_report

    def close(self) -> None:
        _native.check_thread()
        if self._raw is None or self._closing and self._raw.closed:
            return
        with self._lock:
            self._closing = True
        failure: BaseException | None = None
        try:
            self._raw.close()
        except BaseException as error:
            # Preserve a transport failure if recording cleanup also fails.
            failure = error
        try:
            with self._lock:
                self._final_loss()
                if self._decoder is not None:
                    self._accept(list(self._decoder.finish()))
                recorder = self._recorder
                self._recorder = None
            if self._bridge is not None:
                self._bridge.retire()
                self._bridge.drain()
            if recorder is not None:
                self._recording_report = recorder.stop()
                if not self._recording_report.complete:
                    raise OSError(self._recording_report.failure)
        except BaseException as error:
            if failure is not None:
                raise BaseExceptionGroup(
                    "MIDI close and recording cleanup both failed", [failure, error]
                ) from None
            raise
        if failure is not None:
            raise failure

    def __enter__(self) -> InputConnection[T]:
        if self.closed:
            raise midi.StateError("input is closed")
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        close_context(self.close, exc_value)

    def __del__(self) -> None:
        if (bridge := getattr(self, "_bridge", None)) is not None:
            bridge.retire()
        if (recorder := getattr(self, "_recorder", None)) is not None:
            recorder.fail("input was abandoned during recording")


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


def input_size(item: object) -> int:
    if isinstance(item, events.ReceivedPacket):
        return (
            len(item.packet.words) * 4
            if isinstance(item.packet, midi.UmpPacket)
            else len(item.packet)
        )
    return _native.live_size(cast(events.Event | events.StreamDiagnostic, item))


INPUT_ERRORS = (
    _native.MidiError,
    _native.StateError,
    _native.NativePanicError,
    _native.ResourceError,
    _native.WorkerError,
    _native.WorkerTimeoutError,
    OSError,
)
