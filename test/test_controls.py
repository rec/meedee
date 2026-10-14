"""Compound command vectors and conservative observations of primitive streams."""

import inspect
from collections.abc import Iterator
from pathlib import Path

import pytest

from meedee import codecs, controls, messages


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
        yield (
            constructor(
                **dict(
                    zip(
                        inspect.signature(constructor).parameters,
                        (int(v) for v in values),
                        strict=True,
                    )
                )
            ),
            bytes.fromhex(wire),
        )


def test_commands_expand_to_shared_independent_wire_vectors() -> None:
    for c, b in command_vectors():
        primitives = codecs.expand(c)  # ty: ignore[invalid-argument-type]
        assert all(type(p) is messages.ControlChange for p in primitives)
        assert b"".join(codecs.encode(p) for p in primitives) == b
        # A compound command must not masquerade as one raw transport packet.
        with pytest.raises(TypeError):
            codecs.encode(c)  # ty: ignore[invalid-argument-type]
    with pytest.raises(TypeError):
        codecs.expand(messages.ControlChange(channel=1, controller=7, value=1))  # ty: ignore[invalid-argument-type]


def test_commands_are_frozen_strict_keyword_values_with_checked_replacement() -> None:
    for c, _ in command_vectors():
        constructor = getattr(messages, type(c).__name__)
        assert isinstance(c, messages.Message)
        fields = constructor.__annotations__
        assert constructor.__module__ == "meedee.messages"
        assert not hasattr(c, "__dict__")
        values = {k: getattr(c, k) for k in fields}
        rendered = ", ".join(f"{k}={v}" for k, v in values.items())
        assert repr(c) == f"{constructor.__name__}({rendered})"
        assert c == c.replace()
        assert hash(c) == hash(c.replace())
        with pytest.raises(TypeError):
            c.replace(unknown=1)
        with pytest.raises(TypeError):
            constructor(*values.values())
        for f in fields:
            assert fields[f] is int
            assert (
                inspect.signature(constructor).parameters[f].kind
                == inspect.Parameter.KEYWORD_ONLY
            )
            with pytest.raises(AttributeError):
                setattr(c, f, 1)
            for v in (True, 1.0, "1", None):
                with pytest.raises(TypeError):
                    constructor(**(values | {f: v}))
                with pytest.raises(TypeError):
                    c.replace(**{f: v})
    command = messages.NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
    assert command.replace(value=10001).value == 10001
    assert command.value == 10000
    with pytest.raises(ValueError):
        command.replace(resolution=7)
    assert command.replace(resolution=7, value=127).value == 127
    match command:
        case messages.NrpnSet(1, 1234, 10000, 14):
            pass
        case _:
            pytest.fail("command pattern did not match")


def test_command_ranges_and_null_parameter_are_explicit() -> None:
    for channel in (0, 17, 10**100):
        with pytest.raises(ValueError):
            messages.ControlChange14(channel=channel, controller=7, value=0)
    for controller in (-1, 32):
        with pytest.raises(ValueError):
            messages.ControlChange14(channel=1, controller=controller, value=0)
    for value in (-1, 16384):
        with pytest.raises(ValueError):
            messages.ControlChange14(channel=1, controller=0, value=value)
    for name in ("RpnSet", "NrpnSet"):
        constructor = getattr(messages, name)
        for resolution in (0, 8, 13, 15):
            with pytest.raises(ValueError):
                constructor(channel=1, parameter=0, value=0, resolution=resolution)
        with pytest.raises(ValueError):
            constructor(channel=1, parameter=0, value=128, resolution=7)
        with pytest.raises(TypeError):
            constructor(channel=1, parameter=0, value=0)
    for name in ("RpnSet", "RpnIncrement", "RpnDecrement"):
        fields = {"channel": 1, "parameter": 16383}
        if name == "RpnSet":
            fields |= {"value": 0, "resolution": 7}
        with pytest.raises(ValueError):
            getattr(messages, name)(**fields)
    assert (
        messages.NrpnSet(
            channel=16, parameter=16383, value=16383, resolution=14
        ).parameter
        == 16383
    )


def test_fine_control_observations_preserve_wire_order_and_source_ranges() -> None:
    interpreter = controls.Interpreter()

    def cc(channel: int, controller: int, value: int) -> object:
        return interpreter.feed(
            messages.ControlChange(channel=channel, controller=controller, value=value)
        )[0]

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


