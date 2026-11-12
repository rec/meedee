"""Universal MIDI values, codecs and bounded identity inquiry."""

from __future__ import annotations

import sys
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, messages, universal

if TYPE_CHECKING:
    from . import midi

_core.populate_universal(sys.modules[__name__])

del sys, messages


def identify(
    receiver: midi.Input,
    output: midi.Output,
    *,
    device_id: int = 127,
    timeout: float | str = 1.0,
    message_limit: int = 4096,
    byte_limit: int | str = 8_388_608,
    payload_limit: int | str = 1_048_576,
    cancel: events.Cancellation | None = None,
) -> universal.IdentityReport:
    """Send once and collect all raw identity replies through the timeout.

    Connections remain open. Buffered input is discarded. Replies are not
    deduplicated, interpreted as model selections, or retried.
    """
    return _core.run_identify(
        receiver,
        output,
        device_id,
        timeout,
        message_limit,
        byte_limit,
        payload_limit,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
