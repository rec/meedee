"""Exercise the public values using the shared representation contract."""

import sys
from pathlib import Path
from subprocess import run

from native import meedee_messages

from meedee import messages


def test_midi_versions_share_classes_without_reinterpreting_values() -> None:
    import pytest

    from meedee import codecs, events, ump

    pairs = [
        (messages.NoteOn(60, 127, 1), messages.NoteOn(60, 65535, 1, version=2)),
        (messages.NoteOff(60, 127, 1), messages.NoteOff(60, 65535, 1, version=2)),
        (
            messages.PolyPressure(60, 127, 1),
            messages.PolyPressure(60, 2**32 - 1, 1, version=2),
        ),
        (
            messages.ControlChange(7, 127, 1),
            messages.ControlChange(7, 2**32 - 1, 1, version=2),
        ),
        (messages.ProgramChange(128, 1), messages.ProgramChange(128, 1, version=2)),
        (
            messages.ChannelPressure(127, 1),
            messages.ChannelPressure(2**32 - 1, 1, version=2),
        ),
        (messages.PitchBend(8191, 1), messages.PitchBend(2**31 - 1, 1, version=2)),
    ]
    timestamp = events.Timestamp(ticks=12, clock=events.Clock(identity="capture"))
    for a, b in pairs:
        assert type(a) is type(b) and a != b
        assert (a.version, a.group, b.version, b.group) == (1, None, 2, 1)
        assert codecs.encode(a)
        assert ump.decode(ump.encode(b)) == b
        marked = b.replace(timestamp=timestamp, stream="keyboard")
        assert marked == b and hash(marked) == hash(b)
        assert marked.replace(channel=2).timestamp == timestamp
        assert marked.replace(channel=2).stream == "keyboard"
        with pytest.raises(ValueError):
            codecs.encode(b)
        with pytest.raises((ValueError, TypeError)):
            ump.encode(a)
        with pytest.raises(ValueError, match="version"):
            a.replace(version=2)
        with pytest.raises(ValueError, match="version"):
            b.replace(version=1)
        with pytest.raises(ValueError):
            a.replace(group=1)
    note = messages.NoteOn(60, version=2)
    assert note.channel is None and note.velocity == ump.scale_up(
        value=90, source_bits=7, destination_bits=16
    )
    assert messages.NoteOn(60, 90, version=2).velocity == 90
    with pytest.raises(ValueError):
        ump.encode(note)
    attributed = note.replace(channel=2, group=3, attribute_type=3, attribute_data=123)
    shifted = attributed.replace(timestamp=timestamp).transpose(12)
    assert shifted == attributed.replace(note=72)
    assert shifted.timestamp == timestamp
    with pytest.raises(ValueError):
        messages.NoteOn(60, attribute_type=0)
    with pytest.raises(ValueError):
        messages.ProgramChange(1, options=0)


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


def test_transformations_preserve_metadata_and_payload_identity() -> None:
    from meedee import codecs, events, gm, ump

    clock = events.Clock(identity="capture")
    timestamp = events.Timestamp(ticks=12, clock=clock)
    span = events.SourceSpan(first_packet=1, last_packet=1)
    for m in [
        messages.NoteOn(60, channel=2),
        messages.SysEx(b"\x7d\x01"),
        messages.TimingClock(),
        gm.BankProgramSelect(12, channel=2),
        ump.Midi1Packet(group=1, message=messages.NoteOn(60, channel=2)),
    ]:
        tagged = m.replace(timestamp=timestamp, stream="keyboard", span=span)
        assert tagged == m and hash(tagged) == hash(m)
        assert tagged.replace().timestamp == timestamp
        assert tagged.replace().stream == "keyboard"
        assert tagged.replace().span == span
        assert m.timestamp is None and m.stream is None and m.span is None
        cleared = tagged.replace(timestamp=None, stream=None, span=None)
        assert (
            cleared.timestamp is None
            and cleared.stream is None
            and cleared.span is None
        )
    for m in [messages.NoteOn(120, channel=2), messages.NoteOff(120, channel=2)]:
        tagged = m.replace(timestamp=timestamp, stream="keyboard", span=span)
        changed = tagged.transpose(12).replace(velocity=33)
        assert changed.note == 127 and changed.velocity == 33
        assert (changed.timestamp, changed.stream, changed.span) == (
            timestamp,
            "keyboard",
            span,
        )
        assert codecs.encode(tagged) == codecs.encode(m)
    assert not hasattr(events, "Event")


