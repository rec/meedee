"""One ownership facade for isolated and in-process Rust transport."""

from __future__ import annotations

import inspect
import math
import sys
from _thread import LockType
from collections.abc import Callable, Generator, Iterable, Iterator
from fractions import Fraction
from functools import partial
from itertools import chain
from os import PathLike
from pathlib import Path
from queue import Empty, Full
from threading import Lock, RLock, Thread
from time import monotonic
from types import TracebackType
from typing import Annotated, Literal, Self, cast
from uuid import uuid4

from pydantic import (
    BeforeValidator,
    Field,
    PrivateAttr,
    field_validator,
    model_validator,
)

from . import _native, _units, archive, codecs, events, messages, midi
from ._errors import CompoundSendError
from ._resource import ResourceModel
from ._transport import DEFAULT_RECEIVE_BYTE_LIMIT, ReceiveQueue
from .events import Clock


class _Client(ResourceModel, frozen=True):
    encoding: midi.Encoding = Field(default_factory=lambda: midi.Encoding.Bytes)
    backend: midi.Backend | None = None
    port: str | None = Field(default=None, min_length=1)
    run_in_subprocess: bool = False
    subprocess_worker: Literal["python", "rust"] = "python"
    subprocess_executable: str | None = None
    timeout: float = Field(default=5.0, gt=0, allow_inf_nan=False)

    _settings: dict[str, object] = PrivateAttr(default_factory=dict)
    _initialization: LockType = PrivateAttr(default_factory=Lock)
    _closed: bool = PrivateAttr(default=False)
    _failure: Exception | None = PrivateAttr(default=None)

    @model_validator(mode="before")
    @classmethod
    def _control_thread(cls, value: object) -> object:
        _native.check_thread()
        return value

    @field_validator("timeout", mode="before")
    @classmethod
    def _seconds(cls, value: object) -> object:
        if type(value) not in (int, float, str):
            raise ValueError("timeout must be seconds or a unit expression")
        return _units.real(cast(float | str, value), "second")

    @field_validator("subprocess_executable", mode="before")
    @classmethod
    def _executable(cls, value: object) -> object:
        return str(Path(value)) if isinstance(value, (str, PathLike)) else value


def _builtin_integer(value: object) -> object:
    if type(value) is not int:
        raise ValueError("expected a built-in integer")
    return value


_Channel = Annotated[int, BeforeValidator(_builtin_integer), Field(ge=1, le=16)]


