"""Compound command vectors and conservative observations of primitive streams."""

import inspect
from collections.abc import Iterator
from pathlib import Path

import pytest

from meedee import codecs, controls, events, messages, ump


def test_controller_resolution_is_explicit_and_preserved() -> None:
    coarse = messages.ControlChange(1, 90, channel=2)
    fine = messages.ControlChange(1, 90, channel=2, resolution=14)
    native = messages.ControlChange(1, 90, channel=2, version=2)
    assert (coarse.resolution, fine.resolution, native.resolution) == (7, 14, 32)
    assert len({coarse, fine, native}) == 3
    assert fine.replace() == fine
    assert fine.replace(value=16383).resolution == 14
    assert fine.replace(resolution=7) == coarse
    assert messages.ControlChange(7, 1).channel is None
    stamp = events.Timestamp(ticks=3, clock=events.Clock(identity="cc"))
    marked = fine.replace(timestamp=stamp, stream="controller")
    assert marked.replace(value=100).timestamp == stamp
    pair = codecs.expand(marked)
    assert pair == [messages.ControlChange(1, 0, 2), messages.ControlChange(33, 90, 2)]
    assert all(p.timestamp == stamp and p.stream == "controller" for p in pair)
    wire = b"".join(codecs.encode(p) for p in pair)
    assert codecs.Decoder().feed(wire) == pair
    with pytest.raises(ValueError, match="expansion"):
        codecs.encode(fine)
    with pytest.raises((TypeError, ValueError)):
        ump.encode(fine)
    assert ump.decode(ump.encode(native)) == native
    converted = ump.to_midi2(fine, group=1)
    assert converted.messages == [
        messages.ControlChange(
            1, ump.scale_up(value=90, source_bits=14, destination_bits=32), 2, version=2
        )
    ]
    for version, resolution in [(1, 8), (1, 32), (2, 7), (2, 14)]:
        with pytest.raises(ValueError):
            messages.ControlChange(1, 0, version=version, resolution=resolution)  # ty: ignore[invalid-argument-type]
    with pytest.raises(TypeError):
        messages.ControlChange(1, 0, resolution=True)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        fine.replace(value=128, resolution=7)
    with pytest.raises(ValueError):
        fine.replace(controller=32)
    with pytest.raises(ValueError, match="unassigned"):
        codecs.expand(messages.ControlChange(1, 128, resolution=14))


def command_vectors() -> Iterator[tuple[object, bytes]]:
    fixture = (
        Path(__file__).resolve().parents[1]
        / "crates/meedee/tests/fixtures/commands.txt"
    )
    for line in fixture.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        fields, wire = line.split("|")
        name, *values = fields.split()
        constructor = getattr(messages, name)
        if name == "ControlChange":
            channel, controller, value = (int(v) for v in values)
            yield (
                constructor(controller, value, channel, resolution=14),
                bytes.fromhex(wire),
            )
            continue
        if name == "ParameterStep":
            channel, parameter, direction, family = values
            yield (
                constructor(
                    int(parameter), int(direction), int(channel), family=family
                ),
                bytes.fromhex(wire),
            )
            continue
        channel, parameter, value, resolution, family = values
        yield (
            constructor(
                int(parameter), int(value), int(resolution), int(channel), family=family
            ),
            bytes.fromhex(wire),
        )


def test_commands_expand_to_shared_independent_wire_vectors() -> None:
    for c, b in command_vectors():
        primitives = codecs.expand(c)  # ty: ignore[invalid-argument-type]
        assert all(type(p) is messages.ControlChange for p in primitives)
        assert b"".join(codecs.encode(p) for p in primitives) == b
        # A compound command must not masquerade as one raw transport packet.
        with pytest.raises(
            ValueError if isinstance(c, messages.ControlChange) else TypeError
        ):
            codecs.encode(c)  # ty: ignore[invalid-argument-type]
    message = messages.ControlChange(7, 1, channel=1)
    assert codecs.expand(message) == [message]