def test_metadata_rejects_empty_source_and_inconsistent_span() -> None:
    import pytest

    from meedee import events

    clock = events.Clock(identity="one")
    other = events.Clock(identity="two")
    span = events.SourceSpan(
        first_packet=0,
        last_packet=0,
        first_timestamp=events.Timestamp(ticks=0, clock=clock),
        last_timestamp=events.Timestamp(ticks=1, clock=clock),
    )
    with pytest.raises(ValueError, match="empty stream"):
        messages.NoteOn(60).replace(stream="")
    with pytest.raises(ValueError):
        messages.NoteOn(60).replace(
            span=span, timestamp=events.Timestamp(ticks=2, clock=other)
        )


def test_transpose_defaults_to_the_same_message_object() -> None:
    from meedee import events, ump

    timestamp = events.Timestamp(ticks=12, clock=events.Clock(identity="source"))
    for m in [
        messages.ControlChange(7, 90),
        messages.ChannelPressure(90),
        messages.PitchBend(100),
        messages.TimingClock(),
        messages.SysEx(b"\x7d\x01"),
        ump.Noop(),
        ump.Midi1Packet(group=2, message=messages.TimingClock()),
    ]:
        tagged = m.replace(timestamp=timestamp, stream="keyboard")
        assert tagged.transpose(semitones=12) is tagged
        assert tagged.transpose(-12) is tagged


def test_transpose_shifts_all_note_addresses_and_preserves_other_fields() -> None:
    from meedee import events, ump

    timestamp = events.Timestamp(ticks=12, clock=events.Clock(identity="source"))
    span = events.SourceSpan(first_packet=1, last_packet=2)
    for m in [
        messages.NoteOn(60, 100, 2),
        messages.NoteOff(60, 50, 2),
        messages.PolyPressure(60, 50, 2),
        messages.NoteOn(
            60, 40_000, 2, version=2, group=3, attribute_type=1, attribute_data=123
        ),
        messages.NoteOff(60, 30_000, 2, version=2, group=3),
        messages.PolyPressure(60, 0xFEDCBA98, 2, version=2, group=3),
        ump.Midi2RegisteredPerNoteController(
            group=3, channel=2, note=60, controller=1, value=1234
        ),
        ump.Midi2AssignablePerNoteController(
            group=3, channel=2, note=60, controller=2, value=5678
        ),
        ump.Midi2PerNotePitchBend(group=3, channel=2, note=60, value=-1234),
        ump.Midi2PerNoteManagement(group=3, channel=2, note=60, flags=3),
    ]:
        tagged = m.replace(timestamp=timestamp, stream="keyboard", span=span)
        assert tagged.transpose(12) == tagged.replace(note=72)
        assert tagged.transpose(-(2**63)).note == 0
        assert tagged.transpose(2**63 - 1).note == 127
        shifted = tagged.transpose(12)
        assert type(shifted) is type(tagged)
        assert (shifted.timestamp, shifted.stream, shifted.span) == (
            timestamp,
            "keyboard",
            span,
        )
        assert tagged.note == 60
    wrapped = ump.Midi1Packet(
        group=3, message=messages.PolyPressure(60, 50, 2)
    ).replace(timestamp=timestamp, stream="keyboard", span=span)
    shifted = wrapped.transpose(12)
    assert shifted.group == 3 and shifted.message == messages.PolyPressure(72, 50, 2)
    assert (shifted.timestamp, shifted.stream, shifted.span) == (
        timestamp,
        "keyboard",
        span,
    )
