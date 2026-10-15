"""Standard files retain exact tempo/SMPTE time and require deliberate exports."""

from io import BytesIO

import pytest

from meedee import clip, events, messages, smf, ump


def song(format: int = 1, division: int = 480) -> bytes:
    stream = BytesIO()
    writer = smf.Writer(
        stream, header=smf.Header(format=format, tracks=2, division=division)
    )
    for value in [
        smf.Event(track=0, tick=0, record=smf.Meta(kind=81, data=b"\x07\xa1\x20")),
        smf.Event(track=0, tick=240, record=smf.Meta(kind=81, data=b"\x03\xd0\x90")),
        smf.Event(track=0, tick=480, record=smf.Meta(kind=47, data=b"")),
        smf.Event(
            track=1, tick=240, record=messages.NoteOn(channel=1, note=60, velocity=100)
        ),
        smf.Event(
            track=1, tick=480, record=messages.NoteOn(channel=1, note=61, velocity=100)
        ),
        smf.Event(track=1, tick=480, record=smf.Meta(kind=47, data=b"")),
    ]:
        writer.write(value)
    writer.finish()
    return stream.getvalue()


def test_merged_tempo_map_is_exact_and_keeps_leading_silence() -> None:
    reader = smf.TimedReader(BytesIO(song()), stream="song")
    origin = reader.origin
    values = list(reader)
    assert len(values) == 2 and all(isinstance(v, events.Event) for v in values)
    first = values[0]
    assert isinstance(first, events.Event) and first.timestamp is not None
    assert first.timestamp.seconds == events.Rational(numerator=1, denominator=4)
    second = values[1]
    assert isinstance(second, events.Event) and second.timestamp is not None
    assert second.timestamp.seconds == events.Rational(numerator=3, denominator=8)
    assert events.PlaybackPlan(origin=origin).deadline(first).delay_ns == 250_000_000
    merged = list(smf.MergedReader(BytesIO(song())))
    assert [(v.tick, v.track) for v in merged] == [
        (0, 0),
        (240, 0),
        (240, 1),
        (480, 0),
        (480, 1),
        (480, 1),
    ]
    with pytest.raises(OSError, match="track merge limit"):
        smf.MergedReader(BytesIO(song()), track_limit=1)
    assert smf.TimedReader(BytesIO(song()), stream="song").clock != reader.clock


def test_independent_sequences_and_smpte_export_have_explicit_loss_reports() -> None:
    data = song(format=2, division=0xE328)
    with pytest.raises(OSError, match="sequence"):
        smf.TimedReader(BytesIO(data), stream="s")
    reader = smf.TimedReader(BytesIO(data), stream="s", sequence=1)
    values = list(reader)
    value = values[0]
    assert isinstance(value, events.Event)
    header = smf.Header(format=0, tracks=1, division=480)
    denied = smf.export_event(value, header=header, origin=reader.origin)
    assert denied.status == ump.TranslationStatus.Quantized and denied.events == []
    accepted = smf.export_event(
        value, header=header, origin=reader.origin, allow_loss=True
    )
    assert accepted.events and accepted.rounding_error_ticks.numerator != 0
    wide = value.replace(
        message=ump.Midi2NoteOn(
            group=2,
            channel=1,
            note=60,
            velocity=65535,
            attribute_type=0,
            attribute_data=0,
        )
    )
    assert (
        smf.export_event(
            wide, header=header, origin=reader.origin, allow_loss=True
        ).status
        == ump.TranslationStatus.ContextRequired
    )


def test_clip_timed_reader_omits_file_markers_and_preserves_tempo() -> None:
    stream = BytesIO()
    writer = clip.Writer(stream)
    for value in [
        ump.DeltaClockstamp(ticks=0),
        ump.DeltaClockstampTicksPerQuarter(ticks=480),
        ump.DeltaClockstamp(ticks=0),
        ump.start_of_clip(),
        ump.tempo(group=1, ten_nanoseconds_per_quarter=50_000_000),
        ump.DeltaClockstamp(ticks=480),
        ump.Midi2NoteOn(
            group=1,
            channel=1,
            note=60,
            velocity=65535,
            attribute_type=0,
            attribute_data=0,
        ),
        ump.DeltaClockstamp(ticks=0),
        ump.end_of_clip(),
    ]:
        writer.write(value)
    writer.finish()
    reader = clip.TimedReader(BytesIO(stream.getvalue()), stream="clip")
    values = list(reader)
    assert len(values) == 2 and all(isinstance(v, events.Event) for v in values)
    note = values[1]
    assert isinstance(note, events.Event) and note.timestamp is not None
    assert note.timestamp.seconds == events.Rational(numerator=1, denominator=2)
    assert (
        events.PlaybackPlan(origin=reader.origin).deadline(note).delay_ns == 500_000_000
    )


def test_split_sysex_is_assembled_per_track_when_other_tracks_interleave() -> None:
    stream = BytesIO()
    writer = smf.Writer(stream, header=smf.Header(format=1, tracks=2, division=480))
    for record in [
        smf.Event(
            track=0, tick=0, record=smf.SysExFragment(status=240, data=b"\x7d\x01")
        ),
        smf.Event(
            track=0, tick=2, record=smf.SysExFragment(status=247, data=b"\x02\xf7")
        ),
        smf.Event(track=0, tick=2, record=smf.Meta(kind=47, data=b"")),
        smf.Event(
            track=1, tick=1, record=messages.NoteOn(channel=1, note=60, velocity=1)
        ),
        smf.Event(track=1, tick=2, record=smf.Meta(kind=47, data=b"")),
    ]:
        writer.write(record)
    writer.finish()
    values = list(smf.TimedReader(BytesIO(stream.getvalue()), stream="s"))
    assert len(values) == 2
    assert [v.message for v in values if isinstance(v, events.Event)] == [
        messages.NoteOn(channel=1, note=60, velocity=1),
        messages.SysEx(data=b"\x7d\x01\x02"),
    ]
    sysex = values[1]
    assert isinstance(sysex, events.Event) and sysex.span is not None
    assert sysex.span.first_packet == 0 and sysex.span.last_packet == 2
