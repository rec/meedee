"""Universe retention preserves decoder results and exposes observed partial state."""

import pytest

from middi import controls, events, messages, midi, mpe, tuning, ump, universe


def cc(controller: int, value: int, channel: int = 1) -> messages.ControlChange:
    return messages.ControlChange(controller, value, channel)


def known[T](value: T | None) -> T:
    assert value is not None
    return value


@pytest.mark.parametrize(
    "retention", [universe.Retention.Full, universe.Retention.Minimum]
)
def test_parameter_reselection_matches_standalone_decoder(
    retention: universe.Retention,
) -> None:
    cache = universe.Universe(options=universe.CacheOptions(default=retention))
    decoder = controls.ControlInterpreter()
    scope = universe.Scope()
    clock = events.Clock(identity="test", origin="unit")
    inputs = [
        cc(99, 1),
        cc(98, 2),
        cc(38, 7),
        cc(6, 10),
        cc(98, 3),
        cc(6, 20),
        cc(101, 0),
        cc(100, 0),
        cc(6, 30),
        cc(98, 2),
        cc(38, 5),
        cc(96, 0),
        cc(38, 9),
        cc(101, 127),
        cc(100, 127),
    ]
    for index, message in enumerate(inputs):
        event = message.replace(
            stream="keyboard", timestamp=events.Timestamp(ticks=index, clock=clock)
        )
        assert cache.feed(event, now_ns=index) == decoder.feed(event)
        for family, parameter in [
            (controls.ParameterFamily.Nrpn, 130),
            (controls.ParameterFamily.Nrpn, 131),
            (controls.ParameterFamily.Rpn, 0),
        ]:
            expected = decoder.parameter_state(1, family, parameter)
            actual = cache.parameter_state(scope, 1, family, parameter)
            assert (None if actual is None else actual.components) == expected
    snapshot = cache.snapshot()
    assert snapshot == cache.snapshot()
    assert len(snapshot.parameters) == 3
    selection = cache.selection_state(scope, 1)
    assert selection is not None and selection.active is None
    assert selection.nrpn.fine == 2
    cache.reset()
    assert cache.snapshot().parameters == ()
    assert len(snapshot.parameters) == 3


def test_policy_inheritance_and_optional_storage() -> None:
    options = universe.CacheOptions(
        default=universe.Retention.Minimum,
        universe=universe.Retention.Full,
        categories={universe.Category.Notes: universe.Retention.Minimum},
    )
    cache = universe.Universe(options=options)
    scope = universe.Scope()
    assert cache.effective(universe.Category.Notes) == universe.Retention.Minimum
    assert cache.effective(universe.Category.Performance) == universe.Retention.Full
    cache.feed(messages.NoteOn(60, 90, 1))
    cache.feed(messages.ProgramChange(8, 1))
    assert cache.note_state(scope, 1, 60) is None
    assert known(cache.program_state(scope, 1)).program == 8
    minimum = universe.Universe(
        options=universe.CacheOptions(default=universe.Retention.Minimum)
    )
    minimum.feed(messages.ProgramChange(8, 1))
    assert minimum.snapshot().programs == ()
    minimum.feed(cc(1, 0))
    assert known(minimum.controller_state(scope, 1, 1)).coarse == 0


def test_program_bank_partial_controls_and_snapshot_independence() -> None:
    cache = universe.Universe()
    scope = universe.Scope()
    cache.feed(cc(32, 4))
    cache.feed(messages.ProgramChange(7, 1))
    old = known(cache.program_state(scope, 1))
    assert (old.bank_msb, old.bank_lsb, old.program) == (None, 4, 7)
    cache.feed(cc(0, 5))
    new = known(cache.program_state(scope, 1))
    assert (new.pending_msb, new.bank_msb, new.bank_lsb) == (5, None, 4)
    cache.feed(messages.ProgramChange(9, 1))
    assert known(cache.program_state(scope, 1)).bank_msb == 5
    cache.feed(cc(33, 8))
    assert known(cache.controller_state(scope, 1, 1)).coarse is None
    assert known(cache.controller_state(scope, 1, 1)).fine == 8
    cache.feed(cc(1, 0))
    assert known(cache.controller_state(scope, 1, 1)).fine is None
    with pytest.raises(AttributeError):
        old.program = 3  # ty: ignore[invalid-assignment]


