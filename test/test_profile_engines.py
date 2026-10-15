"""Profile wire evidence and bounded deterministic interpretation without devices."""

from pathlib import Path

import pytest

from meedee import codecs, controls, gm, messages, mpe, timecode


def test_extended_sysex_preserves_unknown_ids_order_and_mixed_endianness() -> None:
    values = [
        (
            gm.ControllerDestination(
                device_id=127,
                channel=7,
                source=1,
                controller=None,
                entries=[(0, 66), (1, 96), (5, 32)],
            ),
            "f0 7f 7f 09 01 06 00 42 01 60 05 20 f7",
        ),
        (
            gm.ControllerDestination(
                device_id=1, channel=16, source=3, controller=74, entries=[(127, 1)]
            ),
            "f0 7f 01 09 03 0f 4a 7f 01 f7",
        ),
        (
            gm.KeyBasedControl(
                device_id=1, channel=10, note=36, entries=[(7, 64), (120, 65)]
            ),
            "f0 7f 01 0a 01 09 24 07 40 78 41 f7",
        ),
        (
            gm.GlobalParameterControl(
                device_id=1,
                slot_path=[257],
                parameter_width=2,
                value_width=2,
                entries=[((1, 2), (3, 4))],
            ),
            "f0 7f 01 04 05 01 02 02 02 01 01 02 03 04 f7",
        ),
    ]
    fixture = (
        Path(__file__).resolve().parents[1]
        / "crates/meedee/tests/fixtures/profile-engines.txt"
    )
    expected_values = [
        bytes.fromhex(x.split("|", 1)[1])
        for x in fixture.read_text().splitlines()
        if "|" in x
    ]
    for (v, wire), expected in zip(values, expected_values[:4], strict=True):
        assert bytes.fromhex(wire) == expected
        assert isinstance(v, messages.Message)
        assert codecs.encode(v) == expected
        assert gm.decode(messages.SysEx(data=expected[1:-1])) == v
        assert v.replace() == v
        assert hash(v.replace()) == hash(v)
        with pytest.raises(AttributeError):
            v.device_id = 0  # ty: ignore[invalid-assignment]
    assert codecs.encode(
        gm.reverb_parameter(device_id=1, parameter=1, value=64)
    ) == bytes.fromhex("f0 7f 01 04 05 01 01 01 02 01 01 40 f7")
    assert gm.chorus_parameter(device_id=1, parameter=4, value=127).slot_path == (258,)
    for wire in (
        "7f 01 09 03 00",
        "7f 01 0a 01 00 3c 07",
        "7f 01 04 05 01 00 01 02 01",
        "7f 01 09 01 10",
    ):
        with pytest.raises(ValueError):
            gm.decode(messages.SysEx(data=bytes.fromhex(wire)))
    with pytest.raises(ValueError):
        gm.KeyBasedControl(device_id=1, channel=1, note=1, entries=[(0, 0)])
    entries = [(1, 2)] * 1025
    with pytest.raises(ValueError):
        gm.KeyBasedControl(device_id=1, channel=1, note=1, entries=entries)
    large = gm.KeyBasedControl(
        device_id=1, channel=1, note=1, entries=entries, max_entries=1025
    )
    assert large.replace() == large
    raw = messages.SysEx(data=codecs.encode(large)[1:-1])
    with pytest.raises(ValueError):
        gm.decode(raw)
    assert gm.decode(raw, max_entries=1025) == large
    with pytest.raises(TypeError):
        gm.decode(raw, max_entries=True)


def test_velocity_prefix_is_one_shot_per_channel_and_zero_note_on_ignores_it() -> None:
    decoder = controls.VelocityInterpreter()
    decoder.feed(messages.ControlChange(channel=1, controller=88, value=127))
    decoder.feed(messages.TimingClock())
    other = decoder.feed(messages.NoteOn(channel=2, note=60, velocity=1))
    assert other is not None and (other.velocity, other.resolution) == (1, 7)
    fine = decoder.feed(messages.NoteOn(channel=1, note=60, velocity=1))
    assert fine is not None and (fine.velocity, fine.resolution) == (255, 14)
    coarse = decoder.feed(messages.NoteOff(channel=1, note=60, velocity=64))
    assert coarse is not None and (coarse.velocity, coarse.resolution) == (64, 7)
    decoder.feed(messages.ControlChange(channel=1, controller=88, value=127))
    off = decoder.feed(messages.NoteOn(channel=1, note=60, velocity=0))
    assert (
        off is not None
        and not off.note_on
        and off.zero_velocity_note_on
        and off.velocity == 0
    )
    decoder.feed(messages.ControlChange(channel=1, controller=88, value=127))
    decoder.reset()
    coarse = decoder.feed(messages.NoteOn(channel=1, note=60, velocity=1))
    assert coarse is not None and coarse.resolution == 7


def test_mpe_allocator_never_steals_and_retains_released_sustained_voices() -> None:
    allocator = mpe.Allocator(zone=mpe.ZoneConfiguration(manager_channel=1, members=2))
    a = allocator.allocate(note=60, velocity=100)
    b = allocator.allocate(note=60, velocity=100)
    assert (a.channel, b.channel) == (2, 3)
    assert a.note_on == messages.NoteOn(channel=2, note=60, velocity=100)
    with pytest.raises(ValueError):
        allocator.allocate(note=64, velocity=100)
    assert allocator.set_sustain(enabled=True).controller == 64
    assert allocator.release(voice_id=a.id, velocity=64).channel == 2
    with pytest.raises(ValueError):
        allocator.allocate(note=64, velocity=100)
    allocator.set_sustain(enabled=False)
    c = allocator.allocate(note=64, velocity=100)
    assert c.channel == 2 and c.id != a.id
    with pytest.raises(ValueError):
        allocator.release(voice_id=a.id, velocity=64)
    allocator.reset()
    assert allocator.allocate(note=60, velocity=100).id > c.id


