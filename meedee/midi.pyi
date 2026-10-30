"""Raw byte/UMP transport backed by the shared Rust toolkit."""

from collections.abc import Callable, Iterable
from os import PathLike
from types import TracebackType
from typing import ClassVar

from . import archive, events, gm, messages, mpe

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

class Input:
    def __init__(
        self,
        *,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        timestamps: TimestampMode = TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float = 5.0,
        receive_byte_limit: int = 8_388_608,
    ) -> None: ...
    def ports(self) -> list[Port]: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    def connect(
        self, port: Port, callback: Callable[[events.ReceivedPacket], object]
    ) -> InputConnection: ...
    def create_virtual(
        self, name: str, callback: Callable[[events.ReceivedPacket], object]
    ) -> InputConnection: ...

class Output:
    def __init__(
        self,
        *,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        isolated: bool = True,
        timeout: float = 5.0,
    ) -> None: ...
    def ports(self) -> list[Port]: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    def connect(self, port: Port) -> OutputConnection: ...
    def create_virtual(self, name: str) -> OutputConnection: ...

class InputConnection:
    @property
    def clock(self) -> events.Clock: ...
    @property
    def dropped_packets(self) -> int: ...
    @property
    def last_message_time(self) -> float | None: ...
    @property
    def worker_failure(self) -> Exception | None: ...
    @property
    def closed(self) -> bool: ...
    def close(self) -> None: ...
    def __enter__(self) -> InputConnection: ...
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
        origin: events.Timestamp,
        mappings: list[events.ClockMapping]
        | tuple[events.ClockMapping, ...]
        | None = None,
        cancel: events.Cancellation | None = None,
        allow_rounding: bool = False,
        metrics: bool = False,
    ) -> events.PlaybackReport: ...
    def send_message(self, message: messages.Message) -> None: ...
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
    ) -> None: ...
    def send_packet(
        self, packet: bytes | bytearray | memoryview | UmpPacket
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

class TypedInput:
    def __init__(
        self,
        *,
        protocol: Protocol = Protocol.Midi1,
        backend: Backend | None = None,
        timestamps: TimestampMode = TimestampMode.Absolute,
        isolated: bool = True,
        timeout: float = 5.0,
        receive_byte_limit: int = 8_388_608,
        event_limit: int = 128,
        sysex_limit: int = 1_048_576,
    ) -> None: ...
    def ports(self) -> list[Port]: ...
    def connect(
        self,
        port: Port,
        callback: Callable[[events.Event | events.StreamDiagnostic], object]
        | None = None,
    ) -> TypedInputConnection: ...
    def create_virtual(
        self,
        name: str,
        callback: Callable[[events.Event | events.StreamDiagnostic], object]
        | None = None,
    ) -> TypedInputConnection: ...

class TypedInputConnection:
    @property
    def stream(self) -> str: ...
    @property
    def clock(self) -> events.Clock: ...
    @property
    def closed(self) -> bool: ...
    @property
    def dropped_packets(self) -> int: ...
    @property
    def failure(self) -> Exception | None: ...
    @property
    def recording_report(self) -> archive.RecordingReport | None: ...
    def receive(
        self, *, timeout: float | None = None, cancel: events.Cancellation | None = None
    ) -> events.Event | events.StreamDiagnostic | None: ...
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
    ) -> archive.Recorder: ...
    def stop_recording(self, *, timeout: float = 5.0) -> archive.RecordingReport: ...
    def close(self) -> None: ...
    def __enter__(self) -> TypedInputConnection: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