class Input[T = messages.Message | events.StreamDiagnostic](_Client, frozen=True):
    raw: bool = False
    channel: _Channel | list[_Channel] | None = None
    event_limit: int = Field(default=128, gt=0)
    sysex_limit: int = Field(default=1_048_576, gt=0)
    timestamps: midi.TimestampMode = Field(
        default_factory=lambda: midi.TimestampMode.Absolute
    )
    receive_byte_limit: int = Field(default=DEFAULT_RECEIVE_BYTE_LIMIT, gt=0)

    _native_client: _native.Input | None = PrivateAttr(default=None)
    _connection: _InputState[T] | None = PrivateAttr(default=None)
    _channels: frozenset[int] | None = PrivateAttr(default=None)

    @field_validator("channel", mode="before")
    @classmethod
    def _channel_values(cls, value: object) -> object:
        if value is None or isinstance(value, int):
            return value
        return list(value) if isinstance(value, Iterable) else value

    @field_validator("event_limit", mode="before")
    @classmethod
    def _events(cls, value: object) -> object:
        return _builtin_integer(value)

    @field_validator("sysex_limit", "receive_byte_limit", mode="before")
    @classmethod
    def _bytes(cls, value: object) -> object:
        return _builtin_integer(
            _units.integer(value, "byte") if isinstance(value, str) else value
        )

    @model_validator(mode="after")
    def _decoded_channels(self) -> Self:
        if self.raw and self.channel is not None:
            raise ValueError("channel filtering requires decoded input (raw=False)")
        return self

    def model_post_init(self, context: object) -> None:
        if self.channel is not None:
            self._channels = frozenset(
                [self.channel] if isinstance(self.channel, int) else self.channel
            )
        self._settings = {
            "identity": uuid4().hex,
            "encoding": self.encoding,
            "backend": self.backend,
            "timestamps": self.timestamps,
            "receive_byte_limit": self.receive_byte_limit,
        }

    def _initialize(self) -> None:
        _native.check_thread()
        with self._initialization:
            if self._closed:
                raise midi.StateError("client is closed")
            if self._native_client is not None:
                return
            settings: dict[str, object] = dict(self._settings)
            if self.run_in_subprocess:
                settings.update(
                    worker_program=self.subprocess_executable
                    or (
                        sys.executable
                        if self.subprocess_worker == "python"
                        else "middi-worker"
                    ),
                    worker_args=["-m", "middi._worker"]
                    if self.subprocess_worker == "python"
                    else [],
                    worker_timeout=self.timeout,
                )
            self._native_client = _native.Input(**settings)

    def ports(self) -> list[midi.Port]:
        if self._closed or self._connection is not None:
            raise midi.StateError("client is connected or closed")
        self._initialize()
        assert self._native_client is not None
        return self._native_client.ports()

    @property
    def worker_failure(self) -> Exception | None:
        if self._native_client is not None and self.run_in_subprocess:
            return self._native_client.worker_failure
        return self._failure

    def connect(
        self, port: midi.Port, callback: Callable[[T], object] | None = None
    ) -> Input[T]:
        self._open(port, None, callback)
        return self

    def create_virtual(
        self, name: str, callback: Callable[[T], object] | None = None
    ) -> Input[T]:
        validate_open(name, callback)
        self._open(None, name, callback)
        return self

    def _open(
        self,
        port: midi.Port | None,
        name: str | None,
        callback: Callable[[T], object] | None,
    ) -> _InputState[T]:
        if self._closed or self._connection is not None:
            raise midi.StateError("input is closed or already connected")
        validate_open(callback=callback)
        if name is None and not isinstance(port, midi.Port):
            raise TypeError("port must be a discovery snapshot")
        self._initialize()
        assert self._native_client is not None
        identity = uuid4().hex
        connection = _InputState[T]._create(self, callback)
        transport = _InputTransport[T]._create(self)
        transport._native = (
            self._native_client.connect(
                cast(midi.Port, port), connection._deliver, identity
            )
            if name is None
            else self._native_client.create_virtual(name, connection._deliver, identity)
        )
        transport._closed = False
        connection._raw = transport
        self._connection = connection
        connection._schedule_callback_close()
        return connection

    def record(
        self,
        duration: float | str,
        *,
        output: str | PathLike[str] | None = None,
        overwrite: bool = False,
    ) -> list[
        events.Clock
        | archive.Stream
        | messages.Message
        | archive.Gap
        | archive.Discontinuity
    ]:
        """Capture to memory, optionally saving the same records to an archive."""
        seconds = _units.real(duration, "second")
        if not math.isfinite(seconds) or seconds < 0:
            raise ValueError("duration must be finite and nonnegative")
        if self.raw:
            raise midi.StateError("recording requires decoded input (raw=False)")
        if self._connection is not None and self._connection._bridge is not None:
            raise midi.StateError("recording is unavailable in callback mode")
        with self as receiver:
            recorder = receiver.start()
            deadline = monotonic() + seconds
            while (remaining := deadline - monotonic()) > 0:
                try:
                    receiver.receive(timeout=min(0.05, remaining))
                except INPUT_ERRORS as error:
                    recorder.fail(str(error))
                    break
            receiver.close()
        records = recorder.records
        assert records is not None
        if output is not None:
            with Path(output).open("wb" if overwrite else "xb") as destination:
                writer = archive.Writer(destination)
                for r in records:
                    writer.write(r)
                writer.finish()
        return cast(
            list[
                events.Clock
                | archive.Stream
                | messages.Message
                | archive.Gap
                | archive.Discontinuity
            ],
            records,
        )

    def __enter__(self) -> Input[T]:
        # An early receive overflow may still have an accepted prefix to drain.
        # Entering its existing ownership scope does not reopen the transport.
        if self._closed or (self._connection is not None and self._connection._closing):
            raise midi.StateError("input is closed")
        if self._connection is None:
            self.connect(_native.select_port(self.ports(), self.port, "input"))
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        close_context(self.close, exc_value)

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
        owned = self._connection is None
        try:
            if owned:
                self.__enter__()
            assert self._connection is not None
            yield from self._connection.with_errors()
        except INPUT_ERRORS as error:
            yield error
        finally:
            if owned:
                self.close()

    @property
    def closed(self) -> bool:
        return (
            self._closed
            or self.worker_failure is not None
            or (self._connection is not None and self._connection.closed)
        )

    def close(self) -> None:
        _native.check_thread()
        with self._initialization:
            if self._closed:
                return
            self._closed = True
        try:
            if self._connection is not None:
                self._connection.close()
        finally:
            self._failure = self.worker_failure
            self._native_client = None

    def receive(
        self,
        *,
        timeout: float | str | None = None,
        cancel: events.Cancellation | None = None,
    ) -> T | None:
        if cancel is not None and cancel.cancelled:
            raise InterruptedError("MIDI receive cancelled")
        if self._connection is None:
            self.__enter__()
        assert self._connection is not None
        return self._connection.receive(timeout=timeout, cancel=cancel)

    def start(
        self,
        destination: str | PathLike[str] | None = None,
        *,
        label: str = "",
        best_effort: bool = False,
        payload_limit: int | str = 1_048_576,
        record_limit: int = 128,
        byte_limit: int | str = 8_388_608,
        metrics: bool = False,
    ) -> archive.Recorder:
        if self.raw:
            raise midi.StateError("recording requires decoded input (raw=False)")
        self.__enter__()
        assert self._connection is not None
        return self._connection.start(
            destination,
            label=label,
            best_effort=best_effort,
            payload_limit=payload_limit,
            record_limit=record_limit,
            byte_limit=byte_limit,
            metrics=metrics,
        )

    def stop(self, *, timeout: float | str = 5.0) -> archive.RecordingReport:
        if self._connection is None:
            raise midi.StateError("no recording is attached")
        return self._connection.stop(timeout=timeout)

    @property
    def stream(self) -> str:
        if self._connection is None:
            self.__enter__()
        assert self._connection is not None
        return self._connection.stream

    @property
    def clock(self) -> events.Clock:
        if self._connection is None:
            self.__enter__()
        assert self._connection is not None
        return self._connection.clock

    @property
    def dropped_packets(self) -> int:
        if self._connection is None:
            return 0
        return self._connection.dropped_packets

    @property
    def last_message_time(self) -> float | None:
        if self._connection is None:
            return None
        return self._connection.last_message_time

    @property
    def failure(self) -> BaseException | None:
        if self._connection is None:
            return None
        return self._connection.failure

    @property
    def recording_report(self) -> archive.RecordingReport | None:
        if self._connection is None:
            return None
        return self._connection.recording_report

    def __del__(self) -> None:
        try:
            if hasattr(self, "_closed"):
                self.close()
        except INPUT_ERRORS as error:
            print(error, file=sys.stderr)


