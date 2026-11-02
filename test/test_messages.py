"""Exercise the public values using the shared representation contract."""

import sys
from pathlib import Path
from subprocess import run

from native import meedee_messages

from meedee import messages


def test_public_messages_preserve_the_frozen_representation_contract() -> None:
    meedee_messages.verify(messages)


def test_message_import_does_not_initialize_midi_runtime() -> None:
    fixture = Path(__file__).parent / "native" / "meedee_message_import.py"
    run([sys.executable, str(fixture)], check=True, timeout=10)


def test_unassigned_messages_preserve_identity_until_resolved() -> None:
    import pytest

    from meedee import codecs

    note = messages.NoteOn(60)
    assert note.channel is None and note.velocity == 90
    assert note == note.replace() and hash(note) == hash(note.replace())
    assert note != note.replace(channel=1)
    assert note.replace(channel=2).replace(channel=None) == note
    with pytest.raises(ValueError, match="unassigned"):
        codecs.encode(note)
    assert codecs.encode(note.replace(channel=2)) == b"\x91\x3c\x5a"
    command = messages.RpnSet(0, 256)
    assert command.resolution == 14 and command.channel is None
    with pytest.raises(ValueError, match="unassigned"):
        codecs.expand(command)
    assert codecs.expand(command.replace(channel=3))
    assert codecs.encode(messages.NoteOff(60, channel=1)) == b"\x80\x3c\x00"
    assert codecs.encode(messages.SongPosition(1)) == b"\xf2\x01\x00"


def test_full_notes_delegate_midi_validation_and_keep_authored_duration() -> None:
    import pytest

    note = messages.Note("500 ms", 60)
    assert (note.duration, note.note, note.velocity, note.channel) == (
        "500 ms",
        60,
        90,
        None,
    )
    assert messages.Rest("1 beat").duration == "1 beat"
    for duration in [-1, float("inf"), float("nan"), True, "1 Hz", "-1 beat"]:
        with pytest.raises((ValueError, TypeError)):
            messages.Note(duration, 60)
        with pytest.raises((ValueError, TypeError)):
            messages.Rest(duration)
    for fields in [
        {"note": 128},
        {"note": True},
        {"note": 60, "velocity": 128},
        {"note": 60, "channel": 17},
    ]:
        with pytest.raises((ValueError, TypeError)):
            messages.Note(1, **fields)
    with pytest.raises(AttributeError):
        note.duration = 2  # ty: ignore[invalid-assignment]


def test_note_transposition_clamps_and_preserves_channel_and_velocity() -> None:
    for constructor in (messages.NoteOn, messages.NoteOff):
        note = constructor(60, 75)
        shifted = note.transpose(12)
        assert (shifted.note, shifted.velocity, shifted.channel) == (72, 75, None)
        assert note.note == 60
        assert note.transpose(-100).note == 0
        assert note.transpose(100).note == 127
        assert constructor(120, 50, 3).transpose(12).channel == 3
