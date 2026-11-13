"""Event sequences, immutable receive packets and clock metadata."""

import sys
from collections.abc import Iterator

from . import _core, _units, messages, ump

_core.populate_events(sys.modules[__name__])

del sys, _core, messages, ump


def with_gap[T](items: Iterator[T], gap: float | str) -> Iterator[T | float | str]:
    """Yield each item followed by a contextual or explicitly unit-bearing delay."""
    if isinstance(gap, str):
        _units.musical_duration(gap)
    for i in items:
        yield i
        yield gap
