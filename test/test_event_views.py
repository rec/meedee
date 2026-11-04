"""Semantic event views retain primitive evidence and source/clock boundaries."""

import pytest

from meedee import controls, events, messages, midi, mpe, timecode, tuning, ump


def event(message: messages.Message, ticks: int) -> messages.Message:
    clock = events.Clock(identity="c", origin="test")
    return (message).replace(
        stream="s", timestamp=events.Timestamp(ticks=ticks, clock=clock)
    )


@pytest.mark.parametrize("velocity", [False, True])
def test_interpreters_require_reset_between_anonymous_and_named_sources(
    velocity: bool,
) -> None:
    interpreter = (
        controls.VelocityInterpreter() if velocity else controls.ControlInterpreter()
    )
    prefix = messages.ControlChange(88 if velocity else 7, 1, channel=1)
    following = (
        messages.NoteOn(60, 1, 1) if velocity else messages.ControlChange(39, 1, 1)
    )
    interpreter.feed(prefix)
    with pytest.raises(ValueError, match="source"):
        interpreter.feed(following.replace(stream="keyboard"))
    result = interpreter.feed(following)[0]
    assert result.stream is None and result.first_timestamp is None
    assert isinstance(
        result.value, (controls.ControlObservation, controls.VelocityObservation)
    )
    assert result.value.resolution == 14

    interpreter.reset()
    interpreter.feed(prefix.replace(stream="keyboard"))
    for m in [following, following.replace(stream="other")]:
        with pytest.raises(ValueError, match="source"):
            interpreter.feed(m)
    result = interpreter.feed(following.replace(stream="keyboard"))[0]
    assert result.stream == "keyboard"
    assert isinstance(
        result.value, (controls.ControlObservation, controls.VelocityObservation)
    )
    assert result.value.resolution == 14

    decoder = events.PacketDecoder(stream="keyboard", protocol=midi.Protocol.Midi1)
    loss = decoder.loss(lost=3, reason="known loss")
    interpreter.feed(prefix.replace(stream="keyboard"))
    reported = interpreter.feed(loss)[0].value
    assert isinstance(reported, events.StreamDiagnostic)
    assert all(
        getattr(reported, n) == getattr(loss, n)
        for n in ("stream", "packet", "kind", "lost", "reason")
    )
    result = interpreter.feed(following.replace(stream="keyboard"))[0].value
    if velocity:
        assert (
            isinstance(result, controls.VelocityObservation) and result.resolution == 7
        )
    else:
        assert (
            isinstance(result, controls.Diagnostic)
            and result.kind == controls.DiagnosticKind.MissingCoarse
        )


@pytest.mark.parametrize("velocity", [False, True])
def test_anonymous_clock_changes_report_reset_without_inventing_a_source(
    velocity: bool,
) -> None:
    interpreter = (
        controls.VelocityInterpreter() if velocity else controls.ControlInterpreter()
    )
    first = events.Timestamp(ticks=10, clock=events.Clock(identity="first"))
    second = events.Timestamp(ticks=20, clock=events.Clock(identity="second"))
    prefix = messages.ControlChange(88 if velocity else 7, 1, channel=1)
    following = (
        messages.NoteOn(60, 1, 1) if velocity else messages.ControlChange(39, 1, 1)
    )
    interpreter.feed(prefix.replace(timestamp=first))
    reset, result = interpreter.feed(following.replace(timestamp=second))
    assert reset.stream is None and result.stream is None
    assert isinstance(reset.value, controls.Diagnostic)
    assert reset.value.kind == controls.DiagnosticKind.StateReset
    assert reset.first_timestamp is None and reset.last_timestamp == second
    if velocity:
        assert isinstance(result.value, controls.VelocityObservation)
        assert result.value.resolution == 7
    else:
        assert isinstance(result.value, controls.Diagnostic)
        assert result.value.kind == controls.DiagnosticKind.MissingCoarse


def test_control_and_velocity_views_keep_contributor_times_after_long_gaps() -> None:
    view = controls.ControlInterpreter()
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
    velocity = controls.VelocityInterpreter()
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
    view = controls.ControlInterpreter(group=2)
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
        controls.ControlInterpreter().feed(
            event(ump.Midi1Packet(group=2, message=messages.TimingClock()), 4)
        )


def test_mpe_and_mtc_views_do_not_invent_unobserved_state_or_native_time() -> None:
    tracker = mpe.EventTracker(zone=mpe.ZoneConfiguration(manager_channel=1, members=2))
    assert tracker.snapshot(channel=2).pitch_bend is None
    observations = tracker.feed(
        event(messages.NoteOn(channel=2, note=60, velocity=100), 1)
    )
    assert observations and isinstance(observations[0].value, mpe.ChannelState)
    assembler = timecode.EventAssembler(timeout="100 ns")
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
