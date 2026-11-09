"""Rust-owned streaming SYX file codecs."""

from __future__ import annotations

import sys
from collections.abc import Iterable
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, messages, sysex

if TYPE_CHECKING:
    from . import midi

_core.populate_sysex(sys.modules[__name__])

del sys


def send(
    output: midi.Output, messages: Iterable[messages.SysEx], *, gap: float | str
) -> events.PlaybackReport:
    """Send untimed SysEx with a minimum pause between successful handoffs.

    Does not close the output, retry a failure, or infer device acknowledgement.
    """
    return _core.run_sysex_send(
        output, messages, gap, events.Cancellation(), monotonic_ns
    )


def dump(
    receiver: midi.Input,
    output: midi.Output | None = None,
    *,
    model: str,
    scope: str = "all",
    device_id: int | None = None,
    timeout: float | str = 120.0,
    silence: float | str = 1.0,
    message_limit: int = 4096,
    byte_limit: int | str = 8_388_608,
    payload_limit: int | str = 1_048_576,
    cancel: events.Cancellation | None = None,
) -> sysex.DumpReport:
    """Request an opaque dump, or capture a front-panel dump with output=None.

    Connections remain open. This consumes input and discards buffered events
    before capture. Manual completion and model identity remain unverified.
    """
    return _core.run_sysex_dump(
        receiver,
        output,
        model,
        scope,
        device_id,
        timeout,
        silence,
        message_limit,
        byte_limit,
        payload_limit,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
