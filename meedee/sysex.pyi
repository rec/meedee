"""Streaming concatenated SysEx dumps."""

from collections.abc import Iterable, Iterator
from os import PathLike
from typing import BinaryIO

from . import events, midi
from .messages import SysEx

class DumpReport:
    @property
    def model(self) -> str | None: ...
    @property
    def scope(self) -> str: ...
    @property
    def messages(self) -> list[SysEx]: ...
    @property
    def status(self) -> str: ...
    @property
    def complete(self) -> bool: ...
    @property
    def requests_sent(self) -> int: ...
    @property
    def uncertain_send(self) -> bool: ...
    @property
    def ignored_events(self) -> int: ...
    @property
    def wire_bytes(self) -> int: ...
    @property
    def reason(self) -> str | None: ...

def models() -> list[str]: ...
def dump(
    receiver: midi.Input,
    output: midi.Output | None = None,
    *,
    model: str | None = None,
    scope: str = "all",
    device_id: int | None = None,
    timeout: float | str = 120.0,
    silence: float | str = 1.0,
    identity_timeout: float | str = 1.0,
    message_limit: int = 4096,
    byte_limit: int | str = 8_388_608,
    payload_limit: int | str = 1_048_576,
    cancel: events.Cancellation | None = None,
) -> DumpReport: ...

class Reader:
    def __init__(
        self,
        source: str | PathLike[str] | BinaryIO,
        *,
        payload_limit: int | str = 1_048_576,
    ) -> None: ...
    def close(self) -> None: ...
    def __iter__(self) -> Iterator[SysEx]: ...
    def __next__(self) -> SysEx: ...

def write(
    destination: str | PathLike[str] | BinaryIO,
    messages: Iterable[SysEx],
    *,
    payload_limit: int | str = 1_048_576,
) -> None: ...
def send(
    output: midi.Output, messages: Iterable[SysEx], *, gap: float | str
) -> events.PlaybackReport: ...
