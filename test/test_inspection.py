"""Read-only reports distinguish file completion from capture quality."""

from io import BytesIO
from pathlib import Path

import pytest

from meedee import archive, events, inspection, messages, smf, ump


def capture(*, complete: bool = True, opaque: bool = False) -> bytes:
    output = BytesIO()
    writer = archive.Writer(output)
    clock = events.Clock(identity="native", origin="test")
    writer.write(clock)
    writer.write(archive.Stream(id="keys", label="Keyboard", clock_identity="native"))
    writer.write(events.Event(stream="keys", message=messages.TimingClock()))
    writer.write(
        events.Event(
            stream="keys",
            timestamp=events.Timestamp(ticks=-1, clock=clock),
            message=ump.Midi2ControlChange(
                group=16, channel=16, controller=7, value=0xFEDCBA98
            ),
        )
    )
    writer.write(archive.Gap(stream="keys", lost=3, reason="queue overflow"))
    writer.write(archive.Discontinuity(stream="keys", reason="reset"))
    if opaque:
        writer.write(archive.Opaque(kind=100, data=b"unknown"))
    writer.finish(complete=complete, reason="" if complete else "lost input")
    return output.getvalue()


def test_archive_completion_is_distinct_from_capture_loss() -> None:
    source = BytesIO(capture())
    report = inspection.inspect_file(source, format="archive")
    assert not source.closed
    assert report.complete and report.problem is None
    assert report.records == 6
    assert report.message_counts == {"TimingClock": 1, "UMP.Midi2ChannelVoice": 1}
    assert report.streams[0].id == "keys"
    assert report.streams[0].label == "Keyboard"
    assert report.streams[0].clock_identity == report.clocks[0].identity == "native"
    assert report.clocks[0].tick_denominator == 1_000_000_000
    assert report.gap_records == 1
    assert report.lost_events == 3
    assert report.discontinuities == 1
    assert report.untimed_events == 1
    report.message_counts.clear()
    report.clocks.clear()
    report.streams.clear()
    assert report.message_counts["TimingClock"] == 1
    assert len(report.clocks) == len(report.streams) == 1
    with pytest.raises(AttributeError):
        report.complete = False  # ty: ignore[invalid-assignment]


def test_incomplete_corrupt_and_unknown_archives_keep_prefix_reports() -> None:
    incomplete = inspection.inspect_file(
        BytesIO(capture(complete=False)), format="archive"
    )
    assert not incomplete.complete and incomplete.records == 6
    assert "lost input" in str(incomplete.problem)
    data = capture()
    corrupt = bytearray(data)
    corrupt[20] ^= 1
    report = inspection.inspect_file(BytesIO(corrupt), format="archive")
    assert not report.complete and report.records == 0
    assert "CRC" in str(report.problem)
    for cut in range(len(data)):
        report = inspection.inspect_file(BytesIO(data[:cut]), format="archive")
        assert not report.complete and report.problem
    data = capture(opaque=True)
    report = inspection.inspect_file(BytesIO(data), format="archive")
    assert not report.complete and "unknown record" in str(report.problem)
    report = inspection.inspect_file(
        BytesIO(data), format="archive", preserve_unknown=True
    )
    assert report.complete and report.opaque_records == 1 and report.records == 7


def test_smf_tracks_and_fragments_retain_file_semantics(tmp_path: Path) -> None:
    path = tmp_path / "recording.mid"
    writer = smf.Writer(path, header=smf.Header(format=2, tracks=2, division=480))
    writer.write(
        smf.Event(
            track=0, tick=0, record=messages.NoteOn(channel=1, note=60, velocity=0)
        )
    )
    writer.write(smf.Event(track=0, tick=0, record=smf.Meta(kind=47, data=b"")))
    writer.write(
        smf.Event(
            track=1, tick=0, record=smf.SysExFragment(status=247, data=b"\x90\xff")
        )
    )
    writer.write(smf.Event(track=1, tick=2, record=smf.Meta(kind=47, data=b"")))
    writer.finish()
    before = path.read_bytes()
    report = inspection.inspect_file(path, format="smf")
    assert path.read_bytes() == before
    assert report.complete and report.records == 4
    assert report.smf_header is not None
    assert report.smf_header.format == 2 and report.smf_header.division == 480
    assert report.track_records == {0: 2, 1: 2}
    assert report.message_counts == {"NoteOn": 1, "SysExFragment": 1}
    assert not report.clocks and not report.streams
    unknown = before[:14] + b"JUNK\x00\x00\x00\x02xy" + before[14:]
    report = inspection.inspect_file(BytesIO(unknown), format="smf")
    assert report.complete and report.opaque_records == 1 and report.records == 5
    assert report.track_records == {0: 2, 1: 2}


