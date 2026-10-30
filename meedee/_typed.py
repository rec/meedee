"""Typed input adapters over the sole transport implementation."""

from __future__ import annotations

import math
from _thread import LockType
from collections.abc import Callable
from os import PathLike
from queue import Empty, Full
from threading import Lock
from time import monotonic
from types import TracebackType
from uuid import uuid4

from . import _native, archive, events, midi
from ._clients import (
    Input,
    InputConnection,
    close_context,
    validate_open,
)
from ._transport import ReceiveQueue


class TypedInput:
    def __init__(
        self,
        *,
        protocol: midi.Protocol = midi.Protocol.Midi1,
        backend: midi.Backend | None = None,
        timestamps: midi.TimestampMode = midi.TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float = 5.0,
        receive_byte_limit: int = 8_388_608,
        event_limit: int = 128,
        sysex_limit: int = 1_048_576,
    ) -> None:
        for n in (event_limit, sysex_limit):
            if type(n) is not int:
                raise TypeError("event_limit and sysex_limit must be built-in integers")
            if n <= 0:
                raise ValueError(
                    "event_limit and sysex_limit must be positive built-in integers"
                )
        self._protocol = protocol
        self._byte_limit = receive_byte_limit
        self._event_limit = event_limit
        self._sysex_limit = sysex_limit
        self._client = Input(
            protocol=protocol,
            backend=backend,
            timestamps=timestamps,
            isolated=isolated,
            timeout=timeout,
            receive_byte_limit=receive_byte_limit,
        )

    def ports(self) -> list[midi.Port]:
        return self._client.ports()

    def connect(
        self,
        port: midi.Port,
        callback: Callable[[events.Event | events.StreamDiagnostic], object]
        | None = None,
    ) -> TypedInputConnection:
        return self._open(port, None, callback)

    def create_virtual(
        self,
        name: str,
        callback: Callable[[events.Event | events.StreamDiagnostic], object]
        | None = None,
    ) -> TypedInputConnection:
        if not isinstance(name, str):
            raise TypeError("virtual port name must be a string")
        return self._open(None, name, callback)

    def _open(
        self,
        port: midi.Port | None,
        name: str | None,
        callback: Callable[[events.Event | events.StreamDiagnostic], object] | None,
    ) -> TypedInputConnection:
        if callback is not None:
            validate_open(callback=callback)
        connection = TypedInputConnection._create(self, callback)
        if name is None:
            if port is None:
                raise TypeError("port must be a discovery snapshot")
            connection._raw = self._client.connect(port, connection._deliver)
        else:
            connection._raw = self._client.create_virtual(name, connection._deliver)
        return connection


class TypedInputConnection:
    _raw: InputConnection | None
    _stream: str
    _decoder: events.PacketDecoder
    _bridge: _native.CallbackBridge | None
    _queue: ReceiveQueue[events.Event | events.StreamDiagnostic]
    _lock: LockType
    _closing: bool
    _failure: Exception | None
    _recorder: archive.Recorder | None
    _recording_starting: bool
    _known_lost: int
    _ignored_on_close: int
    _recording_report: archive.RecordingReport | None

    def __init__(self) -> None:
        raise TypeError("connection handles are created by TypedInput")

    @classmethod
    def _create(
        cls,
        client: TypedInput,
        callback: Callable[[events.Event | events.StreamDiagnostic], object] | None,
    ) -> TypedInputConnection:
        connection = object.__new__(cls)
        connection._raw = None
        connection._stream = uuid4().hex
        connection._decoder = events.PacketDecoder(
            stream=connection._stream,
            protocol=client._protocol,
            sysex_limit=client._sysex_limit,
        )
        connection._bridge = (
            None if callback is None else _native.CallbackBridge(callback)
        )
        connection._queue = ReceiveQueue[events.Event | events.StreamDiagnostic](
            client._byte_limit, capacity=client._event_limit, size_of=_native.live_size
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
    def recording_report(self) -> archive.RecordingReport | None:
        return self._recording_report

    def _accept(self, items: list[events.Event | events.StreamDiagnostic]) -> None:
        for item in items:
            if isinstance(item, events.StreamDiagnostic) and item.kind == "loss":
                self._known_lost += item.lost
            if self._recorder is not None:
                self._recorder.record(item)
            if self._bridge is None and self._failure is None:
                try:
                    self._queue.put_nowait(item)
                except Full:
                    self._failure = midi.ResourceError(
                        "typed receive queue overflowed; "
                        "recording has independent admission"
                    )

    def _deliver(self, packet: events.ReceivedPacket) -> None:
        with self._lock:
            if self._closing:
                self._ignored_on_close += 1
                return
            items = self._decoder.feed(packet)
            self._accept(items)
        if self._bridge is not None:
            for item in items:
                self._bridge.deliver_typed(item)

    def receive(
        self, *, timeout: float | None = None, cancel: events.Cancellation | None = None
    ) -> events.Event | events.StreamDiagnostic | None:
        _native.check_thread()
        if self._bridge is not None:
            raise midi.StateError("receive is unavailable in callback mode")
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
                raise midi.StateError("typed input is closed")
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
        payload_limit: int = 1_048_576,
        record_limit: int = 128,
        byte_limit: int = 8_388_608,
        metrics: bool = False,
    ) -> archive.Recorder:
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

    def stop_recording(self, *, timeout: float = 5.0) -> archive.RecordingReport:
        _native.check_thread()
        with self._lock:
            self._final_loss()
            recorder = self._recorder
            self._recorder = None
        if recorder is None:
            raise midi.StateError("no recording is attached")
        if self._decoder.pending:
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
                self._accept(self._decoder.finish())
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

    def __enter__(self) -> TypedInputConnection:
        if self.closed:
            raise midi.StateError("typed input is closed")
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