def test_commands_are_frozen_positional_values_with_checked_replacement() -> None:
    for c, _ in command_vectors():
        if isinstance(c, (messages.ParameterStep, messages.ControlChange)):
            continue  # These have keyword-only fields covered separately.
        constructor = getattr(messages, type(c).__name__)
        assert isinstance(c, messages.Message)
        fields = constructor.__annotations__
        assert constructor.__module__ == "meedee.messages"
        assert not hasattr(c, "__dict__")
        values = {k: getattr(c, k) for k in fields}
        rendered = ", ".join(f"{k}={v!r}" for k, v in values.items())
        assert repr(c) == f"{constructor.__name__}({rendered})"
        assert c == c.replace()
        assert hash(c) == hash(c.replace())
        with pytest.raises(TypeError):
            c.replace(unknown=1)
        assert (
            constructor(
                *(values[k] for k in constructor.__match_args__), family=c.family
            )
            == c
        )
        for f in fields:
            assert fields[f] == (
                str if f == "family" else int | None if f == "channel" else int
            )
            assert inspect.signature(constructor).parameters[f].kind == (
                inspect.Parameter.KEYWORD_ONLY
                if f == "family"
                else inspect.Parameter.POSITIONAL_OR_KEYWORD
            )
            with pytest.raises(AttributeError):
                setattr(c, f, 1)
            for v in (True, 1.0, "1", None):
                if f == "channel" and v is None:
                    continue
                error = (
                    ValueError if f == "family" and isinstance(v, str) else TypeError
                )
                with pytest.raises(error):
                    constructor(**(values | {f: v}))
                with pytest.raises(error):
                    c.replace(**{f: v})
    command = messages.ParameterSet(
        family="nrpn", channel=1, parameter=1234, value=10000, resolution=14
    )
    registered = command.replace(family="rpn")
    assert registered == messages.ParameterSet(1234, 10000, channel=1)
    assert len({command, registered}) == 2
    stamp = events.Timestamp(ticks=3, clock=events.Clock(identity="parameter"))
    marked = command.replace(timestamp=stamp, stream="knob")
    assert marked.replace(family="rpn").timestamp == stamp
    assert marked.replace(value=10).stream == "knob"
    assert all(
        p.timestamp == stamp and p.stream == "knob" for p in codecs.expand(marked)
    )
    boundary = command.replace(parameter=16383)
    with pytest.raises(ValueError):
        boundary.replace(family="rpn")
    assert boundary.replace(parameter=0, family="rpn").parameter == 0
    with pytest.raises(ValueError, match="family"):
        command.replace(family="other")  # ty: ignore[invalid-argument-type]
    assert command.replace(value=10001).value == 10001
    assert command.value == 10000
    with pytest.raises(ValueError):
        command.replace(resolution=7)
    assert command.replace(resolution=7, value=127).value == 127
    match command:
        case messages.ParameterSet(1234, 10000, 14, 1, family="nrpn"):
            pass
        case _:
            pytest.fail("command pattern did not match")


def test_command_ranges_and_null_parameter_are_explicit() -> None:
    for channel in (0, 17, 10**100):
        with pytest.raises(ValueError):
            messages.ControlChange(
                resolution=14, channel=channel, controller=7, value=0
            )
    for controller in (-1, 32):
        with pytest.raises(ValueError):
            messages.ControlChange(
                resolution=14, channel=1, controller=controller, value=0
            )
    for value in (-1, 16384):
        with pytest.raises(ValueError):
            messages.ControlChange(resolution=14, channel=1, controller=0, value=value)
    for family in ("rpn", "nrpn"):
        for resolution in (0, 8, 13, 15):
            with pytest.raises(ValueError):
                messages.ParameterSet(
                    channel=1,
                    parameter=0,
                    value=0,
                    resolution=resolution,
                    family=family,
                )
        with pytest.raises(ValueError):
            messages.ParameterSet(
                channel=1, parameter=0, value=128, resolution=7, family=family
            )
        assert (
            messages.ParameterSet(
                channel=1, parameter=0, value=0, family=family
            ).resolution
            == 14
        )
    for name in ("ParameterSet", "ParameterStep"):
        fields = {"channel": 1, "parameter": 16383}
        if name == "ParameterSet":
            fields |= {"value": 0, "resolution": 7}
        with pytest.raises(ValueError):
            getattr(messages, name)(**fields)
    assert (
        messages.ParameterSet(
            family="nrpn", channel=16, parameter=16383, value=16383, resolution=14
        ).parameter
        == 16383
    )


