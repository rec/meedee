"""Universal MIDI values, codecs and bounded identity inquiry."""

from . import events, messages, midi
from .messages import SysEx

class IdentityReport:
    @property
    def messages(self) -> list[SysEx]: ...
    @property
    def status(self) -> str: ...
    @property
    def uncertain_send(self) -> bool: ...
    @property
    def ignored_events(self) -> int: ...
    @property
    def reason(self) -> str | None: ...

def identify(
    receiver: midi.Input,
    output: midi.Output,
    *,
    device_id: int = 127,
    timeout: float | str = 1.0,
    message_limit: int = 4096,
    byte_limit: int | str = 8_388_608,
    payload_limit: int | str = 1_048_576,
    cancel: events.Cancellation | None = None,
) -> IdentityReport: ...

class IdentityRequest(messages.Message):
    def __init__(self, *, device_id: int) -> None: ...
    @property
    def device_id(self) -> int: ...
    def replace(
        self,
        *,
        device_id: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> IdentityRequest: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class IdentityReply(messages.Message):
    def __init__(
        self,
        *,
        device_id: int,
        manufacturer: list[int] | tuple[int, ...],
        family: int,
        model: int,
        revision: list[int] | tuple[int, ...],
    ) -> None: ...
    @property
    def device_id(self) -> int: ...
    @property
    def manufacturer(self) -> tuple[int, ...]: ...
    @property
    def family(self) -> int: ...
    @property
    def model(self) -> int: ...
    @property
    def revision(self) -> tuple[int, ...]: ...
    def replace(
        self,
        *,
        device_id: int = ...,
        manufacturer: list[int] | tuple[int, ...] = ...,
        family: int = ...,
        model: int = ...,
        revision: list[int] | tuple[int, ...] = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> IdentityReply: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class MasterVolume(messages.Message):
    def __init__(self, *, device_id: int, value: int) -> None: ...
    @property
    def device_id(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        device_id: int = ...,
        value: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> MasterVolume: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class MasterFineTuning(messages.Message):
    def __init__(self, *, device_id: int, value: int) -> None: ...
    @property
    def device_id(self) -> int: ...
    @property
    def value(self) -> int: ...
    def replace(
        self,
        *,
        device_id: int = ...,
        value: int = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> MasterFineTuning: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class MasterCoarseTuning(messages.Message):
    def __init__(self, *, device_id: int, semitones: int | str) -> None: ...
    @property
    def device_id(self) -> int: ...
    @property
    def semitones(self) -> int: ...
    def replace(
        self,
        *,
        device_id: int = ...,
        semitones: int | str = ...,
        timestamp: events.Timestamp | None = ...,
        stream: str | None = ...,
        span: events.SourceSpan | None = ...,
    ) -> MasterCoarseTuning: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

def decode(
    message: messages.SysEx,
) -> (
    IdentityRequest
    | IdentityReply
    | MasterVolume
    | MasterFineTuning
    | MasterCoarseTuning
    | None
): ...
