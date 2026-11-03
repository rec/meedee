"""Immutable values backed by the shared Rust MIDI toolkit."""

from typing import Self

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
        self, note: int, velocity: int = 90, channel: int | None = None
    ) -> None: ...
    def transpose(self, semitones: int) -> NoteOn: ...
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
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NoteOn: ...

class SysEx(Message):
    def __init__(self, data: bytes | bytearray | memoryview) -> None: ...
    @property
    def data(self) -> bytes: ...
    def replace(
        self,
        data: bytes | bytearray | memoryview = ...,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SysEx: ...

class NoteOff(Message):
    def __init__(
        self, note: int, velocity: int = 0, channel: int | None = None
    ) -> None: ...
    def transpose(self, semitones: int) -> NoteOff: ...
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
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NoteOff: ...

class PolyPressure(Message):
    def __init__(
        self, note: int, pressure: int = 0, channel: int | None = None
    ) -> None: ...
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
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> PolyPressure: ...

class ControlChange(Message):
    def __init__(
        self, controller: int, value: int, channel: int | None = None
    ) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def controller(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        controller: int = ...,
        value: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ControlChange: ...

class ProgramChange(Message):
    def __init__(self, program: int, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def program(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        program: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ProgramChange: ...

class ChannelPressure(Message):
    def __init__(self, pressure: int = 0, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def pressure(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        pressure: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ChannelPressure: ...

class PitchBend(Message):
    def __init__(self, value: int = 0, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        value: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> PitchBend: ...

class QuarterFrame(Message):
    def __init__(self, message_type: int, value: int) -> None: ...
    @property
    def message_type(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        message_type: int = ...,
        value: int = ...,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> QuarterFrame: ...

class SongPosition(Message):
    def __init__(self, position: int) -> None: ...
    @property
    def position(self) -> int: ...
    def replace(
        self,
        position: int = ...,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SongPosition: ...

class SongSelect(Message):
    def __init__(self, song: int) -> None: ...
    @property
    def song(self) -> int: ...
    def replace(
        self,
        song: int = ...,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SongSelect: ...

class TuneRequest(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> TuneRequest: ...

class TimingClock(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> TimingClock: ...

class Start(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Start: ...

class Continue(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Continue: ...

class Stop(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> Stop: ...

class ActiveSensing(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ActiveSensing: ...

class SystemReset(Message):
    def __init__(self) -> None: ...
    def replace(
        self,
        *,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> SystemReset: ...

class ControlChange14(Message):
    def __init__(
        self, controller: int, value: int, channel: int | None = None
    ) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def controller(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        controller: int = ...,
        value: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> ControlChange14: ...

class RpnSet(Message):
    def __init__(
        self,
        parameter: int,
        value: int,
        resolution: int = 14,
        channel: int | None = None,
    ) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def resolution(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        value: int = ...,
        resolution: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> RpnSet: ...

class NrpnSet(Message):
    def __init__(
        self,
        parameter: int,
        value: int,
        resolution: int = 14,
        channel: int | None = None,
    ) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    @property
    def value(self) -> int: ...
    @property
    def resolution(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        value: int = ...,
        resolution: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NrpnSet: ...

class RpnIncrement(Message):
    def __init__(self, parameter: int, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> RpnIncrement: ...

class RpnDecrement(Message):
    def __init__(self, parameter: int, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> RpnDecrement: ...

class NrpnIncrement(Message):
    def __init__(self, parameter: int, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NrpnIncrement: ...

class NrpnDecrement(Message):
    def __init__(self, parameter: int, channel: int | None = None) -> None: ...
    @property
    def channel(self) -> int | None: ...
    @property
    def parameter(self) -> int: ...
    def replace(
        self,
        *,
        channel: int | None = ...,
        parameter: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> NrpnDecrement: ...
