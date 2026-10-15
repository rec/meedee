"""File record fidelity and bounded streaming independent of MIDI services."""

from io import BytesIO

import pytest

from meedee import clip, messages, smf, syx, ump

SMF = bytes.fromhex(
    "4d546864000000060000000101e04d54726b0000001300ff510307a12000903c6483603c0000ff2f00"
)


def test_smf_fidelity_tracks_and_borrowed_stream_ownership() -> None:
    with pytest.raises(ValueError):
        smf.Header(format=0, tracks=2, division=480)
    with pytest.raises(ValueError):
        smf.Meta(kind=81, data=b"\x00")
    stream = BytesIO(SMF)
    reader = smf.Reader(stream)
    records = list(reader)
    header = reader.header
    assert header.division == 480
    reader.close()
    assert not stream.closed
    with pytest.raises(ValueError, match="closed"):
        next(reader)
    assert [e.tick for e in records] == [0, 0, 480, 480]
    assert records[1].record == messages.NoteOn(channel=1, note=60, velocity=100)
    assert records[0].record == smf.Meta(kind=81, data=b"\x07\xa1\x20")
    output = BytesIO()
    writer = smf.Writer(output, header=header)
    for e in records:
        writer.write(e)
    writer.finish()
    assert not output.closed and not stream.closed
    assert list(smf.Reader(BytesIO(output.getvalue()))) == records
    for cut in range(len(SMF)):
        with pytest.raises(OSError, match="byte"):
            list(smf.Reader(BytesIO(SMF[:cut])))
    with pytest.raises(TypeError):
        records[0].replace(tick=True)
    with pytest.raises(AttributeError):
        records[0].tick = 8  # ty: ignore[invalid-assignment]
    assert records[0].replace(tick=7).tick == 7
    for division in (0xE728, 0xE328, 0xE228):
        assert smf.Header(format=2, tracks=2, division=division).division == division


def test_syx_limits_and_file_records_are_not_implicitly_live_messages() -> None:
    output = BytesIO()
    syx.write(output, [messages.SysEx(data=b"\x7d\x01"), messages.SysEx(data=b"")])
    assert output.getvalue() == b"\xf0\x7d\x01\xf7\xf0\xf7"
    assert len(list(syx.Reader(BytesIO(output.getvalue())))) == 2
    with pytest.raises(OSError, match="limit"):
        list(syx.Reader(BytesIO(output.getvalue()), payload_limit=1))
    with pytest.raises(TypeError):
        syx.write(BytesIO(), [smf.Meta(kind=47, data=b"")])  # ty: ignore[invalid-argument-type]
    fragment = smf.SysExFragment(status=247, data=b"\x90\xff\x00")
    assert fragment.data == b"\x90\xff\x00"
    assert not isinstance(fragment, messages.Message)


def test_clip_preserves_big_endian_words_and_requires_completion() -> None:
    packets = [
        ump.DeltaClockstamp(ticks=0),
        ump.DeltaClockstampTicksPerQuarter(ticks=480),
        ump.DeltaClockstamp(ticks=0),
        ump.start_of_clip(),
        ump.DeltaClockstamp(ticks=1),
        ump.Midi1Packet(
            group=1, message=messages.NoteOn(channel=1, note=60, velocity=100)
        ),
        ump.DeltaClockstamp(ticks=0),
        ump.end_of_clip(),
    ]
    stream = BytesIO()
    writer = clip.Writer(stream)
    for p in packets:
        writer.write(p)
    writer.finish()
    assert stream.getvalue().startswith(b"SMF2CLIP\x00\x40\x00\x00\x00\x30\x01\xe0")
    entries = list(clip.Reader(BytesIO(stream.getvalue())))
    assert [e.message for e in entries] == packets
    assert entries[5].tick == 1 and entries[5].sequence
    with pytest.raises(OSError, match="EndOfClip"):
        list(clip.Reader(BytesIO(stream.getvalue()[:-16])))


class ShortWriter(BytesIO):
    def write(self, data: bytes) -> int:  # ty: ignore[invalid-method-override]
        return super().write(data[:1])


class FullDisk(BytesIO):
    def write(self, data: bytes) -> int:  # ty: ignore[invalid-method-override]
        raise OSError(28, "disk full")


def test_short_writes_are_completed_and_disk_failures_are_reported() -> None:
    output = ShortWriter()
    syx.write(output, [messages.SysEx(data=b"\x7d")])
    assert output.getvalue() == b"\xf0\x7d\xf7"
    with pytest.raises(OSError, match="disk full"):
        syx.write(FullDisk(), [messages.SysEx(data=b"\x7d")])
