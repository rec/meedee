"""Read-only file summaries and capture validation, without MIDI initialization."""

from __future__ import annotations

import sys
from collections.abc import Callable
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, archive, events, notes, smf

if TYPE_CHECKING:
    from . import inspection, midi


def run(
    monitor: inspection.Monitor,
    receiver: midi.Input,
    *,
    callback: Callable[[inspection.MonitorEntry], None] | None = None,
    duration: float | str | None = None,
    cancel: events.Cancellation | None = None,
    metrics: bool = False,
) -> inspection.MonitorReport:
    """Run a fresh monitor without closing its input.

    The optional callback receives typed entries, including loss notifications.
    Known loss resets interpretation and continues; terminal failures raise.
    snapshot() preserves progress on failure. Callbacks must not reenter monitor.
    """
    return _core.run_monitor(
        monitor,
        receiver,
        callback,
        duration,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
        metrics,
    )


_core.populate_inspection(sys.modules[__name__])

del sys, archive, notes, smf
