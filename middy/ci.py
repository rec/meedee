"""MIDI-CI management codecs, caller-driven sessions and live browsing."""

from __future__ import annotations

import sys
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, ci, events, messages

if TYPE_CHECKING:
    from . import midi

_core.populate_ci(sys.modules[__name__])

del sys, messages


def browse(
    browser: ci.Browser,
    receiver: midi.Input,
    output: midi.Output,
    *,
    cancel: events.Cancellation | None = None,
    message_limit: int = 1_048_576,
    byte_limit: int | str = 8_388_608,
) -> ci.BrowseReport:
    """Run a fresh browser; callers own connections and serialize all access.

    A group-less route uses MIDI 1 bytes; an explicit group uses UMP SysEx7.
    Full-action handoffs are counted, and a partial/interrupted action is uncertain.
    """
    return _core.run_ci(
        browser,
        receiver,
        output,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
        message_limit,
        byte_limit,
    )