def test_fine_control_observations_preserve_wire_order_and_source_ranges() -> None:
    interpreter = controls.ControlInterpreter()

    def cc(channel: int, controller: int, value: int) -> object:
        return interpreter.feed(
            messages.ControlChange(channel=channel, controller=controller, value=value)
        )[0].value

    early = cc(1, 39, 7)
    assert isinstance(early, controls.Diagnostic)
    assert early.kind == controls.DiagnosticKind.MissingCoarse
    coarse = cc(1, 7, 2)
    assert isinstance(coarse, controls.ControlObservation)
    assert (coarse.value, coarse.resolution, coarse.first_index, coarse.last_index) == (
        2,
        7,
        1,
        1,
    )
    cc(2, 7, 9)
    assert interpreter.feed(messages.TimingClock()) == []
    fine = cc(1, 39, 1)
    assert isinstance(fine, controls.ControlObservation)
    assert (
        fine.channel,
        fine.controller,
        fine.value,
        fine.resolution,
        fine.first_index,
        fine.last_index,
    ) == (1, 7, 257, 14, 1, 4)
    repeated = cc(1, 39, 2)
    assert isinstance(repeated, controls.ControlObservation) and repeated.value == 258
    cc(1, 7, 3)
    fresh = cc(1, 39, 4)
    assert isinstance(fresh, controls.ControlObservation)
    assert (fresh.value, fresh.first_index, fresh.last_index) == (388, 6, 7)


@pytest.mark.parametrize(
    ("family", "msb", "lsb", "other_msb", "other_lsb"),
    (
        (controls.ParameterFamily.Nrpn, 99, 98, 101, 100),
        (controls.ParameterFamily.Rpn, 101, 100, 99, 98),
    ),
)
def test_selector_edits_retain_the_other_half_but_not_data(
    family: controls.ParameterFamily, msb: int, lsb: int, other_msb: int, other_lsb: int
) -> None:
    interpreter = controls.ControlInterpreter()

    def cc(controller: int, value: int) -> list[object]:
        return [
            i.value
            for i in interpreter.feed(
                messages.ControlChange(channel=1, controller=controller, value=value)
            )
        ]

    assert cc(msb, 1) == []
    partial = cc(6, 2)[0]
    assert (
        isinstance(partial, controls.Diagnostic)
        and partial.kind == controls.DiagnosticKind.IncompleteSelection
    )
    assert cc(lsb, 2) == []
    coarse = cc(6, 3)[0]
    assert isinstance(coarse, controls.ParameterObservation)
    assert (coarse.family, coarse.parameter, coarse.value, coarse.resolution) == (
        family,
        130,
        3,
        7,
    )
    fine = cc(38, 4)[0]
    assert isinstance(fine, controls.ParameterObservation)
    assert (fine.value, fine.resolution, fine.first_index, fine.last_index) == (
        388,
        14,
        0,
        4,
    )
    assert cc(lsb, 3) == []
    missing = cc(38, 4)[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )
    edited = cc(6, 7)[0]
    assert isinstance(edited, controls.ParameterObservation)
    assert (edited.family, edited.parameter, edited.value, edited.resolution) == (
        family,
        131,
        7,
        7,
    )
    assert (edited.first_index, edited.last_index) == (0, 7)
    assert cc(msb, 2) == []
    missing = cc(38, 4)[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )
    edited = cc(6, 8)[0]
    assert isinstance(edited, controls.ParameterObservation)
    assert (edited.family, edited.parameter, edited.value) == (family, 259, 8)
    assert (edited.first_index, edited.last_index) == (5, 10)
    invalidated = cc(other_lsb, 2)[0]
    assert (
        isinstance(invalidated, controls.Diagnostic)
        and invalidated.kind == controls.DiagnosticKind.SelectionInvalidated
    )
    partial = cc(38, 3)[0]
    assert (
        isinstance(partial, controls.Diagnostic)
        and partial.kind == controls.DiagnosticKind.IncompleteSelection
    )
    cc(other_msb, 0)
    missing = cc(38, 1)[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )


@pytest.mark.parametrize(
    ("family", "msb", "lsb"),
    ((controls.ParameterFamily.Nrpn, 99, 98), (controls.ParameterFamily.Rpn, 101, 100)),
)
def test_repeated_selectors_preserve_data_and_its_source_range(
    family: controls.ParameterFamily, msb: int, lsb: int
) -> None:
    interpreter = controls.ControlInterpreter()
    for controller, value in ((msb, 1), (lsb, 2), (6, 10)):
        interpreter.feed(messages.ControlChange(controller, value, channel=1))
    for controller, value, first_index, last_index in ((msb, 1, 1, 4), (lsb, 2, 2, 6)):
        assert (
            interpreter.feed(messages.ControlChange(controller, value, channel=1)) == []
        )
        observation = interpreter.feed(messages.ControlChange(38, 3, channel=1))[
            0
        ].value
        assert isinstance(observation, controls.ParameterObservation)
        assert (
            observation.family,
            observation.parameter,
            observation.value,
            observation.resolution,
        ) == (family, 130, 1283, 14)
        assert (observation.first_index, observation.last_index) == (
            first_index,
            last_index,
        )
    for value in (3, 2):
        interpreter.feed(messages.ControlChange(lsb, value, channel=1))
    missing = interpreter.feed(messages.ControlChange(38, 3, channel=1))[0].value
    assert isinstance(missing, controls.Diagnostic)
    assert missing.kind == controls.DiagnosticKind.MissingCoarse


