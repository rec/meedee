"""Decoded or raw MIDI input and message output over the shared Rust transport."""

from collections.abc import Callable, Generator, Iterable, Iterator
from os import PathLike
from types import TracebackType
from typing import ClassVar, Literal, overload

from . import archive, events, messages

__version__: str

class Protocol:
    Midi1: ClassVar[Protocol]
    Ump: ClassVar[Protocol]

class Backend:
    CoreMidi: ClassVar[Backend]
    CoreMidiUmp: ClassVar[Backend]
    AlsaSeq: ClassVar[Backend]
    AlsaSeqUmp: ClassVar[Backend]
    AlsaRaw: ClassVar[Backend]
    AlsaRawUmp: ClassVar[Backend]
    WinMm: ClassVar[Backend]
    WindowsMidiServices: ClassVar[Backend]

class TimestampMode:
    Absolute: ClassVar[TimestampMode]
    Off: ClassVar[TimestampMode]

class UmpPacket:
    def __init__(self, *, words: list[int] | tuple[int, ...]) -> None: ...
    @property
    def words(self) -> tuple[int, ...]: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

def compiled_backends() -> list[Backend]: ...

class MidiError(Exception): ...
class CapabilityError(MidiError): ...
class InitError(MidiError): ...
class PortInfoError(MidiError): ...
class ConnectError(MidiError): ...
class SendError(MidiError): ...

class CompoundSendError(SendError):
    def __init__(
        self, message: str, completed_messages: int | None, total_messages: int
    ) -> None: ...
    @property
    def completed_messages(self) -> int | None: ...
    @property
    def total_messages(self) -> int: ...

class StateError(RuntimeError): ...
class CallbackThreadError(RuntimeError): ...
class NativePanicError(RuntimeError): ...
class ResourceError(RuntimeError): ...
class WorkerError(RuntimeError): ...
class WorkerTimeoutError(TimeoutError): ...

class Port:
    @property
    def name(self) -> str: ...
    @property
    def backend(self) -> Backend: ...
    @property
    def backend_id(self) -> int: ...
    @property
    def _wire(self) -> tuple[str, int, int, str, str, int, bool]: ...
    @staticmethod
    def _from_wire(
        owner: str,
        snapshot: int,
        index: int,
        name: str,
        backend: Backend,
        backend_id: int,
        input: bool,
    ) -> Port: ...
    def __eq__(self, other: object) -> bool: ...
    __hash__: ClassVar[None]

class Input[T = messages.Message | events.StreamDiagnostic]:
    @overload
    def __init__(
        self: Input[events.ReceivedPacket],
        *,
        raw: Literal[True],
        channel: int | Iterable[int] | None = None,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        timestamps: TimestampMode = TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float | str = 5.0,
        receive_byte_limit: int | str = 8_388_608,
        event_limit: int = 128,
        sysex_limit: int | str = 1_048_576,
    ) -> None: ...
    @overload
    def __init__(
        self: Input[messages.Message | events.StreamDiagnostic],
        *,
        raw: Literal[False] = False,
        channel: int | Iterable[int] | None = None,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        timestamps: TimestampMode = TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float | str = 5.0,
        receive_byte_limit: int | str = 8_388_608,
        event_limit: int = 128,
        sysex_limit: int | str = 1_048_576,
    ) -> None: ...
    @overload
    def __init__(
        self: Input[messages.Message | events.StreamDiagnostic | events.ReceivedPacket],
        *,
        raw: bool,
        channel: int | Iterable[int] | None = None,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        timestamps: TimestampMode = TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float | str = 5.0,
        receive_byte_limit: int | str = 8_388_608,
        event_limit: int = 128,
        sysex_limit: int | str = 1_048_576,
    ) -> None: ...
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
    ]: ...
    def ports(self) -> list[Port]: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    def connect(
        self, port: Port, callback: Callable[[T], object] | None = None
    ) -> InputConnection[T]: ...
    def create_virtual(
        self, name: str, callback: Callable[[T], object] | None = None
    ) -> InputConnection[T]: ...
    def __enter__(self) -> InputConnection[T]: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
    @overload
    def __iter__(
        self: Input[events.ReceivedPacket],
    ) -> Iterator[events.ReceivedPacket]: ...
    @overload
    def __iter__(
        self: Input[messages.Message | events.StreamDiagnostic],
    ) -> Iterator[messages.Message]: ...
    @overload
    def __iter__(self) -> Iterator[messages.Message | events.ReceivedPacket]: ...
    def with_errors(
        self,
    ) -> Generator[
        messages.Message | events.ReceivedPacket | events.StreamDiagnostic | Exception,
        None,
        None,
    ]: ...

class Output:
    def __init__(
        self,
        *,
        channel: int | None = None,
        bpm: int | float | None = None,
        port: str | None = None,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        isolated: bool = False,
        timeout: float | str = 5.0,
    ) -> None: ...
    def play(self, source: Iterable[object]) -> events.PlaybackReport: ...
    def ports(self) -> list[Port]: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    def connect(self, port: Port) -> OutputConnection: ...
    def create_virtual(self, name: str) -> OutputConnection: ...
    def __enter__(self) -> OutputConnection: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...

class OutputConnection:
    def play(
        self,
        source: Iterable[object],
        *,
        origin: events.Timestamp | None = None,
        mappings: list[events.ClockMapping]
        | tuple[events.ClockMapping, ...]
        | None = None,
        cancel: events.Cancellation | None = None,
        allow_rounding: bool = False,
        metrics: bool = False,
    ) -> events.PlaybackReport: ...
    def send(
        self,
        *values: messages.Note
        | messages.Rest
        | messages.Message
        | bytes
        | bytearray
        | memoryview
        | UmpPacket
        | int
        | float
        | str,
    ) -> None: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    @property
    def closed(self) -> bool: ...
    def close(self) -> None: ...
    def __enter__(self) -> OutputConnection: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...

class InputConnection[T = messages.Message | events.StreamDiagnostic]:
    @overload
    def __iter__(
        self: InputConnection[events.ReceivedPacket],
    ) -> Iterator[events.ReceivedPacket]: ...
    @overload
    def __iter__(
        self: InputConnection[messages.Message | events.StreamDiagnostic],
    ) -> Iterator[messages.Message]: ...
    @overload
    def __iter__(self) -> Iterator[messages.Message | events.ReceivedPacket]: ...
    def with_errors(
        self,
    ) -> Generator[
        messages.Message | events.ReceivedPacket | events.StreamDiagnostic | Exception,
        None,
        None,
    ]: ...
    @property
    def stream(self) -> str: ...
    @property
    def clock(self) -> events.Clock: ...
    @property
    def closed(self) -> bool: ...
    @property
    def dropped_packets(self) -> int: ...
    @property
    def last_message_time(self) -> float | None: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    @property
    def failure(self) -> Exception | None: ...
    @property
    def recording_report(self) -> archive.RecordingReport | None: ...
    def receive(
        self,
        *,
        timeout: float | str | None = None,
        cancel: events.Cancellation | None = None,
    ) -> T | None: ...
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
    ) -> archive.Recorder: ...
    def stop_recording(
        self, *, timeout: float | str = 5.0
    ) -> archive.RecordingReport: ...
    def close(self) -> None: ...
    def __enter__(self) -> InputConnection[T]: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
