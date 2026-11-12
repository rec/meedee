"""The shared semantic boundary preserves native values and explicit addressing."""

from io import BytesIO

import pytest

from meedee import (
    Reader,
    Writer,
    archive,
    codecs,
    messages,
    midi,
    testing,
    translation,
    ump,
)


def test_grouped_messages_require_explicit_byte_address_removal() -> None:
    note = messages.NoteOn(60, 127, 16, group=16).replace(stream="keys")
    assert note.version == 1
    assert ump.encode(note) == (0x2F9F3C7F,)
    assert ump.decode(ump.encode(note)) == note
    assert type(ump.decode(ump.encode(note))) is messages.NoteOn
    with pytest.raises(ValueError, match="group"):
        codecs.encode(note)
    edited = note.replace(channel=2).transpose(1)
    assert edited.stream == "keys" and edited.group == 16
    assert codecs.encode(edited.replace(group=None)) == b"\x91\x3d\x7f"
    assert ump.encode(messages.TimingClock(group=2)) == (0x11F80000,)
    with pytest.raises(ValueError, match="group"):
        ump.encode(messages.TimingClock())


def test_writer_retains_grouped_logical_values_and_native_midi2() -> None:
    values = [
        messages.SysEx(bytes(range(20)), group=3),
        messages.ControlChange(7, 10000, 2, resolution=14, group=3),
        messages.PerNotePitchBend(group=3, channel=2, note=60, value=-(2**31)),
        ump.SysEx7Packet(group=3, format=1, data=b"abc"),
    ]
    destination = BytesIO()
    with Writer(destination) as writer:
        writer.write(archive.Stream(id="keys", label="keyboard"))
        for v in values:
            writer.write(v.replace(stream="keys"))
    with Reader(BytesIO(destination.getvalue())) as reader:
        restored = [v for v in reader if isinstance(v, messages.Message)]
    assert restored == values
    assert [type(v) for v in restored] == [type(v) for v in values]
    assert len(ump.packetize_sysex7(values[0])) == 4


def test_one_ump_handoff_preserves_compound_values_and_complete_sysex() -> None:
    output = testing.Output(encoding="ump")
    output.send(messages.ControlChange(7, 10000, 2, resolution=14, group=3))
    assert output.take_packet() == midi.UmpPacket(words=(0x22B1074E, 0x22B12710))
    output.send(messages.SysEx(bytes(range(7)), group=3))
    assert output.take_packet() == midi.UmpPacket(
        words=(0x32160001, 0x02030405, 0x32310600, 0)
    )
    assert output.snapshot().accepted == 2
    note = messages.NoteOn(60, 90, 2).replace(stream="keys")
    converted = translation.to_midi2(note, group=3).messages[0]
    assert converted.stream == "keys"
    restored = translation.to_midi1(converted).messages[0]
    assert restored == note and restored.stream == "keys"


def test_legacy_files_require_explicit_group_removal() -> None:
    destination = BytesIO()
    with Writer(destination, format="syx") as writer:
        with pytest.raises(OSError, match="group"):
            writer.write(messages.SysEx(b"abc", group=3))
    assert destination.getvalue() == b""
