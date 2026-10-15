"""Streaming concatenated SysEx dumps."""

from collections.abc import Iterable, Iterator
from os import PathLike
from typing import BinaryIO

from .messages import SysEx

class Reader:
    def __init__(
        self, source: str | PathLike[str] | BinaryIO, *, payload_limit: int = 1_048_576
    ) -> None: ...
    def close(self) -> None: ...
    def __iter__(self) -> Iterator[SysEx]: ...
    def __next__(self) -> SysEx: ...

def write(
    destination: str | PathLike[str] | BinaryIO,
    messages: Iterable[SysEx],
    *,
    payload_limit: int = 1_048_576,
) -> None: ...
