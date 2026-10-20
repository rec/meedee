"""Passive MIDI 1 SysEx capture through an existing typed input."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import transfer

if TYPE_CHECKING:
    from . import events, midi


def run(
    capture: transfer.Capture,
    receiver: midi.TypedInputConnection,
    *,
    cancel: events.Cancellation | None = None,
) -> transfer.CaptureReport:
    """Run a fresh capture; callers own the connection and serialize access.

    Uses the supplied input's existing decoder. No requests, retries or cleanup
    traffic are sent. The capture deadline starts here, after connection setup.
    Receive waits are at most 50 ms; an in-process driver stall remains unbounded.
    """
    from . import midi

    capture.start(monotonic_ns())
    try:
        while capture.snapshot().status == "active":
            if cancel is not None and cancel.cancelled:
                capture.cancel("cancelled by caller")
                break
            if receiver.dropped_packets:
                capture.fail("input packet loss reported by transport counter")
                break
            if (error := receiver.failure) is not None:
                raise error
            now = monotonic_ns()
            capture.poll(now)
            report = capture.snapshot()
            if report.status != "active":
                break
            assert report.deadline_ns is not None
            item = receiver.receive(
                timeout=min(0.05, (report.deadline_ns - now) / 1_000_000_000)
            )
            if receiver.dropped_packets:
                capture.fail("input packet loss reported by transport counter")
                break
            if (error := receiver.failure) is not None:
                raise error
            if cancel is not None and cancel.cancelled:
                capture.cancel("cancelled by caller")
                break
            if item is not None:
                capture.feed(item, monotonic_ns())
    except KeyboardInterrupt:
        capture.cancel("interrupted")
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
        capture.fail(str(error))
    finally:
        if capture.snapshot().status == "active":
            capture.fail("capture did not complete")
    return capture.snapshot()
