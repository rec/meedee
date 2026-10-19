"""Note observations retain protocol meaning without claiming device voices."""

import pytest

from meedee import events, messages, midi, notes, ump


def event(
    message: messages.Message, *, stream: str = "s", clock: str = "c"
) -> events.Event:
    return events.Event(
        stream=stream,
        message=message,
        timestamp=events.Timestamp(ticks=0, clock=events.Clock(identity=clock)),
    )


def test_repeated_notes_count_releases_without_pairing_instances() -> None:
    tracker = notes.Tracker(stream="s")
    on = messages.NoteOn(channel=1, note=60, velocity=100)
    tracker.feed(event(on))
    tracker.feed(event(on))
    before = tracker.snapshot()
    tracker.feed(event(on.replace(velocity=0)))
    channel = tracker.snapshot().channels[0]
    assert channel.notes[0].held == channel.notes[0].released == 1
    assert before.channels[0].notes[0].held == 2
    tracker.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))
    tracker.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))
    channel = tracker.snapshot().channels[0]
    assert channel.notes[0].held == 0 and channel.notes[0].released == 2
    assert channel.unmatched_releases == 1 and channel.uncertain
    before.channels.clear()
    assert len(before.channels) == 1
    with pytest.raises(AttributeError):
        channel.notes[0].held = 2  # ty: ignore[invalid-assignment]
    with pytest.raises(ValueError, match="one source"):
        tracker.feed(event(on, stream="other"))
    assert tracker.snapshot().channels[0].unmatched_releases == 1


def test_ump_groups_and_protocols_preserve_zero_velocity_note_on_meaning() -> None:
    tracker = notes.Tracker(stream="s")
    on = messages.NoteOn(channel=1, note=60, velocity=0)
    tracker.feed(event(on))
    tracker.feed(event(ump.Midi1Packet(group=16, message=on)))
    tracker.feed(
        event(
            ump.Midi2NoteOn(
                group=16,
                channel=1,
                note=60,
                velocity=0,
                attribute_type=0,
                attribute_data=0,
            )
        )
    )
    channels = tracker.snapshot().channels
    assert len(channels) == 3
    assert channels[0].group is None and channels[0].unmatched_releases == 1
    assert channels[1].protocol == "midi1" and channels[1].unmatched_releases == 1
    assert channels[2].protocol == "midi2" and channels[2].notes[0].held == 1
    assert channels[2].group == 16


def test_pedal_values_and_channel_modes_do_not_invent_sounding_voices() -> None:
    tracker = notes.Tracker(stream="s")
    on = messages.NoteOn(channel=1, note=60, velocity=1)
    tracker.feed(event(on))
    tracker.feed(event(messages.ControlChange(channel=1, controller=64, value=127)))
    tracker.feed(event(messages.ControlChange(channel=1, controller=66, value=64)))
    tracker.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))
    channel = tracker.snapshot().channels[0]
    assert channel.sustain == 127 and channel.sostenuto == 64
    assert channel.notes[0].held == 0 and channel.notes[0].released == 1
    tracker.feed(event(messages.ControlChange(channel=1, controller=121, value=0)))
    channel = tracker.snapshot().channels[0]
    assert channel.sustain is None and channel.sostenuto is None
    assert channel.notes[0].released == 1
    tracker.feed(event(on))
    tracker.feed(event(messages.ControlChange(channel=1, controller=123, value=0)))
    assert tracker.snapshot().channels[0].notes[0].released == 2
    tracker.feed(event(on))
    tracker.feed(event(messages.ControlChange(channel=1, controller=120, value=0)))
    channel = tracker.snapshot().channels[0]
    assert channel.notes[0].held == 0 and channel.notes[0].released == 2
    tracker.feed(
        event(
            ump.Midi2ControlChange(group=1, channel=1, controller=64, value=0xFEDCBA98)
        )
    )
    assert tracker.snapshot().channels[1].sustain == 0xFEDCBA98


def test_loss_and_clock_changes_invalidate_only_affected_history() -> None:
    tracker = notes.Tracker(stream="s")
    on = messages.NoteOn(channel=1, note=60, velocity=1)
    tracker.feed(event(on))
    tracker.feed(event(on, clock="other"))
    channel = tracker.snapshot().channels[0]
    assert channel.uncertain and channel.notes[0].held == 1
    tracker.invalidate()
    assert tracker.snapshot().channels[0].notes == []
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    tracker.feed(decoder.loss(lost=1, reason="known loss"))
    tracker.feed(event(on.replace(channel=2)))
    assert tracker.snapshot().channels[1].uncertain
    assert tracker.snapshot().boundaries == 3
    tracker.feed(event(messages.SystemReset()))
    assert tracker.snapshot().channels[1].notes == []
    assert tracker.snapshot().boundaries == 4


def test_capacity_retains_admitted_channels_and_counts_omitted_events() -> None:
    tracker = notes.Tracker(stream="s", detail_limit=1)
    on = messages.NoteOn(channel=1, note=60, velocity=1)
    tracker.feed(event(on))
    tracker.feed(event(on.replace(channel=2)))
    tracker.feed(event(on))
    report = tracker.snapshot()
    assert len(report.channels) == 1 and report.omitted_events == 1
    assert report.channels[0].notes[0].held == 2
    with pytest.raises(ValueError):
        notes.Tracker(stream="s", detail_limit=0)
