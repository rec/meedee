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