def test_group_protocol_and_source_isolation() -> None:
    cache = universe.Universe()
    scope = universe.Scope()
    grouped = universe.Scope(group=2)
    native = universe.Scope(group=2, version=2)
    cache.feed(cc(7, 10).replace(stream="keyboard"))
    cache.feed(cc(7, 20).replace(group=2, stream="keyboard"))
    cache.feed(
        messages.ControlChange(7, 0xFEDCBA98, 1, version=2, group=2).replace(
            stream="keyboard"
        )
    )
    assert known(cache.controller_state(scope, 1, 7)).coarse == 10
    assert known(cache.controller_state(grouped, 1, 7)).coarse == 20
    assert known(cache.controller_state(native, 1, 7)).native == 0xFEDCBA98
    with pytest.raises(ValueError, match="source"):
        cache.feed(cc(7, 99).replace(stream="other"))
    assert known(cache.controller_state(scope, 1, 7)).coarse == 10
    decoder = events.PacketDecoder(stream="keyboard", encoding=midi.Encoding.Bytes)
    loss = decoder.loss(lost=1, reason="test")
    observations = cache.feed(loss)
    assert len(observations) == 1 and observations[0].value is not None
    empty = universe.Universe()
    assert len(empty.feed(loss)) == 1
    assert cache.snapshot().controllers == ()
    cache.reset()
    cache.feed(cc(7, 3).replace(stream="other"))


def test_velocity_prefix_channel_reset_and_clock_loss() -> None:
    cache = universe.Universe()
    scope = universe.Scope()
    cache.feed(cc(88, 5), now_ns=0)
    cache.feed(messages.NoteOn(60, 10, 1), now_ns=1)
    state = known(cache.note_state(scope, 1, 60))
    assert (state.velocity, state.velocity_resolution, state.held) == (1285, 14, True)
    cache.feed(messages.NoteOff(60, 0, 1), now_ns=2)
    assert known(cache.note_state(scope, 1, 60)).held is False
    cache.feed(cc(7, 3, 1), now_ns=3)
    cache.feed(cc(7, 4, 2), now_ns=4)
    cache.feed(cc(121, 0, 1), now_ns=5)
    assert cache.controller_state(scope, 1, 7) is None
    assert known(cache.controller_state(scope, 2, 7)).coarse == 4
    first = events.Clock(identity="first", origin="test")
    second = events.Clock(identity="second", origin="test")
    cache.feed(
        cc(7, 3).replace(timestamp=events.Timestamp(ticks=1, clock=first)), now_ns=6
    )
    cache.feed(
        cc(39, 5).replace(timestamp=events.Timestamp(ticks=2, clock=second)), now_ns=7
    )
    assert cache.controller_state(scope, 2, 7) is None
    assert known(cache.controller_state(scope, 1, 7)).coarse is None
    with pytest.raises(ValueError, match="monotonic"):
        cache.poll(now_ns=6)


def test_timecode_timeout_transport_and_mpe() -> None:
    cache = universe.Universe()
    cache.configure_mpe(mpe.ZoneConfiguration(manager_channel=1, members=2))
    cache.feed(messages.NoteOn(60, 10, 2), now_ns=0)
    assert cache.snapshot().mpe
    cache.feed(messages.Start(), now_ns=1)
    state = known(cache.system_state(universe.Scope()))
    assert state.running is True and state.position == 0
    full = messages.SysEx(bytes([127, 127, 1, 1, 1, 2, 3, 4]))
    cache.feed(full, now_ns=2)
    state = known(cache.system_state(universe.Scope()))
    assert known(state.timecode).encoded.hours == 1
    cache.feed(messages.QuarterFrame(0, 0), now_ns=3)
    cache.poll(now_ns=1_000_000_004)
    assert known(cache.system_state(universe.Scope())).timecode is None


def test_invalid_addresses_and_commands_do_not_change_state() -> None:
    cache = universe.Universe()
    with pytest.raises(ValueError):
        universe.Scope(version=2)
    with pytest.raises(ValueError):
        cache.controller_state(universe.Scope(), 0, 7)
    with pytest.raises(ValueError):
        cache.parameter_state(universe.Scope(), 1, controls.ParameterFamily.Rpn, 16384)
    with pytest.raises(ValueError, match="primitive"):
        cache.feed(messages.ParameterSet(0, 1, channel=1))
    assert cache.snapshot().parameters == ()


