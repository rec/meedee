"""Same-format comparisons preserve records and require explicit timing policy."""

from io import BytesIO

import pytest

from meedee import archive, clip, events, inspection, messages, smf, ump


def take(source: str, clock_id: str, tick: int) -> BytesIO:
    out = BytesIO()
    writer = archive.Writer(out)
    clock = events.Clock(identity=clock_id)
    timestamp = events.Timestamp(ticks=tick, clock=clock)
    writer.write(clock)
    writer.write(archive.Stream(id=source, label="keys", clock_identity=clock_id))
    writer.write(
        events.Event(
            stream=source,
            message=messages.NoteOn(channel=1, note=60, velocity=100),
            timestamp=timestamp,
            span=events.SourceSpan(
                first_packet=0,
                last_packet=0,
                first_timestamp=timestamp,
                last_timestamp=timestamp,
            ),
        )
    )
    writer.write(
        archive.Metadata(key="marker", data=b"x", stream=source, timestamp=timestamp)
    )
    writer.finish()
    out.seek(0)
    return out


def test_archive_mapping_and_tolerance_cover_spans_and_metadata() -> None:
    report = inspection.compare_files(
        take("a", "ca", 10), take("b", "cb", 11), format="archive"
    )
    assert not report.equal and report.differences == 4
    report = inspection.compare_files(
        take("a", "ca", 10),
        take("b", "cb", 11),
        format="archive",
        source_map={"a": "b"},
        clock_map={"ca": "cb"},
    )
    assert report.differences == 2
    report = inspection.compare_files(
        take("a", "ca", 10),
        take("b", "cb", 11),
        format="archive",
        tick_tolerance=1,
        source_map={"a": "b"},
        clock_map={"ca": "cb"},
    )
    assert report.equal and report.left_records == report.right_records == 4
    report = inspection.compare_files(
        take("a", "ca", -100),
        take("b", "cb", 100),
        format="archive",
        ignore_timestamps=True,
        source_map={"a": "b"},
        clock_map={"ca": "cb"},
    )
    assert report.equal
    with pytest.raises(ValueError, match="one-to-one"):
        inspection.compare_files(
            take("a", "ca", 0),
            take("b", "cb", 0),
            format="archive",
            source_map={"a": "b", "c": "b"},
        )


def test_differences_are_bounded_but_both_trailers_are_checked() -> None:
    a = BytesIO(b"\xf0\x01\xf7" * 4)
    b = BytesIO(b"\xf0\x02\xf7" * 3)
    report = inspection.compare_files(a, b, format="syx", detail_limit=1)
    assert report.differences == 4 and len(report.examples) == 1
    assert report.examples[0].record == 0
    assert report.left_complete and report.right_complete
    assert not a.closed and not b.closed
    report.examples.clear()
    assert len(report.examples) == 1
    a = take("s", "c", 0).getvalue()
    report = inspection.compare_files(BytesIO(a[:-1]), BytesIO(a), format="archive")
    assert not report.equal and not report.left_complete and report.right_complete
    assert "truncated" in str(report.left_problem)
    report = inspection.compare_files(
        BytesIO(b"bad"), BytesIO(b"bad"), format="archive"
    )
    assert not report.equal and not report.left_complete and not report.right_complete


def test_smf_header_metadata_and_record_order_remain_significant() -> None:
    def score(tick: int, division: int = 480) -> BytesIO:
        out = BytesIO()
        writer = smf.Writer(
            out, header=smf.Header(format=0, tracks=1, division=division)
        )
        writer.write(
            smf.Event(track=0, tick=tick, record=smf.Meta(kind=1, data=b"text"))
        )
        writer.write(smf.Event(track=0, tick=tick, record=smf.Meta(kind=47, data=b"")))
        writer.finish()
        out.seek(0)
        return out

    assert inspection.compare_files(
        score(10), score(11), format="smf", tick_tolerance=1
    ).equal
    report = inspection.compare_files(score(10), score(10, 960), format="smf")
    assert report.differences == 1 and report.examples[0].reason == "header"
    assert report.examples[0].record is None
    with pytest.raises(ValueError, match="only to archives"):
        inspection.compare_files(
            score(0), score(0), format="smf", source_map={"a": "b"}
        )
    with pytest.raises(ValueError, match="tolerance"):
        inspection.compare_files(
            score(0), score(0), format="smf", ignore_timestamps=True, tick_tolerance=1
        )


def test_clip_tolerance_compares_cumulative_ticks_and_retains_configuration() -> None:
    def score(tick: int) -> BytesIO:
        out = BytesIO()
        writer = clip.Writer(out)
        for p in [
            ump.DeltaClockstamp(ticks=0),
            ump.DeltaClockstampTicksPerQuarter(ticks=480),
            ump.DeltaClockstamp(ticks=0),
            ump.start_of_clip(),
            ump.DeltaClockstamp(ticks=tick),
            ump.Midi1Packet(group=1, message=messages.TimingClock()),
            ump.DeltaClockstamp(ticks=0),
            ump.end_of_clip(),
        ]:
            writer.write(p)
        writer.finish()
        out.seek(0)
        return out

    assert not inspection.compare_files(score(10), score(11), format="clip").equal
    assert inspection.compare_files(
        score(10), score(11), format="clip", tick_tolerance=1
    ).equal
