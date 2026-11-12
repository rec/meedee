"""Immutable values backed by the shared Rust MIDI toolkit."""

from typing import Literal, Self

from . import events

class Note:
    """An attack and release separated by a duration, resolved during playback."""
    def __init__(
        self,
        duration: int | float | str,
        note: int,
        velocity: int = 90,
        channel: int | None = None,
    ) -> None: ...
    @property
    def duration(self) -> int | float | str: ...
    @property
    def note(self) -> int: ...
    @property
    def velocity(self) -> int: ...
    @property
    def channel(self) -> int | None: ...

class Rest:
    """Advance playback by a duration without sending a message."""
    def __init__(self, duration: int | float | str) -> None: ...
    @property
    def duration(self) -> int | float | str: ...

class Message:
    """Abstract native base. Direct construction is rejected."""
    def transpose(self, semitones: int) -> Self:
        """Shift note addresses with clamping; otherwise return self."""
        ...
    @property
    def timestamp(self) -> events.Timestamp | None: ...
    @property
    def stream(self) -> str | None: ...
    @property
    def span(self) -> events.SourceSpan | None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Self: ...

class NoteOn(Message):
    def __init__(
        self,
        note: int,
        velocity: int | None = None,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
        attribute_type: int | None = None,
        attribute_data: int | None = None,
    ) -> None: ...
    def transpose(self, semitones: int) -> NoteOn: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def attribute_type(self) -> int | None: ...
    @property
    def attribute_data(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def note(self) -> int: ...
    @property
    def velocity(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        note: int = ...,
        velocity: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        attribute_type: int | None = ...,
        attribute_data: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NoteOn: ...

class SysEx(Message):
    def __init__(
        self, data: bytes | bytearray | memoryview, *, group: int | None = None
    ) -> None: ...
    @property
    def group(self) -> int | None: ...
    @property
    def data(self) -> bytes: ...
    def replace(
        self,
        data: bytes | bytearray | memoryview = ...,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SysEx: ...

class NoteOff(Message):
    def __init__(
        self,
        note: int,
        velocity: int | None = None,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
        attribute_type: int | None = None,
        attribute_data: int | None = None,
    ) -> None: ...
    def transpose(self, semitones: int) -> NoteOff: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def attribute_type(self) -> int | None: ...
    @property
    def attribute_data(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def note(self) -> int: ...
    @property
    def velocity(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        note: int = ...,
        velocity: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        attribute_type: int | None = ...,
        attribute_data: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NoteOff: ...

class PolyPressure(Message):
    def __init__(
        self,
        note: int,
        pressure: int | None = None,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
    ) -> None: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def note(self) -> int: ...
    @property
    def pressure(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        note: int = ...,
        pressure: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> PolyPressure: ...

class ControlChange(Message):
    def __init__(
        self,
        controller: int,
        value: int,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
        resolution: Literal[7, 14, 32] | None = None,
    ) -> None: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def controller(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def resolution(self) -> Literal[7, 14, 32]: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        controller: int = ...,
        value: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        resolution: Literal[7, 14, 32] = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ControlChange: ...

class ProgramChange(Message):
    def __init__(
        self,
        program: int,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
        bank_msb: int | None = None,
        bank_lsb: int | None = None,
    ) -> None: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def bank_msb(self) -> int | None: ...
    @property
    def bank_lsb(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def program(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        program: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        bank_msb: int | None = ...,
        bank_lsb: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ProgramChange: ...

class ChannelPressure(Message):
    def __init__(
        self,
        pressure: int | None = None,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
    ) -> None: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def pressure(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        pressure: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ChannelPressure: ...

class PitchBend(Message):
    def __init__(
        self,
        value: int | None = None,
        channel: int | None = None,
        *,
        version: Literal[1, 2] = 1,
        group: int | None = None,
    ) -> None: ...
    @property
    def version(self) -> Literal[1, 2]: ...
    @property
    def group(self) -> int | None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        value: int = ...,
        version: Literal[1, 2] = ...,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> PitchBend: ...

class QuarterFrame(Message):
    def __init__(
        self, message_type: int, value: int, *, group: int | None = None
    ) -> None: ...
    @property
    def group(self) -> int | None: ...
    @property
    def message_type(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        message_type: int = ...,
        value: int = ...,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> QuarterFrame: ...

class SongPosition(Message):
    def __init__(self, position: int, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    @property
    def position(self) -> int: ...
    def replace(
        self,
        position: int = ...,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SongPosition: ...

class SongSelect(Message):
    def __init__(self, song: int, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    @property
    def song(self) -> int: ...
    def replace(
        self,
        song: int = ...,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SongSelect: ...

class TuneRequest(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> TuneRequest: ...

class TimingClock(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> TimingClock: ...

class Start(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Start: ...

class Continue(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Continue: ...

class Stop(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Stop: ...

class ActiveSensing(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ActiveSensing: ...

class SystemReset(Message):
    def __init__(self, *, group: int | None = None) -> None: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SystemReset: ...

class ParameterSet(Message):
    def __init__(
        self,
        parameter: int,
        value: int,
        resolution: int = 14,
        channel: int | None = None,
        *,
        family: Literal["rpn", "nrpn"] = "rpn",
        group: int | None = None,
    ) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def resolution(self) -> int: ...
    @property
    def family(self) -> Literal["rpn", "nrpn"]: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        value: int = ...,
        resolution: int = ...,
        family: Literal["rpn", "nrpn"] = ...,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ParameterSet: ...

class ParameterStep(Message):
    def __init__(
        self,
        parameter: int,
        direction: Literal[-1, 1] = 1,
        channel: int | None = None,
        *,
        family: Literal["rpn", "nrpn"] = "rpn",
        group: int | None = None,
    ) -> None: ...
    @property
    def parameter(self) -> int: ...
    @property
    def direction(self) -> Literal[-1, 1]: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def family(self) -> Literal["rpn", "nrpn"]: ...
    @property
    def group(self) -> int | None: ...
    def replace(
        self,
        *,
        parameter: int = ...,
        direction: Literal[-1, 1] = ...,
        channel: int | None = ...,
        family: Literal["rpn", "nrpn"] = ...,
        group: int | None = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ParameterStep: ...

class PerNoteController(Message):
    @property
    def version(self) -> Literal[2]: ...
    def __init__(
        self,
        *,
        group: int,
        channel: int,
        note: int,
        controller: int,
        value: int,
        family: Literal["registered", "assignable"] = "registered",
    ) -> None: ...
    @property
    def group(self) -> int: ...
    @property
    def channel(self) -> int: ...
    @property
    def note(self) -> int: ...
    @property
    def controller(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def family(self) -> Literal["registered", "assignable"]: ...
    def replace(self, **changes: object) -> PerNoteController: ...

class PerNoteManagement(Message):
    @property
    def version(self) -> Literal[2]: ...
    def __init__(self, *, group: int, channel: int, note: int, flags: int) -> None: ...
    @property
    def group(self) -> int: ...
    @property
    def channel(self) -> int: ...
    @property
    def note(self) -> int: ...
    @property
    def flags(self) -> int: ...
    def replace(self, **changes: object) -> PerNoteManagement: ...

class ParameterController(Message):
    @property
    def version(self) -> Literal[2]: ...
    def __init__(
        self,
        *,
        group: int,
        channel: int,
        bank: int,
        index: int,
        value: int,
        family: Literal["registered", "assignable"] = "registered",
    ) -> None: ...
    @property
    def group(self) -> int: ...
    @property
    def channel(self) -> int: ...
    @property
    def bank(self) -> int: ...
    @property
    def index(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def family(self) -> Literal["registered", "assignable"]: ...
    def replace(self, **changes: object) -> ParameterController: ...

class RelativeParameterController(Message):
    @property
    def version(self) -> Literal[2]: ...
    def __init__(
        self,
        *,
        group: int,
        channel: int,
        bank: int,
        index: int,
        value: int,
        family: Literal["registered", "assignable"] = "registered",
    ) -> None: ...
    @property
    def group(self) -> int: ...
    @property
    def channel(self) -> int: ...
    @property
    def bank(self) -> int: ...
    @property
    def index(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def family(self) -> Literal["registered", "assignable"]: ...
    def replace(self, **changes: object) -> RelativeParameterController: ...

class PerNotePitchBend(Message):
    @property
    def version(self) -> Literal[2]: ...
    def __init__(self, *, group: int, channel: int, note: int, value: int) -> None: ...
    @property
    def group(self) -> int: ...
    @property
    def channel(self) -> int: ...
    @property
    def note(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(self, **changes: object) -> PerNotePitchBend: ...
