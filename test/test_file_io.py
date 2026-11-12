"""The simple file-editing loop preserves the complete file structure."""

from io import BytesIO
from pathlib import Path

import pytest

import meedee as md


def midi_file(path: Path) -> tuple[md.smf.Header, list[md.smf.Event]]:
    header = md.smf.Header(format=1, tracks=2, division=480, extra=b"xy")
    records = [
        md.smf.Event(
            track=0, tick=0, record=md.smf.UnknownChunk(id=b"JUNK", data=b"xy")
        ),
        md.smf.Event(
            track=0, tick=0, record=md.smf.Meta(kind=81, data=b"\x07\xa1\x20")
        ),
        md.smf.Event(track=0, tick=0, record=md.NoteOn(120, channel=1)),
        md.smf.Event(track=0, tick=480, record=md.smf.Meta(kind=47, data=b"")),
        md.smf.Event(track=1, tick=0, record=md.NoteOn(60, channel=10)),
        md.smf.Event(
            track=1, tick=1, record=md.smf.SysExFragment(status=240, data=b"\x7d\x01")
        ),
        md.smf.Event(
            track=1, tick=2, record=md.smf.SysExFragment(status=247, data=b"\x02\xf7")
        ),
        md.smf.Event(track=1, tick=480, record=md.smf.Meta(kind=47, data=b"")),
    ]
    with md.Writer(path) as writer:
        writer.write(header)
        for r in records:
            writer.write(r)
    return header, records


def test_transpose_loop_preserves_header_tracks_tempo_and_fragments(
    tmp_path: Path,
) -> None:
    source = tmp_path / "song.mid"
    header, records = midi_file(source)
    destination = tmp_path / "song-up.mid"
    with md.Writer(destination) as writer:
        for m in md.Reader(source):
            if getattr(m, "channel", 10) != 10:
                m = m.transpose(12)
            writer.write(m)
    with md.Reader(destination) as reader:
        transformed = list(reader)
        assert reader.complete
        assert reader.problem is None
    assert transformed[0] == header
    records[2] = records[2].transpose(12)
    assert records[2].record == md.NoteOn(127, channel=1)
    assert transformed[1:] == records
    assert source.read_bytes() != destination.read_bytes()
    before = destination.read_bytes()
    with pytest.raises(OSError):
        md.Writer(destination)
    assert destination.read_bytes() == before


def test_detection_uses_contents_and_borrowed_streams_remain_open(
    tmp_path: Path,
) -> None:
    path = tmp_path / "song.mid"
    header, records = midi_file(path)
    renamed = tmp_path / "not-an-archive.meedee"
    renamed.write_bytes(path.read_bytes())
    assert list(md.Reader(renamed)) == [header, *records]
    stream = BytesIO(path.read_bytes())
    with md.Reader(stream) as reader:
        assert list(reader) == [header, *records]
    assert not stream.closed
    with pytest.raises(ValueError, match="closed"):
        next(reader)
    output = BytesIO()
    with md.Writer(output) as writer:
        writer.write(md.archive.Metadata(key="title", data=b"song"))
    assert not output.closed
    with md.Reader(BytesIO(output.getvalue())) as archive:
        assert list(archive) == [md.archive.Metadata(key="title", data=b"song")]
        assert archive.complete
    with pytest.raises(ValueError, match="closed"):
        writer.write(md.archive.Metadata(key="title", data=b"song"))
    report = md.Output().send(md.Reader(BytesIO(output.getvalue())))
    assert report.sent_events == 0


def test_invalid_or_incomplete_file_is_reported_without_finalizing(
    tmp_path: Path,
) -> None:
    source = tmp_path / "song.mid"
    header, records = midi_file(source)
    reader = md.Reader(BytesIO(source.read_bytes()[:-1]))
    with pytest.raises(OSError):
        list(reader)
    assert not reader.complete and reader.problem is not None
    with pytest.raises(OSError, match="header"):
        with md.Writer(tmp_path / "missing-header.mid") as writer:
            writer.write(records[0])
    with pytest.raises(OSError, match="track count"):
        with md.Writer(tmp_path / "incomplete.mid") as writer:
            writer.write(header)
    with pytest.raises(ValueError, match="destination"):
        md.Writer(tmp_path / "wrong.txt")
    assert not (tmp_path / "wrong.txt").exists()
    with pytest.raises(RuntimeError, match="editing failed"):
        with md.Writer(tmp_path / "interrupted.mid") as writer:
            raise RuntimeError("editing failed")
    with pytest.raises(ValueError, match="closed"):
        writer.write(header)


@pytest.mark.parametrize(
    "message",
    [
        md.NoteOn(120, channel=1),
        md.NoteOff(120, channel=1),
        md.PolyPressure(120, 70, channel=1),
        md.ControlChange(1, 70, channel=1),
    ],
)
def test_file_event_transpose_matches_message_transform(message: md.Message) -> None:
    event = md.smf.Event(track=2, tick=57, record=message)
    assert event.channel == 1
    assert event.transpose(12) == event.replace(record=message.transpose(12))
    assert event.record == message
    with pytest.raises(TypeError):
        event.transpose(True)


def test_syx_writer_accepts_structured_messages_and_reader_detects_short_streams(
    tmp_path: Path,
) -> None:
    path = tmp_path / "identity.syx"
    request = md.universal.IdentityRequest(device_id=127)
    with md.Writer(path) as writer:
        writer.write(request)
        writer.write(md.SysEx(data=b""))
    expected = md.codecs.encode(request) + b"\xf0\xf7"
    assert path.read_bytes() == expected
    with md.Reader(BytesIO(expected)) as reader:
        result = list(reader)
        assert reader.complete and reader.problem is None
    assert [md.codecs.encode(m) for m in result] == [
        md.codecs.encode(request),
        b"\xf0\xf7",
    ]
    with md.Reader(BytesIO(b"\xf0\xf7")) as reader:
        assert list(reader) == [md.SysEx(data=b"")]
        assert reader.complete
    with md.Reader(BytesIO()) as reader:
        assert list(reader) == [] and reader.complete


def test_syx_stream_writer_preserves_ownership_limits_and_format_errors() -> None:
    stream = BytesIO()
    with md.Writer(stream, format="syx", payload_limit=1) as writer:
        writer.write(md.SysEx(data=b"\x7d"))
        with pytest.raises(OSError, match="limit"):
            writer.write(md.SysEx(data=b"\x7d\x01"))
        with pytest.raises(OSError, match="SysEx"):
            writer.write(md.NoteOn(60, channel=1))
        with pytest.raises(OSError, match="archives"):
            writer.finish(complete=False)
    assert not stream.closed and stream.getvalue() == b"\xf0\x7d\xf7"
    with md.Reader(BytesIO(b"\xf0\x7d\xf7\xf0\x01")) as reader:
        assert next(reader) == md.SysEx(data=b"\x7d")
        with pytest.raises(OSError):
            next(reader)
        assert not reader.complete and reader.problem


def test_tuning_dump_writes_directly_without_opaque_wrapping() -> None:
    values = [
        md.NoteTuning(
            note=69, pitch=md.tuning.Pitch.from_frequency(frequency_hz=440).pitch
        )
    ]
    dump = md.tuning.bulk_dump(
        values, device_id=0, program=2, name="A", preserve_unmapped=True
    )
    stream = BytesIO()
    with md.Writer(stream, format="syx") as writer:
        writer.write(dump)
    assert stream.getvalue() == md.codecs.encode(dump)
