"""One MIDI 1 MTS dump query through caller-owned MIDI connections."""

from __future__ import annotations

from time import monotonic_ns
from typing import TYPE_CHECKING

from . import tuning

if TYPE_CHECKING:
    from . import events, midi


def run(
    query: tuning.Query,
    receiver: midi.TypedInputConnection,
    output: midi.OutputConnection,
    *,
    cancel: events.Cancellation | None = None,
) -> tuning.QueryReport:
    """Send once, then receive until terminal. Never close connections or retry.

    The caller supplies one MIDI 1 queued input and output. Existing queued input
    is eligible: MTS has no transaction ID to disambiguate an identical old dump.
    """
    from . import midi

    if query.snapshot().status != "ready":
        raise ValueError("query must be fresh")
    try:
        while query.snapshot().status in ("ready", "waiting"):
            if cancel is not None and cancel.cancelled:
                query.cancel("cancelled by caller")
                break
            if receiver.dropped_packets:
                query.fail("input packet loss reported by transport counter")
                break
            if (error := receiver.failure) is not None:
                raise error
            now = monotonic_ns()
            if query.snapshot().status == "ready":
                request = query.prepare(now)
                output.send_message(request)
                query.confirm(monotonic_ns())
                continue
            query.poll(now)
            report = query.snapshot()
            if report.status != "waiting":
                break
            assert report.deadline_ns is not None
            item = receiver.receive(
                timeout=min(0.05, (report.deadline_ns - now) / 1_000_000_000)
            )
            if receiver.dropped_packets:
                query.fail("input packet loss reported by transport counter")
                break
            if (error := receiver.failure) is not None:
                raise error
            if cancel is not None and cancel.cancelled:
                query.cancel("cancelled by caller")
                break
            if item is not None:
                query.feed(item, monotonic_ns())
    except KeyboardInterrupt:
        query.cancel("interrupted")
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
        query.fail(str(error))
    finally:
        if query.snapshot().status in ("ready", "sending", "waiting"):
            query.fail("query did not complete")
    return query.snapshot()