class Output(_Client, frozen=True):
    """Discover MIDI destinations and open message/packet output connections."""

    channel: _Channel | None = None
    bpm: int | float | None = Field(default=None, gt=0)

    _native_client: _native.Output | None = PrivateAttr(default=None)
    _connection: _OutputState | None = PrivateAttr(default=None)
    _beat_period: Fraction | None = PrivateAttr(default=None)

    @field_validator("bpm", mode="before")
    @classmethod
    def _numeric_bpm(cls, value: object) -> object:
        if value is not None and type(value) not in (int, float):
            raise ValueError("bpm must be a number")
        if isinstance(value, (int, float)) and not math.isfinite(value):
            raise ValueError("bpm must be positive and finite")
        return value

    def model_post_init(self, context: object) -> None:
        self._beat_period = (
            None if self.bpm is None else Fraction(60) / Fraction(str(self.bpm))
        )
        self._settings = {
            "identity": uuid4().hex,
            "encoding": self.encoding,
            "backend": self.backend,
        }

    def _initialize(self) -> None:
        _native.check_thread()
        with self._initialization:
            if self._closed:
                raise midi.StateError("client is closed")
            if self._native_client is not None:
                return
            settings: dict[str, object] = dict(self._settings)
            if self.run_in_subprocess:
                settings.update(
                    worker_program=self.subprocess_executable
                    or (
                        sys.executable
                        if self.subprocess_worker == "python"
                        else "middi-worker"
                    ),
                    worker_args=["-m", "middi._worker"]
                    if self.subprocess_worker == "python"
                    else [],
                    worker_timeout=self.timeout,
                )
            self._native_client = _native.Output(**settings)

    def ports(self) -> list[midi.Port]:
        if self._closed or self._connection is not None:
            raise midi.StateError("client is connected or closed")
        self._initialize()
        assert self._native_client is not None
        return self._native_client.ports()

    @property
    def worker_failure(self) -> Exception | None:
        if self._native_client is not None and self.run_in_subprocess:
            return self._native_client.worker_failure
        return self._failure

    def connect(self, port: midi.Port) -> Output:
        _native.check_thread()
        if self._closed or self._connection is not None:
            raise midi.StateError("output is closed or already connected")
        if not isinstance(port, midi.Port):
            raise TypeError("port must be a discovery snapshot")
        self._initialize()
        connection = _OutputState._create(self)
        assert self._native_client is not None
        connection._native = self._native_client.connect(port)
        connection._closed = False
        self._connection = connection
        return self

    def create_virtual(self, name: str) -> Output:
        if self._closed or self._connection is not None:
            raise midi.StateError("output is closed or already connected")
        validate_open(name)
        self._initialize()
        connection = _OutputState._create(self)
        assert self._native_client is not None
        connection._native = self._native_client.create_virtual(name)
        connection._closed = False
        self._connection = connection
        return self

    def send(
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
        | str
        | Iterable[object],
        origin: events.Timestamp | None = None,
        mappings: list[events.ClockMapping]
        | tuple[events.ClockMapping, ...]
        | None = None,
        cancel: events.Cancellation | None = None,
        allow_rounding: bool = False,
        metrics: bool = False,
    ) -> events.PlaybackReport:
        """Send values in sequence, preserving timing within recorded sources."""
        _native.check_thread()
        if self._closed:
            raise midi.StateError("output is closed")
        try:
            timeline = self._timeline(values, origin, mappings, allow_rounding)
            if cancel is None or not cancel.cancelled:
                first = next(timeline, None)
                if first is not None:
                    if first[1] is not None:
                        self.__enter__()
                    timeline = chain([first], timeline)
            return _native.output_sequence(
                self._send,
                timeline,
                cancel=cancel,
                metrics=metrics,
            )
        finally:
            for v in values:
                if isinstance(v, archive.Reader):
                    v.close()

    def _send(self, value: object) -> None:
        self.__enter__()
        assert self._connection is not None
        self._connection._send(value)

    def _timeline(
        self,
        values: Iterable[object],
        origin: events.Timestamp | None,
        mappings: list[events.ClockMapping] | tuple[events.ClockMapping, ...] | None,
        allow_rounding: bool,
    ) -> Iterator[tuple[int, object, bool]]:
        elapsed = Fraction(0)
        pending: list[object] = []
        for v in values:
            if isinstance(
                v,
                (
                    messages.Message,
                    messages.Note,
                    messages.Rest,
                    bytes,
                    bytearray,
                    memoryview,
                    midi.UmpPacket,
                    int,
                    float,
                    str,
                ),
            ):
                pending.append(v)
                continue
            if pending:
                plan, elapsed = self._prepare_sequence(pending, elapsed)
                yield from plan
                pending.clear()
            base = elapsed
            mode: bool | None = None
            planner: events.PlaybackPlan | None = None
            ordinary: list[object] = []
            for m in cast(Iterable[object], v):
                if isinstance(m, (events.Clock, archive.Stream, archive.Metadata)):
                    continue
                timed = isinstance(m, messages.Message) and m.timestamp is not None
                if mode is None:
                    mode = timed
                elif mode != timed:
                    raise ValueError(
                        "cannot mix timestamped and untimestamped values "
                        "within one source"
                    )
                if timed:
                    assert isinstance(m, messages.Message)
                    assert m.timestamp is not None
                    if planner is None:
                        planner = events.PlaybackPlan(
                            origin=origin or m.timestamp,
                            mappings=mappings,
                            allow_rounding=allow_rounding,
                        )
                    planned = planner.deadline(m)
                    elapsed = base + Fraction(planned.delay_ns, 1_000_000_000)
                    plan, _ = self._prepare_sequence([m], elapsed)
                    for deadline, value, _ in plan:
                        if value is not None:
                            yield (
                                deadline,
                                value,
                                planned.rounding_error_ns.numerator != 0,
                            )
                else:
                    if isinstance(
                        m,
                        (
                            events.StreamDiagnostic,
                            archive.Gap,
                            archive.Discontinuity,
                            archive.Opaque,
                        ),
                    ):
                        raise ValueError(
                            "gap, discontinuity or opaque data requires "
                            "an explicit send policy"
                        )
                    ordinary.append(m)
            if ordinary:
                plan, elapsed = self._prepare_sequence(ordinary, elapsed)
                yield from plan
        if pending:
            plan, elapsed = self._prepare_sequence(pending, elapsed)
            yield from plan

    def _prepare_sequence(
        self, values: Iterable[object], elapsed: Fraction
    ) -> tuple[list[tuple[int, object, bool]], Fraction]:
        timeline: list[tuple[int, object, bool]] = []
        for v in values:
            deadline = round(elapsed * 1_000_000_000)
            rounded = (elapsed * 1_000_000_000).denominator != 1
            if isinstance(v, (messages.Note, messages.Rest, int, float, str)):
                duration = (
                    v.duration if isinstance(v, (messages.Note, messages.Rest)) else v
                )
                elapsed += _units.sequence_duration(duration, self._beat_period)
                end = round(elapsed * 1_000_000_000)
                if end > 2**64 - 1:
                    raise ValueError("delay exceeds scheduler range")
                if isinstance(v, messages.Note):
                    channel = self.channel or v.channel or 1
                    attack = messages.NoteOn(v.note, v.velocity, channel)
                    release = messages.NoteOff(v.note, channel=channel)
                    timeline.append(
                        (
                            deadline,
                            _native.prepare_output(self.encoding, attack),
                            rounded,
                        )
                    )
                    timeline.append(
                        (
                            end,
                            _native.prepare_output(self.encoding, release),
                            (elapsed * 1_000_000_000).denominator != 1,
                        )
                    )
            else:
                v = _native.resolve_output_channel(v, self.channel)
                prepared = _native.prepare_output(self.encoding, v)
                timeline.append(
                    (deadline, v if isinstance(prepared, tuple) else prepared, rounded)
                )
        timeline.append((round(elapsed * 1_000_000_000), None, False))
        return timeline, elapsed

    def __enter__(self) -> Output:
        if self._closed:
            raise midi.StateError("output is closed")
        if self._connection is None:
            self.connect(_native.select_port(self.ports(), self.port, "output"))
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        close_context(self.close, exc_value)

    @property
    def closed(self) -> bool:
        return (
            self._closed
            or self.worker_failure is not None
            or (self._connection is not None and self._connection.closed)
        )

    def close(self) -> None:
        _native.check_thread()
        with self._initialization:
            if self._closed:
                return
            self._closed = True
        try:
            if self._connection is not None:
                self._connection.close()
        finally:
            self._failure = self.worker_failure
            self._native_client = None

    def __del__(self) -> None:
        try:
            if hasattr(self, "_closed"):
                self.close()
        except INPUT_ERRORS as error:
            print(error, file=sys.stderr)