def test_mpe_tracker_keeps_overlap_and_sustain_separate_from_member_expression() -> (
    None
):
    tracker = mpe.Tracker(zone=mpe.ZoneConfiguration(manager_channel=1, members=2))
    initial = tracker.snapshot(channel=2)
    assert (
        initial.pitch_bend
        is initial.pressure
        is initial.timbre
        is initial.sustain
        is None
    )
    tracker.feed(messages.ControlChange(channel=2, controller=64, value=127))
    assert tracker.snapshot(channel=2).sustain is None
    for n in (60, 64, 60):
        tracker.feed(messages.NoteOn(channel=2, note=n, velocity=100))
    state = tracker.feed(messages.PitchBend(channel=2, value=300))[0]
    assert state.ambiguous and state.pitch_bend == 300
    assert [(n.note, n.held) for n in state.notes] == [(60, 2), (64, 1)]
    tracker.feed(messages.ControlChange(channel=1, controller=64, value=127))
    tracker.feed(messages.NoteOff(channel=2, note=60, velocity=64))
    state = tracker.snapshot(channel=2)
    assert state.sustain is True
    assert [(n.held, n.sustained) for n in state.notes] == [(1, 1), (1, 0)]
    tracker.feed(messages.ControlChange(channel=2, controller=123, value=0))
    state = tracker.snapshot(channel=2)
    assert not state.ambiguous and all(n.held == 0 for n in state.notes)
    tracker.feed(messages.ControlChange(channel=1, controller=64, value=0))
    assert tracker.snapshot(channel=2).notes == []
    tracker.feed(messages.ControlChange(channel=1, controller=121, value=0))
    state = tracker.snapshot(channel=2)
    assert state.pitch_bend is state.pressure is state.timbre is state.sustain is None
    tracker.configure(zone=mpe.ZoneConfiguration(manager_channel=16, members=1))
    assert tracker.feed(messages.NoteOn(channel=2, note=60, velocity=100)) == []
    assert tracker.feed(messages.NoteOn(channel=15, note=60, velocity=100))[0].notes
    tracker.feed(messages.SystemReset())
    assert tracker.snapshot(channel=15).notes == []


def quarter_cycle(frames: int, reverse: bool = False) -> list[messages.QuarterFrame]:
    values = [frames & 15, frames >> 4, 0, 0, 0, 0, 0, 6]
    order = range(7, -1, -1) if reverse else range(8)
    return [messages.QuarterFrame(message_type=i, value=values[i]) for i in order]


def test_mtc_lock_requires_continuity_and_expires_at_the_deadline() -> None:
    assembler = timecode.Assembler(timeout_ns=100)
    now = 0
    for frame in (0, 2):
        result = None
        for q in quarter_cycle(frame):
            result = assembler.feed(q, now_ns=now)
            now += 1
        assert result is not None and result.observation is not None
        assert result.observation.encoded.frames == frame
        assert result.observation.compensated.frames == frame + 2
        assert result.observation.locked == (frame == 2)
    assert assembler.locked
    assert assembler.poll(now_ns=114) is None
    assert assembler.poll(now_ns=115) == timecode.AssemblyDiagnostic.Timeout
    assert not assembler.locked
    with pytest.raises(ValueError):
        assembler.poll(now_ns=114)
    for i, q in enumerate(quarter_cycle(10)):
        assembler.feed(q, now_ns=116 + i)
    for i, q in enumerate(quarter_cycle(14)):
        result = assembler.feed(q, now_ns=124 + i)
    assert result.diagnostic == timecode.AssemblyDiagnostic.Discontinuity
    assert not assembler.locked
    assembler.reset()
    for f in (10, 8):
        for q in quarter_cycle(f, reverse=True):
            now = 200 if now < 200 else now + 1
            result = assembler.feed(q, now_ns=now)
    assert result.observation is not None and result.observation.locked
    assert (
        result.observation.direction == -1
        and result.observation.compensated.frames == 6
    )
    relocated = assembler.feed(
        timecode.FullFrame(
            device_id=127,
            rate=timecode.FrameRate.Fps30,
            hours=1,
            minutes=0,
            seconds=0,
            frames=0,
        ),
        now_ns=300,
    )
    assert (
        relocated.diagnostic == timecode.AssemblyDiagnostic.Relocated
        and not assembler.locked
    )
    bad = assembler.feed(messages.QuarterFrame(message_type=7, value=8), now_ns=301)
    assert bad.diagnostic == timecode.AssemblyDiagnostic.Malformed


def test_drop_frame_advances_skipped_labels_and_wraps_in_both_directions() -> None:
    p = timecode.Position(
        rate=timecode.FrameRate.DropFrame30, hours=0, minutes=0, seconds=59, frames=29
    )
    q = p.advance(frames=1)
    assert (q.minutes, q.seconds, q.frames) == (1, 0, 2)
    assert q.advance(frames=-1) == p
    p = timecode.Position(
        rate=timecode.FrameRate.DropFrame30, hours=0, minutes=9, seconds=59, frames=29
    )
    assert p.advance(frames=1).frames == 0
    with pytest.raises(ValueError):
        q.replace(frames=1)
    start = timecode.Position(
        rate=timecode.FrameRate.Fps24, hours=0, minutes=0, seconds=0, frames=0
    )
    end = start.advance(frames=-1)
    assert (end.hours, end.minutes, end.seconds, end.frames) == (23, 59, 59, 23)
    assert end.advance(frames=1) == start
