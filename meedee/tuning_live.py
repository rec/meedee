"""One MIDI 1 MTS dump query through caller-owned MIDI connections."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, tuning

if TYPE_CHECKING:
    from . import midi


def run(
    query: tuning.Query,
    receiver: midi.Input,
    output: midi.Output,
    *,
    cancel: events.Cancellation | None = None,
) -> tuning.QueryReport:
    """Send once, then receive until terminal. Never close connections or retry.

    The caller supplies one MIDI 1 queued input and output. Existing queued input
    is eligible: MTS has no transaction ID to disambiguate an identical old dump.
    """
    return _core.run_tuning(
        query,
        receiver,
        output,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
