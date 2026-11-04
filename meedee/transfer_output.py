"""Caller-thread SysEx delivery through an existing MIDI 1 output."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, transfer

if TYPE_CHECKING:
    from . import midi


def run(
    session: transfer.Session,
    output: midi.Output,
    *,
    cancel: events.Cancellation | None = None,
) -> transfer.Report:
    """Run until terminal; never close the caller's output or retry a send.

    The caller supplies a MIDI 1 connection. Cancellation is checked at most every
    50 ms while waiting, but cannot interrupt an in-flight native send. Unexpected
    exceptions propagate; the caller can inspect the supplied session afterward.
    """
    return _core.run_transfer_output(
        session,
        output,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
