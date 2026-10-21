"""Optional uFor pitch previews and explicit MTS message construction, without I/O."""

from __future__ import annotations

import sys
from enum import StrEnum, auto
from math import isfinite

if sys.version_info < (3, 13):
    raise ImportError("meedee.ufor_tuning requires Python 3.13 or newer")
else:
    from ufor.base import Model
    from ufor.number import PitchNumber
    from ufor.tuning import FrequencyTable, IntervalPattern, RatioTable, Tuning

from . import codecs, messages, transfer, tuning


class PitchStatus(StrEnum):
    CONVERTED = auto()
    UNMAPPED = auto()
    OUT_OF_RANGE = auto()
    EVALUATION_ERROR = auto()


class KeyPreview(Model, frozen=True):
    note: int
    status: PitchStatus
    frequency: PitchNumber | None = None
    pitch: tuning.Pitch | None = None
    encoded_frequency_hz: float | None = None
    error_cents: float | None = None
    reason: str | None = None

    model_config = {"arbitrary_types_allowed": True, "strict": True}


class Preview(Model, frozen=True):
    entries: list[KeyPreview]

    model_config = {"strict": True}


def preview(source: Tuning, *, notes: list[int]) -> Preview:
    """Resolve selected keys in caller order; preserve uFor's exact result types."""
    if not isinstance(source, Tuning):
        raise TypeError("source must be a uFor Tuning")
    selected = _check_notes(notes)
    definition = source.model_copy(deep=True)
    minimum = tuning.Pitch(semitone=0, fraction=0).frequency_hz
    maximum = tuning.Pitch(semitone=127, fraction=16382).frequency_hz
    entries: list[KeyPreview] = []
    for note in selected:
        if not _covered(definition, note):
            entries.append(
                KeyPreview(
                    note=note,
                    status=PitchStatus.UNMAPPED,
                    reason="key is outside the finite uFor tuning",
                )
            )
            continue
        try:
            frequency = definition(note)
        except (ArithmeticError, ValueError) as error:
            entries.append(
                KeyPreview(
                    note=note,
                    status=PitchStatus.EVALUATION_ERROR,
                    reason=str(error),
                )
            )
            continue
        if isinstance(frequency, float) and not isfinite(frequency):
            entries.append(
                KeyPreview(
                    note=note,
                    status=PitchStatus.EVALUATION_ERROR,
                    reason="uFor resolved a non-finite frequency",
                )
            )
            continue
        if frequency <= 0:
            entries.append(
                KeyPreview(
                    note=note,
                    frequency=frequency,
                    status=PitchStatus.EVALUATION_ERROR,
                    reason="uFor resolved a non-positive frequency",
                )
            )
            continue
        # Compare the original value before converting an exact Fraction to
        # double. An out-of-range rational must not round onto an endpoint.
        if not minimum <= frequency <= maximum:
            entries.append(
                KeyPreview(
                    note=note,
                    frequency=frequency,
                    status=PitchStatus.OUT_OF_RANGE,
                    reason="frequency is outside meedee's MTS frequency range",
                )
            )
            continue
        converted = tuning.Pitch.from_frequency(frequency_hz=float(frequency))
        entries.append(
            KeyPreview(
                note=note,
                frequency=frequency,
                status=PitchStatus.CONVERTED,
                pitch=converted.pitch,
                encoded_frequency_hz=converted.pitch.frequency_hz,
                error_cents=converted.error_cents,
            )
        )
    return Preview(entries=entries)


def bulk_dump(
    result: Preview,
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
    result: Preview,
    *,
    device_id: int,
    program: int,
    name: str,
    gap_ns: int,
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
        gap_ns=gap_ns,
        message_limit=1,
        byte_limit=len(wire),
    )


def note_changes(
    result: Preview,
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
    source = definition.source
    if isinstance(source, FrequencyTable):
        return source.first_note <= note < source.first_note + len(source.values)
    degree = note - definition.root_note
    if isinstance(source, RatioTable):
        return source.repeat_ratio is not None or 0 <= degree < len(source.values)
    if isinstance(source, IntervalPattern):
        return source.repeat or 0 <= degree <= len(source.intervals)
    return True


def _changes(result: Preview, preserve_unmapped: bool) -> list[tuning.NoteTuning]:
    if type(preserve_unmapped) is not bool:
        raise TypeError("preserve_unmapped must be a boolean")
    entries = result.entries.copy()
    _check_notes([k.note for k in entries])
    changes: list[tuning.NoteTuning] = []
    for key in entries:
        if key.status == PitchStatus.CONVERTED and key.pitch is not None:
            value: tuning.Pitch | tuning.NoChange = key.pitch
        elif key.status == PitchStatus.UNMAPPED and preserve_unmapped:
            value = tuning.NoChange()
        else:
            raise ValueError(f"key {key.note} cannot be generated: {key.status.value}")
        changes.append(tuning.NoteTuning(note=key.note, pitch=value))
    return changes
