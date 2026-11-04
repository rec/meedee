"""Caller-thread SysEx delivery through an existing MIDI 1 output."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import events, transfer

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
    from . import midi

    initial = session.snapshot()
    if (
        initial.status != "active"
        or initial.confirmed_messages
        or initial.in_flight is not None
    ):
        raise ValueError("transfer session must be fresh")
    cancel = events.Cancellation() if cancel is None else cancel
    try:
        while session.snapshot().status == "active":
            if cancel is not None and cancel.cancelled:
                session.cancel("cancelled by caller")
                break
            now = monotonic_ns()
            step = session.prepare(now)
            if step is None:
                deadline = session.snapshot().next_ready_ns
                assert deadline is not None
                cancel.wait_for(min(0.05, max(0, deadline - now) / 1_000_000_000))
                continue
            try:
                output(step.message)
            except (
                midi.MidiError,
                midi.StateError,
                midi.CallbackThreadError,
                midi.NativePanicError,
                midi.ResourceError,
                midi.WorkerError,
                midi.WorkerTimeoutError,
                OSError,
                ValueError,
            ) as error:
                session.fail(step.index, str(error))
                break
            session.confirm(step.index, monotonic_ns())
    except KeyboardInterrupt:
        session.cancel("interrupted")
    finally:
        if (index := session.snapshot().in_flight) is not None:
            session.fail(index, "send or confirmation did not complete")
    return session.snapshot()
