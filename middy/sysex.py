"""Opaque instrument dump requests and capture."""

from __future__ import annotations

import sys
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, sysex

if TYPE_CHECKING:
    from . import midi

_core.populate_sysex(sys.modules[__name__])

del sys


def dump(
    receiver: midi.Input,
    output: midi.Output | None = None,
    *,
    model: str | None = None,
    scope: str = "all",
    device_id: int | None = None,
    timeout: float | str = 120.0,
    silence: float | str = 1.0,
    identity_timeout: float | str = 1.0,
    message_limit: int = 4096,
    byte_limit: int | str = 8_388_608,
    payload_limit: int | str = 1_048_576,
    cancel: events.Cancellation | None = None,
) -> sysex.DumpReport:
    """Request an opaque dump, or capture a front-panel dump with output=None.

    Omit model to discover exactly one supported identity using output. Explicit
    model skips discovery and is required for manual capture.
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
        identity_timeout,
        message_limit,
        byte_limit,
        payload_limit,
        events.Cancellation() if cancel is None else cancel,
        monotonic_ns,
    )
