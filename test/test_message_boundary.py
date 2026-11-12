"""The shared semantic boundary preserves native values and explicit addressing."""

from io import BytesIO

import pytest

from meedee import (
    Reader,
    Writer,
    archive,
    codecs,
    events,
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


@pytest.mark.parametrize(
    "words",
    [
        (0x2F9F3C7F,),
        (0x4F9F3C00, 0xFFFF0000),
        (0,),
        (0x3F037D01, 0x02000000),
        (0xB0000000, 1, 2),
    ],
)
def test_opaque_archives_keep_raw_identity_even_for_known_messages(
    words: tuple[int, ...],
) -> None:
    value = ump.OpaquePacket(words=words).replace(stream="keys")
    destination = BytesIO()
    with Writer(destination) as writer:
        writer.write(archive.Stream(id="keys", label="keyboard"))
        writer.write(value)
    for preserve in (False, True):
        with Reader(
            BytesIO(destination.getvalue()), preserve_unknown=preserve
        ) as reader:
            restored = [v for v in reader if isinstance(v, messages.Message)]
            assert reader.complete
        assert len(restored) == 1
        assert type(restored[0]) is ump.OpaquePacket
        assert restored[0].words == words and restored[0].stream == "keys"


@pytest.mark.parametrize(
    "value",
    [
        messages.NoteOn(60, 40000, 2, version=2, group=9),
        messages.NoteOff(60, 40000, 2, version=2, group=9),
        messages.PolyPressure(60, 123456, 2, version=2, group=9),
        messages.ControlChange(7, 123456, 2, version=2, group=9),
        messages.ProgramChange(3, 2, version=2, group=9),
        messages.ChannelPressure(123456, 2, version=2, group=9),
        messages.PitchBend(-123456, 2, version=2, group=9),
    ],
)
def test_midi2_group_removal_never_redirects_to_the_default(
    value: messages.NoteOn
    | messages.NoteOff
    | messages.PolyPressure
    | messages.ControlChange
    | messages.ProgramChange
    | messages.ChannelPressure
    | messages.PitchBend,
) -> None:
    with pytest.raises(ValueError, match="MIDI 2 requires a group"):
        value.replace(group=None)
    assert value.group == 9 and value.version == 2
    assert value.replace().group == 9
    assert value.replace(group=3).group == 3


@pytest.mark.parametrize(
    "value,words",
    [
        (messages.ControlChange(7, 90, 2, group=3), (0x22B1075A,)),
        (
            messages.ControlChange(7, 10000, 2, group=3, resolution=14),
            (0x22B1074E, 0x22B12710),
        ),
        (
            messages.ControlChange(7, 0xFEDCBA98, 2, group=3, version=2),
            (0x42B10700, 0xFEDCBA98),
        ),
    ],
)
def test_ump_controller_encoding_preserves_native_resolution(
    value: messages.ControlChange,
    words: tuple[int, ...],
) -> None:
    assert ump.encode(value) == words
    assert value.version == (2 if value.resolution == 32 else 1)
    if value.resolution == 14:
        with pytest.raises(ValueError):
            codecs.encode(value.replace(group=None))


def test_complete_sysex7_translation_keeps_archive_provenance() -> None:
    clock = events.Clock(identity="clock")
    timestamp = events.Timestamp(ticks=9, clock=clock)
    span = events.SourceSpan(
        first_packet=2,
        last_packet=3,
        first_timestamp=timestamp,
        last_timestamp=timestamp,
    )
    packet = ump.SysEx7Packet(group=9, format=0, data=b"abc").replace(
        stream="keys",
        timestamp=timestamp,
        span=span,
    )
    report = translation.to_midi1(packet)
    assert report.status == translation.TranslationStatus.Exact and report.group == 9
    assert len(report.messages) == 1
    value = report.messages[0]
    assert value == messages.SysEx(b"abc")
    assert (value.stream, value.timestamp, value.span) == ("keys", timestamp, span)
    destination = BytesIO()
    with Writer(destination) as writer:
        writer.write(clock)
        writer.write(
            archive.Stream(id="keys", label="keyboard", clock_identity="clock")
        )
        writer.write(value)
    with Reader(BytesIO(destination.getvalue())) as reader:
        restored = [v for v in reader if isinstance(v, messages.Message)]
        assert reader.complete
    assert restored == [value]
    assert (restored[0].stream, restored[0].timestamp, restored[0].span) == (
        "keys",
        timestamp,
        span,
    )
