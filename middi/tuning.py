"""MIDI Tuning Standard values, note conversion and message construction."""

from __future__ import annotations

import sys
from math import isfinite
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import _core, events, tuning

if TYPE_CHECKING:
    from ufor.tuning import Tuning

    from . import midi

_core.populate_tuning(sys.modules[__name__])

del sys


def convert(source: Tuning, note: int) -> tuning.NoteTuning:
    """Convert one source note to MTS; raise KeyError outside finite coverage."""
    from ufor.tuning import Tuning

    if not isinstance(source, Tuning):
        raise TypeError("source must be a uFor Tuning")
    _check_notes([note])
    if not _covered(source, note):
        raise KeyError(f"note {note} is outside the finite uFor tuning")
    frequency = source(note)
    if isinstance(frequency, float) and not isfinite(frequency):
        raise ValueError("uFor resolved a non-finite frequency")
    if frequency <= 0:
        raise ValueError("uFor resolved a non-positive frequency")
    minimum = tuning.Pitch(semitone=0, fraction=0).frequency_hz
    maximum = tuning.Pitch(semitone=127, fraction=16382).frequency_hz
    # Compare exact values before float conversion can round onto an endpoint.
    if not minimum <= frequency <= maximum:
        raise ValueError("frequency is outside middi's MTS frequency range")
    pitch = tuning.Pitch.from_frequency(frequency_hz=float(frequency)).pitch
    return tuning.NoteTuning(note=note, pitch=pitch)


def query(
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


def _check_notes(notes: list[int]) -> list[int]:
    if type(notes) is not list:
        raise TypeError("notes must be a list of MIDI key integers")
    notes = notes.copy()
    if not 1 <= len(notes) <= 128:
        raise ValueError("select between one and 128 MIDI keys")
    if any(type(n) is not int for n in notes):
        raise TypeError("MIDI keys must be built-in integers")
    if any(not 0 <= n <= 127 for n in notes):
        raise ValueError("MIDI keys must be in 0..127")
    if len(set(notes)) != len(notes):
        raise ValueError("selected MIDI keys must be distinct")
    return notes


def _covered(definition: Tuning, note: int) -> bool:
    from ufor.tuning import FrequencyTable, IntervalPattern, RatioTable

    source = definition.source
    if isinstance(source, FrequencyTable):
        return source.first_note <= note < source.first_note + len(source.values)
    degree = note - definition.root_note
    if isinstance(source, RatioTable):
        return source.repeat_ratio is not None or 0 <= degree < len(source.values)
    if isinstance(source, IntervalPattern):
        return source.repeat or 0 <= degree <= len(source.intervals)
    return True