class _InputTransport[T]:
    _snapshot: tuple[Clock, int, float | None, BaseException | None] | None

    __slots__ = ("_native", "_closed", "_snapshot", "__weakref__")

    def __init__(self) -> None:
        raise TypeError("MIDI connection handles cannot be constructed directly")

    @classmethod
    def _create(cls, client: Input[T]) -> _InputTransport[T]:
        connection = object.__new__(cls)
        connection._native = cast(_native.InputConnection | None, None)
        connection._snapshot = None
        connection._closed = True
        return connection

    @property
    def closed(self) -> bool:
        return self._closed if self._native is None else self._native.closed

    @property
    def clock(self) -> Clock:
        if self._snapshot is not None:
            return self._snapshot[0]
        assert self._native is not None
        return self._native.clock

    @property
    def dropped_packets(self) -> int:
        if self._snapshot is not None:
            return self._snapshot[1]
        assert self._native is not None
        return self._native.dropped_packets

    @property
    def worker_failure(self) -> BaseException | None:
        if self._snapshot is not None:
            return self._snapshot[3]
        return None if self._native is None else self._native.worker_failure

    @property
    def last_message_time(self) -> float | None:
        if self._snapshot is not None:
            return self._snapshot[2]
        return None if self._native is None else self._native.last_message_time

    def close(self) -> None:
        _native.check_thread()
        if self._native is not None:
            try:
                self._native.close()
            finally:
                self._snapshot = (
                    self._native.clock,
                    self._native.dropped_packets,
                    self._native.last_message_time,
                    self._native.worker_failure,
                )
                self._closed = True
                self._native = None


