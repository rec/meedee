"""File notes remain source-local, bounded and independent of completeness."""

from io import BytesIO

from meedee import archive, events, inspection, messages, smf


def test_archive_loss_and_incomplete_tail_keep_only_observed_state() -> None:
    data = BytesIO()
    writer = archive.Writer(data)
    for s in ["a", "b"]:
        writer.write(archive.Stream(id=s, label=""))
    on = messages.NoteOn(channel=1, note=60, velocity=100)
    writer.write(events.Event(stream="a", message=on))
    writer.write(events.Event(stream="b", message=on))
    writer.write(archive.Gap(stream="a", lost=1, reason="loss"))
    writer.write(events.Event(stream="a", message=on))
    writer.finish()
    report = inspection.inspect_file(BytesIO(data.getvalue()), format="archive")
    a, b = report.note_state.channels
    assert report.complete and a.uncertain and not b.uncertain
    assert a.notes[0].held == b.notes[0].held == 1
    assert report.note_state.boundaries == 1
    prefix = inspection.inspect_file(BytesIO(data.getvalue()[:-1]), format="archive")
    assert not prefix.complete
    assert prefix.note_state.channels[0].notes[0].held == 1
    limited = inspection.inspect_file(
        BytesIO(data.getvalue()), format="archive", detail_limit=1
    )
    assert limited.complete and len(limited.note_state.channels) == 1
    assert limited.note_state.omitted_events == 1


def test_smf_tracks_do_not_release_each_others_notes() -> None:
    data = BytesIO()
    writer = smf.Writer(data, header=smf.Header(format=1, tracks=2, division=480))
    writer.write(
        smf.Event(
            track=0, tick=0, record=messages.NoteOn(channel=1, note=60, velocity=1)
        )
    )
    writer.write(smf.Event(track=0, tick=1, record=smf.Meta(kind=47, data=b"")))
    writer.write(
        smf.Event(
            track=1, tick=0, record=messages.NoteOff(channel=1, note=60, velocity=0)
        )
    )
    writer.write(smf.Event(track=1, tick=1, record=smf.Meta(kind=47, data=b"")))
    writer.finish()
    report = inspection.inspect_file(BytesIO(data.getvalue()), format="smf")
    a, b = report.note_state.channels
    assert a.source == "track:0" and b.source == "track:1"
    assert a.notes[0].held == 1 and b.notes == []
    assert b.unmatched_releases == 1 and report.complete
