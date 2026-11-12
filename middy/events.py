"""Event sequences, immutable receive packets and clock metadata."""

import sys
from collections.abc import Iterator

from . import _core, messages, ump

_core.populate_events(sys.modules[__name__])

del sys, _core, messages, ump


def with_gap[T](items: Iterator[T], gap: float) -> Iterator[T | float]:
    """Yield each item followed by gap seconds, including after the final item."""
    for i in items:
        yield i
        yield gap
