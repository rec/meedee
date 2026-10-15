"""One ownership facade for isolated and in-process Rust transport."""

from __future__ import annotations

import inspect
import math
from collections.abc import Callable
from functools import partial
from types import TracebackType
from typing import cast
from uuid import uuid4

from . import _native, gm, messages, midi, mpe
from ._errors import CompoundSendError
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
        timeout: float = 5.0,
        receive_byte_limit: int = DEFAULT_RECEIVE_BYTE_LIMIT,
    ) -> None:
        validate_settings(protocol, backend, isolated, timeout)
        if not isinstance(timestamps, midi.TimestampMode):
            raise TypeError("timestamps must be TimestampMode")
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
    __slots__ = ("_native", "_context", "_protocol")

    def __init__(
        self,
        *,
        protocol: midi.Protocol = midi.Protocol.Midi1,
        backend: midi.Backend | None = None,
        isolated: bool = True,
        timeout: float = 5.0,
    ) -> None:
        validate_settings(protocol, backend, isolated, timeout)
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
    __slots__ = ("_client", "_native", "_closed", "_generation", "__weakref__")

    def __init__(self) -> None:
        raise TypeError("MIDI connection handles cannot be constructed directly")

    @classmethod
    def _create(cls, client: Output) -> OutputConnection:
        connection = object.__new__(cls)
        connection._client = client
        connection._native = cast(_native.OutputConnection | None, None)
        connection._closed = True
        connection._generation = (
            0 if client._context is None else client._context.generation
        )
        return connection

    def send(self, packet: bytes | bytearray | memoryview | midi.UmpPacket) -> None:
        _native.check_thread()
        packet = _native.prepare_send(self._client._protocol, packet)
        if self._native is not None:
            self._native.send(packet)
        else:
            if self.closed:
                raise _native.StateError("MIDI connection is closed or failed")
            assert self._client._context is not None
            self._client._context.request(
                "send",
                [
                    packet.words if isinstance(packet, midi.UmpPacket) else packet,
                    self._generation,
                ],
                serial=True,
            )

    def send_command(
        self,
        command: messages.ControlChange14
        | messages.RpnSet
        | messages.NrpnSet
        | messages.RpnIncrement
        | messages.RpnDecrement
        | messages.NrpnIncrement
        | messages.NrpnDecrement
        | gm.BankProgramSelect
        | mpe.ZoneConfiguration,
    ) -> None:
        _native.check_thread()
        wire = _native.prepare_command(self._client._protocol, command)
        total = wire[2]
        try:
            if self._native is not None:
                self._native.send_command(command)
            else:
                if self.closed:
                    raise _native.StateError("MIDI connection is closed or failed")
                assert self._client._context is not None
                self._client._context.request(
                    "send_command", [wire, self._generation], serial=True
                )
        except (
            _native.WorkerError,
            _native.WorkerTimeoutError,
            _native.NativePanicError,
        ) as error:
            raise CompoundSendError(str(error), None, total) from error

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