class _OutputState:
    """An open destination for messages, encoded packets and timed playback."""

    __slots__ = (
        "_native",
        "_closed",
        "_state",
        "_encoding",
        "_channel",
        "_beat_period",
        "_failure",
        "_run_in_subprocess",
        "__weakref__",
    )

    def __init__(self) -> None:
        raise TypeError("MIDI connection handles cannot be constructed directly")

    @classmethod
    def _create(cls, client: Output) -> _OutputState:
        connection = object.__new__(cls)
        connection._failure = None
        connection._run_in_subprocess = client.run_in_subprocess
        connection._state = _native.OutputState()
        connection._encoding = client.encoding
        connection._channel = client.channel
        connection._beat_period = client._beat_period
        connection._native = cast(_native.OutputConnection | None, None)
        connection._closed = True
        return connection

    def _send(self, value: object) -> None:
        _native.check_thread()
        prepared = _native.prepare_output(self._encoding, value)
        assert self._native is not None
        if isinstance(prepared, tuple):
            total = prepared[2]
            packets = [
                codecs.encode(m)
                for m in cast(
                    Callable[[object], list[messages.Message]], codecs.expand
                )(value)
            ]
            try:
                self._native.send_command(value)
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
        else:
            self._native.send(prepared)
        if isinstance(prepared, bytes):
            self._state.feed(prepared)

    @property
    def closed(self) -> bool:
        return self._closed or (self._native is not None and self._native.closed)

    @property
    def worker_failure(self) -> BaseException | None:
        if self._native is not None and self._run_in_subprocess:
            return self._native.worker_failure
        return self._failure

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
            try:
                self._native.close()
            finally:
                self._failure = self.worker_failure
                self._closed = True
                self._native = None

    def __enter__(self) -> _OutputState:
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


