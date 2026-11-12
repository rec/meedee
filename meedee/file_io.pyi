"""Detect input formats; select output formats from filenames."""

from collections.abc import Iterator
from os import PathLike
from types import TracebackType
from typing import BinaryIO, Literal

from . import archive, events, messages, smf

class Reader:
    def __init__(
        self,
        source: str | PathLike[str] | BinaryIO,
        *,
        payload_limit: int | str = 1_048_576,
        recover: bool = False,
        preserve_unknown: bool = False,
    ) -> None: ...
    def __iter__(
        self,
    ) -> Iterator[
        events.Clock
        | archive.Stream
        | messages.Message
        | archive.Gap
        | archive.Metadata
        | archive.Discontinuity
        | archive.Opaque
        | smf.Header
        | smf.Event
    ]: ...
    def __next__(
        self,
    ) -> (
        events.Clock
        | archive.Stream
        | messages.Message
        | archive.Gap
        | archive.Metadata
        | archive.Discontinuity
        | archive.Opaque
        | smf.Header
        | smf.Event
    ): ...
    def __enter__(self) -> Reader: ...
    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
    def close(self) -> None: ...
    @property
    def complete(self) -> bool: ...
    @property
    def problem(self) -> str | None: ...

class Writer:
    def __init__(
        self,
        destination: str | PathLike[str] | BinaryIO,
        *,
        format: Literal["mid", "midi", "syx", "meedee"] | None = None,
        payload_limit: int | str = 1_048_576,
    ) -> None: ...
    def write(
        self,
        record: events.Clock
        | archive.Stream
        | messages.Message
        | archive.Gap
        | archive.Metadata
        | archive.Discontinuity
        | archive.Opaque
        | smf.Header
        | smf.Event,
    ) -> None: ...
    def finish(self, *, complete: bool = True, reason: str = "") -> None: ...
    def __enter__(self) -> Writer: ...
    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
