"""Semantic event views retain primitive evidence and source/clock boundaries."""

import pytest

from meedee import controls, events, messages, midi, mpe, timecode, tuning, ump


def event(message: messages.Message, ticks: int) -> events.Event:
    clock = events.Clock(identity="c", origin="test")
    return events.Event(
        stream="s",
        message=message,
        timestamp=events.Timestamp(ticks=ticks, clock=clock),
    )


def test_control_and_velocity_views_keep_contributor_times_after_long_gaps() -> None:
    view = controls.EventInterpreter()
    view.feed(event(messages.ControlChange(channel=1, controller=7, value=64), 10))
    for i in range(1500):
        view.feed(event(messages.NoteOn(channel=1, note=60, velocity=100), i + 11))
    observation = view.feed(
        event(messages.ControlChange(channel=1, controller=39, value=1), 2000)
    )[0]
    value = observation.value
    assert isinstance(value, controls.ControlObservation) and value.value == 8193
    assert observation.first_index == 0 and observation.last_index == 1501
    assert (
        observation.first_timestamp is not None
        and observation.first_timestamp.ticks == 10
    )
    assert (
        observation.last_timestamp is not None
        and observation.last_timestamp.ticks == 2000
    )
    with pytest.raises(ValueError, match="source"):
        view.feed(event(messages.TimingClock(), 3000).replace(stream="other"))
    velocity = controls.EventVelocityInterpreter()
    velocity.feed(event(messages.ControlChange(channel=1, controller=88, value=127), 1))
    observation = velocity.feed(
        event(messages.NoteOn(channel=1, note=60, velocity=127), 2)
    )[0]
    value = observation.value
    assert isinstance(value, controls.VelocityObservation) and value.velocity == 16383
    assert (observation.first_index, observation.last_index) == (0, 1)
    velocity.reset()
    unknown_start = event(messages.NoteOn(channel=1, note=60, velocity=1), 3).replace(
        span=events.SourceSpan(first_packet=0, last_packet=1)
    )
    observation = velocity.feed(unknown_start)[0]
    assert observation.first_timestamp is None and observation.last_timestamp is None


def test_group_selection_and_reported_loss_cannot_mix_control_state() -> None:
    view = controls.EventInterpreter(group=2)
    view.feed(
        event(
            ump.Midi1Packet(
                group=2,
                message=messages.ControlChange(channel=1, controller=7, value=1),
            ),
            1,
        )
    )
    assert (
        view.feed(
            event(
                ump.Midi1Packet(
                    group=1,
                    message=messages.ControlChange(channel=1, controller=39, value=2),
                ),
                2,
            )
        )
        == []
    )
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    loss = decoder.loss(lost=1, reason="known loss")
    view.feed(loss)
    observation = view.feed(
        event(
            ump.Midi1Packet(
                group=2,
                message=messages.ControlChange(channel=1, controller=39, value=2),
            ),
            3,
        )
    )[0]
    assert isinstance(observation.value, controls.Diagnostic)
    assert (
        observation.first_timestamp is not None
        and observation.first_timestamp.ticks == 3
    )
    with pytest.raises(ValueError, match="group"):
        controls.EventInterpreter().feed(
            event(ump.Midi1Packet(group=2, message=messages.TimingClock()), 4)
        )


def test_mpe_and_mtc_views_do_not_invent_unobserved_state_or_native_time() -> None:
    tracker = mpe.EventTracker(zone=mpe.ZoneConfiguration(manager_channel=1, members=2))
    assert tracker.snapshot(channel=2).pitch_bend is None
    observations = tracker.feed(
        event(messages.NoteOn(channel=2, note=60, velocity=100), 1)
    )
    assert observations and isinstance(observations[0].value, mpe.ChannelState)
    assembler = timecode.EventAssembler(timeout_ns=100)
    assert (
        assembler.feed(
            event(messages.QuarterFrame(message_type=0, value=0), 10), now_ns=1
        )
        == []
    )
    result = assembler.poll(now_ns=101)
    assert result and isinstance(result[0].value, timecode.AssemblyResult)
    assert result[0].first_timestamp is result[0].last_timestamp is None


def test_ump_assembly_and_mts_decode_keep_the_group_and_contributor_span() -> None:
    payload = messages.SysEx(data=bytes([127, 1, 8, 2, 0, 1, 60, 60, 0, 0]))
    packets = ump.packetize_sysex7(group=2, data=payload.data)
    assembler = ump.EventAssembler()
    observations = [
        o for i, p in enumerate(packets) for o in assembler.feed(event(p, i + 10))
    ]
    assert len(observations) == 1 and observations[0].group == 2
    assert (
        observations[0].first_timestamp is not None
        and observations[0].first_timestamp.ticks == 10
    )
    decoded = tuning.EventDecoder(group=2).feed_assembled(observations[0])
    assert decoded and isinstance(decoded[0].value, tuning.Decoded)
    assert (
        decoded[0].group == 2
        and decoded[0].last_timestamp == observations[0].last_timestamp
    )