def test_selection_changes_cannot_reuse_selector_or_data_halves() -> None:
    interpreter = controls.Interpreter()

    def cc(controller: int, value: int) -> list[object]:
        return list(
            interpreter.feed(
                messages.ControlChange(channel=1, controller=controller, value=value)
            )
        )

    assert cc(99, 1) == []
    partial = cc(6, 2)[0]
    assert (
        isinstance(partial, controls.Diagnostic)
        and partial.kind == controls.DiagnosticKind.IncompleteSelection
    )
    assert cc(98, 2) == []
    coarse = cc(6, 3)[0]
    assert isinstance(coarse, controls.ParameterObservation)
    assert (coarse.family, coarse.parameter, coarse.value, coarse.resolution) == (
        controls.ParameterFamily.Nrpn,
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
    invalidated = cc(98, 3)[0]
    assert (
        isinstance(invalidated, controls.Diagnostic)
        and invalidated.kind == controls.DiagnosticKind.SelectionInvalidated
    )
    partial = cc(6, 7)[0]
    assert (
        isinstance(partial, controls.Diagnostic)
        and partial.kind == controls.DiagnosticKind.IncompleteSelection
    )
    assert cc(99, 1) == []
    missing = cc(38, 4)[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )
    cc(6, 7)
    cc(100, 2)
    partial = cc(38, 3)[0]
    assert (
        isinstance(partial, controls.Diagnostic)
        and partial.kind == controls.DiagnosticKind.IncompleteSelection
    )
    cc(101, 0)
    missing = cc(38, 1)[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )


def test_relative_steps_do_not_invent_absolute_values_or_increment_magnitudes() -> None:
    interpreter = controls.Interpreter()
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
        )[0]
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
    )[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingCoarse
    )


def test_rpn_null_deselects_nrpn_without_reserving_its_maximum_identifier() -> None:
    interpreter = controls.Interpreter()
    for controller in (99, 98):
        interpreter.feed(
            messages.ControlChange(channel=1, controller=controller, value=127)
        )
    observed = interpreter.feed(
        messages.ControlChange(channel=1, controller=6, value=127)
    )[0]
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
    )[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingSelection
    )


def test_resets_invalidate_channel_source_and_parameter_state() -> None:
    one = controls.Interpreter()
    two = controls.Interpreter()
    for channel in (1, 2):
        one.feed(messages.ControlChange(channel=channel, controller=7, value=2))
    fine = messages.ControlChange(channel=1, controller=39, value=1)
    assert isinstance(two.feed(fine)[0], controls.Diagnostic)
    reset = one.feed(messages.ControlChange(channel=1, controller=121, value=0))[0]
    assert isinstance(reset, controls.Diagnostic) and reset.channel == 1
    assert isinstance(one.feed(fine)[0], controls.Diagnostic)
    assert isinstance(one.feed(fine.replace(channel=2))[0], controls.ControlObservation)
    reset = one.feed(messages.SystemReset())[0]
    assert isinstance(reset, controls.Diagnostic) and reset.channel is None
    assert isinstance(one.feed(fine.replace(channel=2))[0], controls.Diagnostic)
    one.feed(messages.ControlChange(channel=1, controller=7, value=2))
    reset = one.reset()[0]
    assert (
        isinstance(reset, controls.Diagnostic)
        and reset.kind == controls.DiagnosticKind.StateReset
    )
    assert (reset.first_index, reset.last_index) == (8, 8)
    assert isinstance(one.feed(fine)[0], controls.Diagnostic)
    for command in codecs.expand(
        messages.NrpnSet(channel=1, parameter=0, value=1, resolution=14)
    )[:2]:
        one.feed(command)
    one.reset()
    missing = one.feed(messages.ControlChange(channel=1, controller=6, value=1))[0]
    assert (
        isinstance(missing, controls.Diagnostic)
        and missing.kind == controls.DiagnosticKind.MissingSelection
    )


def test_observations_are_frozen_owned_values_and_raw_input_is_unchanged() -> None:
    interpreter = controls.Interpreter()
    raw = messages.ControlChange(channel=1, controller=7, value=2)
    first = interpreter.feed(raw)[0]
    same = controls.Interpreter().feed(raw)[0]
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
        messages.ControlChange14(channel=1, controller=7, value=2),
    ):
        with pytest.raises(TypeError):
            interpreter.feed(value)  # ty: ignore[invalid-argument-type]
    fine = interpreter.feed(messages.ControlChange(channel=1, controller=39, value=1))[
        0
    ]
    assert isinstance(fine, controls.ControlObservation) and fine.last_index == 1
    for controller in (0, 32):
        assert (
            interpreter.feed(
                messages.ControlChange(channel=1, controller=controller, value=127)
            )
            == []
        )
    assert interpreter.feed(messages.SysEx(data=b"\x7d")) == []