def test_native_parameters_per_note_state_and_tuning_fragments() -> None:
    cache = universe.Universe()
    scope = universe.Scope(group=2, version=2)
    cache.feed(
        messages.ParameterController(
            family="assignable", group=2, channel=1, bank=1, index=2, value=0xFEDCBA98
        )
    )
    parameter = known(
        cache.parameter_state(scope, 1, controls.ParameterFamily.Nrpn, 130)
    )
    assert parameter.native == 0xFEDCBA98 and parameter.components is None
    assert (
        cache.parameter_state(
            universe.Scope(group=2), 1, controls.ParameterFamily.Nrpn, 130
        )
        is None
    )
    cache.feed(
        messages.PerNoteController(
            group=2, channel=1, note=60, controller=1, value=0xFFFFFFFF
        )
    )
    state = known(cache.note_state(scope, 1, 60))
    assert state.held is None and state.registered == {1: 0xFFFFFFFF}
    state.registered[1] = 0
    assert known(cache.note_state(scope, 1, 60)).registered[1] == 0xFFFFFFFF
    cache.feed(messages.PerNoteManagement(group=2, channel=1, note=60, flags=3))
    assert known(cache.note_state(scope, 1, 60)).registered == {}
    payload = messages.SysEx(bytes([127, 1, 8, 2, 0, 1, 60, 60, 0, 0]), group=2)
    for packet in ump.packetize_sysex7(payload):
        cache.feed(packet)
    state = known(cache.tuning_state(2, 1, None, 0))
    pitch = state.notes[60]
    assert isinstance(pitch, tuning.Pitch)
    assert pitch.semitone == 60
    cache.feed(
        messages.ParameterController(
            family="assignable", group=2, channel=1, bank=1, index=2, value=1
        )
    )
    assert parameter.native == 0xFEDCBA98


def test_universal_controls_and_ump_resets_share_scope() -> None:
    cache = universe.Universe()
    scope = universe.Scope(group=2)
    volume = messages.SysEx(bytes([127, 1, 4, 1, 5, 10]), group=2)
    for packet in ump.packetize_sysex7(volume):
        cache.feed(packet)
    assert known(cache.system_state(scope)).master_volume == {1: 1285}
    cache.feed(cc(7, 10).replace(group=2))
    reset = messages.SysEx(bytes([126, 127, 9, 1]), group=2)
    for packet in ump.packetize_sysex7(reset):
        cache.feed(packet)
    assert cache.controller_state(scope, 1, 7) is None
    assert cache.system_state(scope) is None
    cache.feed(messages.NoteOn(60, 10, 1))
    cache.feed(cc(120, 0))
    assert known(cache.note_state(universe.Scope(), 1, 60)).held is True
    cache.feed(cc(123, 0))
    assert known(cache.note_state(universe.Scope(), 1, 60)).held is False


def test_interleaved_groups_keep_universe_indexes_and_timestamps() -> None:
    cache = universe.Universe()
    clock = events.Clock(identity="clock", origin="test")
    cache.feed(
        cc(7, 10).replace(group=2, timestamp=events.Timestamp(ticks=10, clock=clock))
    )
    cache.feed(cc(1, 1))
    cache.feed(cc(1, 2).replace(group=3))
    observations = cache.feed(
        cc(39, 5).replace(group=2, timestamp=events.Timestamp(ticks=20, clock=clock))
    )
    value = next(
        o for o in observations if isinstance(o.value, controls.ControlObservation)
    )
    assert (value.first_index, value.last_index) == (0, 3)
    assert isinstance(value.value, controls.ControlObservation)
    assert (value.value.first_index, value.value.last_index) == (0, 3)
    assert known(value.first_timestamp).ticks == 10
    assert known(value.last_timestamp).ticks == 20
    packets = ump.packetize_sysex7(
        messages.SysEx(bytes([127, 1, 8, 2, 0, 1, 60, 60, 0, 0]), group=2)
    )
    cache.feed(packets[0])
    cache.feed(cc(1, 3).replace(group=3))
    observations = cache.feed(packets[1])
    decoded = next(o for o in observations if isinstance(o.value, tuning.Decoded))
    assert (decoded.first_index, decoded.last_index) == (4, 6)