class _InputState[T = messages.Message | events.StreamDiagnostic]:
    _raw: _InputTransport[T] | None
    _channels: frozenset[int] | None
    _stream: str
    _decoder: events.PacketDecoder | None
    _bridge: _native.CallbackBridge | None
    _queue: ReceiveQueue[T]
    _lock: LockType
    _closing: bool
    _failure: BaseException | None
    _recorder: archive.Recorder | None
    _recording_in_memory: bool
    _recording_clocks: set[str]
    _recording_starting: bool
    _known_lost: int
    _ignored_on_close: int
    _recording_report: archive.RecordingReport | None
    _recording_stopping: bool
    _last_message_time: float | None
    _callback_cleanup_started: bool
    _close_lock: RLock

    def __init__(self) -> None:
        raise TypeError("connection handles are created by Input")

    @classmethod
    def _create(
        cls,
        client: Input[T],
        callback: Callable[[T], object] | None,
    ) -> _InputState[T]:
        connection = object.__new__(cls)
        connection._raw = None
        connection._channels = client._channels
        connection._stream = uuid4().hex
        connection._decoder = (
            None
            if client.raw
            else events.PacketDecoder(
                stream=connection._stream,
                encoding=client.encoding,
                sysex_limit=client.sysex_limit,
            )
        )
        connection._bridge = (
            None if callback is None else _native.CallbackBridge(callback)
        )
        connection._queue = ReceiveQueue[T](
            client.receive_byte_limit, capacity=client.event_limit, size_of=input_size
        )
        connection._lock = Lock()
        connection._closing = False
        connection._failure = None
        connection._recorder = None
        connection._recording_in_memory = False
        connection._recording_clocks = set()
        connection._recording_starting = False
        connection._known_lost = 0
        connection._ignored_on_close = 0
        connection._recording_report = None
        connection._recording_stopping = False
        connection._last_message_time = None
        connection._callback_cleanup_started = False
        connection._close_lock = RLock()
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
        return (
            self._closing
            or self.failure is not None
            or self._raw is None
            or self._raw.closed
        )

    @property
    def dropped_packets(self) -> int:
        """Observed raw receive drops, including drops followed by silence."""
        return 0 if self._raw is None else self._raw.dropped_packets

    @property
    def failure(self) -> BaseException | None:
        if self._failure is not None:
            return self._failure
        if self._bridge is not None and (failure := self._bridge.failure) is not None:
            return failure
        return None if self._raw is None else self._raw.worker_failure

    @property
    def worker_failure(self) -> BaseException | None:
        return None if self._raw is None else self._raw.worker_failure

    @property
    def last_message_time(self) -> float | None:
        return self._last_message_time

    @property
    def recording_report(self) -> archive.RecordingReport | None:
        return self._recording_report

    def _accept(
        self,
        items: list[messages.Message | events.StreamDiagnostic | events.ReceivedPacket],
    ) -> None:
        for item in items:
            if isinstance(item, events.StreamDiagnostic) and item.kind == "loss":
                self._known_lost += item.lost
            if (
                self._recorder is not None
                and not self._recording_stopping
                and not isinstance(item, events.ReceivedPacket)
            ):
                record_item(self._recorder, item, self._recording_clocks)
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
            self._last_message_time = monotonic()
            items: list[
                messages.Message | events.StreamDiagnostic | events.ReceivedPacket
            ] = [packet] if self._decoder is None else list(self._decoder.feed(packet))
            if self._channels is not None:
                items = [
                    i
                    for i in items
                    if not isinstance(i, messages.Message)
                    or getattr(i, "channel", None) is None
                    or getattr(i, "channel", None) in self._channels
                ]
            self._accept(items)
        if self._bridge is not None:
            for item in items:
                if isinstance(item, events.ReceivedPacket):
                    self._bridge.deliver(item)
                else:
                    self._bridge.deliver_typed(item)
                if (failure := self._bridge.failure) is not None:
                    with self._lock:
                        self._failure = failure
                        self._closing = True
                        if self._recorder is not None:
                            self._recorder.fail(str(failure))
                    self._schedule_callback_close()
                    break

    def _schedule_callback_close(self) -> None:
        with self._lock:
            if (
                self._raw is None
                or self._bridge is None
                or self._bridge.failure is None
                or self._callback_cleanup_started
            ):
                return
            self._callback_cleanup_started = True
        Thread(
            target=self._close_after_callback,
            name="middi callback cleanup",
            daemon=True,
        ).start()

    def _close_after_callback(self) -> None:
        try:
            self.close()
        except INPUT_ERRORS as error:
            print(error, file=sys.stderr)

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
            if isinstance(item, messages.Message):
                yield item
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

    def start(
        self,
        destination: str | PathLike[str] | None = None,
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
                or (destination is not None and self.closed)
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
                    self._recording_clocks = {self.clock.identity}
                    self._recording_in_memory = destination is None
                    if destination is None:
                        # Memory capture includes events buffered before attachment.
                        with self._queue.mutex:
                            buffered = list(self._queue.queue)
                        for item in buffered:
                            record_item(recorder, item, self._recording_clocks)
                        if (failure := self.failure) is not None:
                            recorder.fail(str(failure))
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

    def stop(self, *, timeout: float | str = 5.0) -> archive.RecordingReport:
        _native.check_thread()
        timeout = _units.real(timeout, "second")
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be positive and finite")
        with self._lock:
            recorder = self._recorder
            if recorder is None:
                raise midi.StateError("no recording is attached")
            if not self._recording_stopping:
                self._final_loss()
                if self._decoder is not None and self._decoder.pending:
                    recorder.fail("recording ended with an unfinished message")
                self._recording_stopping = True
        return self._finish_recording(recorder, timeout)

    def _finish_recording(
        self, recorder: archive.Recorder, timeout: float | str = 5.0
    ) -> archive.RecordingReport:
        report = recorder.stop(timeout=timeout)
        with self._lock:
            self._recording_report = report
            if self._recorder is recorder:
                self._recorder = None
                self._recording_stopping = False
        return report

    def close(self) -> None:
        _native.check_thread()
        with self._close_lock:
            if self._raw is None or self._closing and self._raw.closed:
                return
            with self._lock:
                self._closing = True
            self._failure = self.failure
            failure: BaseException | None = None
            try:
                self._raw.close()
            except BaseException as error:
                # Preserve a transport failure if recording cleanup also fails.
                failure = error
                if isinstance(error, Exception) and self._failure is None:
                    self._failure = error
            try:
                with self._lock:
                    self._final_loss()
                    if self._decoder is not None:
                        self._accept(list(self._decoder.finish()))
                    recorder = self._recorder
                    if recorder is not None:
                        self._recording_stopping = True
                if self._bridge is not None:
                    self._bridge.retire()
                    self._bridge.drain()
                if recorder is not None:
                    self._finish_recording(recorder)
                    assert self._recording_report is not None
                    if (
                        not self._recording_report.complete
                        and not self._recording_in_memory
                    ):
                        raise OSError(self._recording_report.failure)
            except BaseException as error:
                if isinstance(error, Exception) and self._failure is None:
                    self._failure = error
                if failure is not None:
                    raise BaseExceptionGroup(
                        "MIDI close and recording cleanup both failed", [failure, error]
                    ) from None
                raise
            if failure is not None:
                raise failure

    def __enter__(self) -> _InputState[T]:
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


def record_item(recorder: archive.Recorder, item: object, clocks: set[str]) -> None:
    if isinstance(item, messages.Message):
        if item.timestamp is not None and item.timestamp.clock.identity not in clocks:
            recorder.record(item.timestamp.clock)
            clocks.add(item.timestamp.clock.identity)
        recorder.record(item)
    elif isinstance(item, events.StreamDiagnostic):
        if item.kind == "clock_change":
            recorder.record(
                archive.Discontinuity(stream=item.stream, reason=item.reason)
            )
        else:
            recorder.record(
                archive.Gap(stream=item.stream, lost=item.lost, reason=item.reason)
            )


def close_context(close: Callable[[], None], body_error: BaseException | None) -> None:
    try:
        close()
    except BaseException as close_error:
        if body_error is None:
            raise
        raise BaseExceptionGroup(
            "MIDI context body and close both failed", [body_error, close_error]
        ) from None


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
    return _native.live_size(cast(messages.Message | events.StreamDiagnostic, item))


INPUT_ERRORS = (
    _native.MidiError,
    _native.StateError,
    _native.NativePanicError,
    _native.ResourceError,
    _native.WorkerError,
    _native.WorkerTimeoutError,
    OSError,
)
