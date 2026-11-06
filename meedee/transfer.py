"""MIDI 1 SysEx plans, progress tracking, live delivery and passive capture."""

from __future__ import annotations

import sys
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, messages, transfer

if TYPE_CHECKING:
    from . import midi

_core.populate_transfer(sys.modules[__name__])

del sys, messages


def send(
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


def capture(
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
