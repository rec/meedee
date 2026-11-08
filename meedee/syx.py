"""Rust-owned streaming SYX file codecs."""

from __future__ import annotations

import sys
from collections.abc import Iterable
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, messages

if TYPE_CHECKING:
    from . import midi

_core.populate_syx(sys.modules[__name__])

del sys


def send(
    output: midi.Output, messages: Iterable[messages.SysEx], *, gap: float | str
) -> events.PlaybackReport:
    """Send untimed SysEx with a minimum pause between successful handoffs.

    Does not close the output, retry a failure, or infer device acknowledgement.
    """
    return _core.run_syx_send(
        output, messages, gap, events.Cancellation(), monotonic_ns
    )
