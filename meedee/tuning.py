"""MIDI Tuning Standard values, note conversion and message construction."""

from __future__ import annotations

import sys
from math import isfinite
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from ufor.tuning import Tuning

from . import _core, codecs, messages, transfer, tuning

_core.populate_tuning(sys.modules[__name__])

del sys, _core


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
        raise ValueError("frequency is outside meedee's MTS frequency range")
    pitch = tuning.Pitch.from_frequency(frequency_hz=float(frequency)).pitch
    return tuning.NoteTuning(note=note, pitch=pitch)


def bulk_dump(
    result: list[tuning.NoteTuning],
    *,
    device_id: int,
    program: int,
    name: str,
    bank: int | None = None,
    preserve_unmapped: bool = False,
) -> tuning.BulkDump | tuning.KeyBasedDump:
    """Build an ordinary or banked 128-key dump. Never open a port or send."""
    changes = _changes(result, preserve_unmapped)
    if len(changes) != 128 and not preserve_unmapped:
        raise ValueError("bulk dumps require all 128 keys or explicit preservation")
    values: list[tuning.Pitch | tuning.NoChange] = [
        tuning.NoChange() for _ in range(128)
    ]
    for change in changes:
        values[change.note] = change.pitch
    table = tuning.TuningTable(values=values)
    if bank is None:
        return tuning.BulkDump(
            device_id=device_id, program=program, name=name, table=table
        )
    return tuning.KeyBasedDump(
        device_id=device_id, bank=bank, program=program, name=name, table=table
    )


def bulk_plan(
    result: list[tuning.NoteTuning],
    *,
    device_id: int,
    program: int,
    name: str,
    gap: float | str,
    bank: int | None = None,
    preserve_unmapped: bool = False,
) -> transfer.Plan:
    """Prepare one bulk dump for the existing transfer executor, without I/O."""
    message = bulk_dump(
        result,
        device_id=device_id,
        program=program,
        name=name,
        bank=bank,
        preserve_unmapped=preserve_unmapped,
    )
    wire = codecs.encode(message)
    return transfer.Plan(
        [messages.SysEx(data=wire[1:-1])],
        gap=gap,
        message_limit=1,
        byte_limit=len(wire),
    )


def note_changes(
    result: list[tuning.NoteTuning],
    *,
    device_id: int,
    program: int,
    realtime: bool,
    bank: int | None = None,
    preserve_unmapped: bool = False,
) -> list[tuning.SingleNoteChange | tuning.BankedNoteChange]:
    """Build ordered batches of at most 127 keys, with explicit real-time intent."""
    if type(realtime) is not bool:
        raise TypeError("realtime must be a boolean")
    if bank is None and not realtime:
        raise ValueError(
            "unbanked note changes are real-time; use a bank for non-real-time"
        )
    changes = _changes(result, preserve_unmapped)
    outgoing: list[tuning.SingleNoteChange | tuning.BankedNoteChange] = []
    for start in range(0, len(changes), 127):
        part = changes[start : start + 127]
        if bank is None:
            outgoing.append(
                tuning.SingleNoteChange(
                    device_id=device_id, program=program, changes=part
                )
            )
        else:
            outgoing.append(
                tuning.BankedNoteChange(
                    device_id=device_id,
                    bank=bank,
                    program=program,
                    realtime=realtime,
                    changes=part,
                )
            )
    return outgoing


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


def _changes(
    result: list[tuning.NoteTuning], preserve_unmapped: bool
) -> list[tuning.NoteTuning]:
    if type(preserve_unmapped) is not bool:
        raise TypeError("preserve_unmapped must be a boolean")
    changes = result.copy()
    _check_notes([k.note for k in changes])
    if not preserve_unmapped and any(
        isinstance(k.pitch, tuning.NoChange) for k in changes
    ):
        raise ValueError("NoChange requires explicit preservation")
    return changes