@pytest.mark.parametrize(
    ("family", "msb", "lsb", "other_msb", "other_lsb"),
    (
        (controls.ParameterFamily.Nrpn, 99, 98, 101, 100),
        (controls.ParameterFamily.Rpn, 101, 100, 99, 98),
    ),
)
def test_family_switches_retain_complete_and_partial_selectors_but_not_data(
    family: controls.ParameterFamily, msb: int, lsb: int, other_msb: int, other_lsb: int
) -> None:
    interpreter = controls.ControlInterpreter()

    def cc(controller: int, value: int) -> object:
        result = interpreter.feed(messages.ControlChange(controller, value, channel=1))
        return result[-1].value if result else None

    cc(msb, 1)
    cc(lsb, 2)
    cc(6, 10)
    cc(other_msb, 3)
    partial = cc(6, 20)
    assert isinstance(partial, controls.Diagnostic)
    assert partial.kind == controls.DiagnosticKind.IncompleteSelection
    cc(lsb, 3)
    missing = cc(38, 4)
    assert isinstance(missing, controls.Diagnostic)
    assert missing.kind == controls.DiagnosticKind.MissingCoarse
    restored = cc(6, 30)
    assert isinstance(restored, controls.ParameterObservation)
    assert (restored.family, restored.parameter, restored.value) == (family, 131, 30)
    cc(other_lsb, 4)
    other = cc(6, 40)
    assert isinstance(other, controls.ParameterObservation)
    assert other.family != family and other.parameter == 388 and other.value == 40
    cc(msb, 2)
    restored = cc(6, 50)
    assert isinstance(restored, controls.ParameterObservation)
    assert (restored.family, restored.parameter, restored.value) == (family, 259, 50)
    interpreter.reset()
    for controller in (lsb, other_lsb):
        cc(controller, 5)
        partial = cc(6, 60)
        assert isinstance(partial, controls.Diagnostic)
        assert partial.kind == controls.DiagnosticKind.IncompleteSelection


def test_relative_steps_do_not_invent_absolute_values_or_increment_magnitudes() -> None:
    interpreter = controls.ControlInterpreter()
    for controller, value in ((101, 0), (100, 1), (6, 2)):
        interpreter.feed(
            messages.ControlChange(channel=1, controller=controller, value=value)
        )
    for controller, value, direction in (
        (96, 127, controls.StepDirection.Increment),
        (97, 99, controls.StepDirection.Decrement),
    ):
        step = interpreter.feed(
            messages.ControlChange(channel=1, controller=controller, value=value)
        )[0].value
        assert isinstance(step, controls.ParameterStep)
        assert (step.family, step.parameter, step.direction, step.first_index) == (
            controls.ParameterFamily.Rpn,
            1,
            direction,
            0,
        )
        assert not hasattr(step, "value")
    missing = interpreter.feed(
        messages.ControlChange(channel=1, controller=38, value=3)
    )[0].value
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )


def test_rpn_null_deselects_nrpn_without_reserving_its_maximum_identifier() -> None:
    interpreter = controls.ControlInterpreter()
    for controller in (99, 98):
        interpreter.feed(
            messages.ControlChange(channel=1, controller=controller, value=127)
        )
    observed = interpreter.feed(
        messages.ControlChange(channel=1, controller=6, value=127)
    )[0].value
    assert isinstance(observed, controls.ParameterObservation)
    assert (observed.family, observed.parameter) == (
        controls.ParameterFamily.Nrpn,
        16383,
    )
    for controller in (101, 100):
        interpreter.feed(
            messages.ControlChange(channel=1, controller=controller, value=127)
        )
    missing = interpreter.feed(
        messages.ControlChange(channel=1, controller=6, value=1)
    )[0].value
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingSelection
    )

    interpreter.feed(messages.ControlChange(98, 127, channel=1))
    missing = interpreter.feed(messages.ControlChange(38, 1, channel=1))[0].value
    assert isinstance(missing, controls.Diagnostic)
    assert missing.kind == controls.DiagnosticKind.MissingCoarse
    restored = interpreter.feed(messages.ControlChange(6, 2, channel=1))[0].value
    assert isinstance(restored, controls.ParameterObservation)
    assert (restored.family, restored.parameter) == (
        controls.ParameterFamily.Nrpn,
        16383,
    )