def test_clip_completion_and_syx_limits_use_existing_readers() -> None:
    # Independent standard-file bytes, including DCS/DCTPQ and clip markers.
    data = bytes.fromhex(
        "534d4632434c4950 00400000 003001e0 00400000"
        "f0200000 00000000 00000000 00000000"
        "00400001 20903c64 00400000"
        "f0210000 00000000 00000000 00000000"
    )
    report = inspection.inspect_file(BytesIO(data), format="clip")
    assert report.complete and report.records == 8
    assert report.message_counts["UMP.Midi1ChannelVoice"] == 1
    assert report.message_counts["UMP.Stream"] == 2
    assert report.untimed_events == 0
    unknown = data[:40] + bytes.fromhex("60000000") + data[44:]
    report = inspection.inspect_file(BytesIO(unknown), format="clip")
    assert not report.complete and report.records == 5
    report = inspection.inspect_file(
        BytesIO(unknown), format="clip", preserve_unknown=True
    )
    assert report.complete and report.opaque_records == 1
    assert report.message_counts["UMP.Opaque"] == 1
    report = inspection.inspect_file(BytesIO(data[:-16]), format="clip")
    assert not report.complete and "EndOfClip" in str(report.problem)
    report = inspection.inspect_file(BytesIO(b"\xf0\x7d\xf7\xf0\x01"), format="syx")
    assert not report.complete and report.records == 1 and report.untimed_events == 1
    assert "byte 5" in str(report.problem)
    report = inspection.inspect_file(
        BytesIO(b"\xf0\x01\x02\xf7"), format="syx", payload_limit=1
    )
    assert not report.complete and "limit" in str(report.problem)


def test_argument_and_file_access_errors_are_not_malformed_file_reports(
    tmp_path: Path,
) -> None:
    with pytest.raises(OSError):
        inspection.inspect_file(tmp_path / "missing", format="archive")
    with pytest.raises(ValueError, match="format"):
        inspection.inspect_file(BytesIO(), format="auto")  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError, match="positive"):
        inspection.inspect_file(BytesIO(), format="syx", payload_limit=0)
    with pytest.raises(TypeError):
        inspection.inspect_file(BytesIO(), format="syx", payload_limit=True)

    class FailedDisk(BytesIO):
        def read(self, size: int | None = -1, /) -> bytes:
            raise OSError("read denied")

    with pytest.raises(OSError, match="read denied"):
        inspection.inspect_file(FailedDisk(), format="archive")


def test_timing_segments_preserve_exact_ranges_and_reset_after_loss() -> None:
    output = BytesIO()
    writer = archive.Writer(output)
    clock = events.Clock(identity="c")
    writer.write(clock)
    writer.write(archive.Stream(id="s", label="", clock_identity="c"))
    for t in [5, 3, None]:
        writer.write(
            events.Event(
                stream="s",
                message=messages.TimingClock(),
                timestamp=None if t is None else events.Timestamp(ticks=t, clock=clock),
            )
        )
    writer.write(archive.Gap(stream="s", lost=1, reason="loss"))
    writer.write(
        events.Event(
            stream="s",
            message=messages.TimingClock(),
            timestamp=events.Timestamp(ticks=-100, clock=clock),
        )
    )
    writer.finish()
    report = inspection.inspect_file(
        BytesIO(output.getvalue()), format="archive", detail_limit=1
    )
    assert report.complete and report.timestamp_regressions == 1
    assert report.source_counts == {"s": 4}
    segment = report.timing_segments[0]
    assert segment.first == 5 and segment.last == 3
    assert segment.minimum == 3 and segment.maximum == 5
    assert segment.untimed == 1 and segment.regressions == 1
    assert report.omitted_segments == 1 and report.omitted_diagnostics == 2
    assert report.timing_diagnostics[0].kind == "timestamp_regression"
