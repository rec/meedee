"""Passive MIDI 1 SysEx capture through an existing typed input."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, transfer

if TYPE_CHECKING:
    from . import midi


def run(
    capture: transfer.Capture,
    receiver: midi.Input,
    *,
    cancel: events.Cancellation | None = None,
) -> transfer.CaptureReport:
    """Run a fresh capture; callers own the connection and serialize access.

    Uses the supplied input's existing decoder. No requests, retries or cleanup
    traffic are sent. The capture deadline starts here, after connection setup.
    Receive waits are at most 50 ms; an in-process driver stall remains unbounded.
    """
    return _core.run_transfer_input(
        capture,
        receiver,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