def test_resets_invalidate_channel_source_and_parameter_state() -> None:
    one = controls.ControlInterpreter()
    two = controls.ControlInterpreter()
    for channel in (1, 2):
        one.feed(messages.ControlChange(channel=channel, controller=7, value=2))
    fine = messages.ControlChange(channel=1, controller=39, value=1)
    assert isinstance(two.feed(fine)[0].value, controls.Diagnostic)
    reset = one.feed(messages.ControlChange(channel=1, controller=121, value=0))[
        0
    ].value
    assert isinstance(reset, controls.Diagnostic) and reset.channel == 1
    assert isinstance(one.feed(fine)[0].value, controls.Diagnostic)
    assert isinstance(
        one.feed(fine.replace(channel=2))[0].value, controls.ControlObservation
    )
    reset = one.feed(messages.SystemReset())[0].value
    assert isinstance(reset, controls.Diagnostic) and reset.channel is None
    assert isinstance(one.feed(fine.replace(channel=2))[0].value, controls.Diagnostic)
    one.feed(messages.ControlChange(channel=1, controller=7, value=2))
    assert one.reset() is None
    assert isinstance(one.feed(fine)[0].value, controls.Diagnostic)
    for command in codecs.expand(
        messages.ParameterSet(
            family="nrpn", channel=1, parameter=0, value=1, resolution=14
        )
    )[:2]:
        one.feed(command)
    one.reset()
    missing = one.feed(messages.ControlChange(channel=1, controller=6, value=1))[
        0
    ].value
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingSelection
    )


def test_observations_are_frozen_owned_values_and_raw_input_is_unchanged() -> None:
    interpreter = controls.ControlInterpreter()
    raw = messages.ControlChange(channel=1, controller=7, value=2)
    first = interpreter.feed(raw)[0].value
    same = controls.ControlInterpreter().feed(raw)[0].value
    assert isinstance(first, controls.ControlObservation)
    assert first == same and hash(first) == hash(same)
    assert first != object()
    assert (
        repr(first)
        == "ControlObservation(channel=1, controller=7, value=2, resolution=7, "
        "first_index=0, last_index=0)"
    )
    assert raw.value == 2
    with pytest.raises(AttributeError):
        first.value = 3  # ty: ignore[invalid-assignment]
    with pytest.raises(TypeError):
        controls.ControlObservation()
    for value in (
        b"\xb0\x07\x01",
        messages.ControlChange(resolution=14, channel=1, controller=7, value=2),
    ):
        with pytest.raises((TypeError, ValueError)):
            interpreter.feed(value)  # ty: ignore[invalid-argument-type]
    fine = interpreter.feed(messages.ControlChange(channel=1, controller=39, value=1))[
        0
    ].value
    assert isinstance(fine, controls.ControlObservation) and fine.last_index == 1
    for controller in (0, 32):
        assert (
            interpreter.feed(
                messages.ControlChange(channel=1, controller=controller, value=127)
            )
            == []
        )
    assert interpreter.feed(messages.SysEx(data=b"\x7d")) == []


def test_parameter_step_defaults_validation_and_metadata_replacement() -> None:
    from meedee import events

    default = messages.ParameterStep(7)
    assert (default.parameter, default.direction, default.channel, default.family) == (
        7,
        1,
        None,
        "rpn",
    )
    clock = events.Clock(identity="source")
    step = default.replace(
        family="nrpn",
        parameter=16383,
        direction=-1,
        channel=2,
        stream="keys",
        timestamp=events.Timestamp(ticks=10, clock=clock),
    )
    assert step == messages.ParameterStep(16383, -1, 2, family="nrpn")
    assert hash(step) == hash(step.replace(timestamp=None, stream=None))
    assert step.replace(channel=3).timestamp == step.timestamp
    assert step.replace(direction=1).stream == "keys"
    assert (
        repr(default)
        == 'ParameterStep(parameter=7, direction=1, channel=None, family="rpn")'
    )
    with pytest.raises(AttributeError):
        step.direction = 1  # ty: ignore[invalid-assignment]
    with pytest.raises(ValueError):
        step.replace(family="rpn")
    for direction in (0, 2, -2):
        with pytest.raises(ValueError, match="direction"):
            messages.ParameterStep(7, direction)  # ty: ignore[invalid-argument-type]
    for direction in (True, 1.0, "1"):
        with pytest.raises(TypeError):
            messages.ParameterStep(7, direction)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError, match="family"):
        messages.ParameterStep(7, family="other")  # ty: ignore[invalid-argument-type]
    with pytest.raises(TypeError):
        messages.ParameterStep(7, 1, 1, "nrpn")  # ty: ignore[too-many-positional-arguments]
