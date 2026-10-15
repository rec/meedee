"""Shared profile vectors and immutable lookup data; no device I/O."""

import inspect
from collections.abc import Iterator
from pathlib import Path

import pytest

from meedee import codecs, controls, gm, messages, mpe, timecode, universal


def profile_vectors() -> Iterator[tuple[object, bytes]]:
    fixture = (
        Path(__file__).resolve().parents[1]
        / "crates/meedee/tests/fixtures/profiles.txt"
    )
    modules = {
        "BankProgramSelect": gm,
        "ZoneConfiguration": mpe,
        "Gm1On": gm,
        "GmOff": gm,
        "Gm2On": gm,
        "IdentityRequest": universal,
        "MasterVolume": universal,
        "MasterFineTuning": universal,
        "MasterCoarseTuning": universal,
        "FullFrame": timecode,
    }
    for line in fixture.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        fields, wire = line.split("|")
        name, *numbers = fields.split()
        constructor = getattr(modules[name], name)
        values: dict[str, object] = dict(
            zip(
                inspect.signature(constructor).parameters,
                (int(n) for n in numbers),
                strict=True,
            )
        )
        if name == "FullFrame":
            values["rate"] = timecode.FrameRate.Fps30
        yield constructor(**values), bytes.fromhex(wire)


def test_shared_vectors_and_explicit_interpretation_preserve_raw_messages() -> None:
    for v, b in profile_vectors():
        assert isinstance(v, messages.Message)
        if isinstance(v, (gm.BankProgramSelect, mpe.ZoneConfiguration)):
            primitives = codecs.expand(v)
            assert b"".join(codecs.encode(p) for p in primitives) == b
            if isinstance(v, gm.BankProgramSelect):
                assert isinstance(primitives[-1], messages.ProgramChange)
            with pytest.raises(TypeError):
                codecs.encode(v)
        else:
            assert codecs.encode(v) == b
            raw = messages.SysEx(data=b[1:-1])
            assert codecs.Decoder().feed(b) == [raw]
            assert v in (gm.decode(raw), universal.decode(raw), timecode.decode(raw))
            if not isinstance(v, (gm.Gm1On, gm.GmOff, gm.Gm2On)):
                assert controls.Interpreter().feed(v) == []
            for i in range(3, len(raw.data)):
                truncated = messages.SysEx(data=raw.data[:i])
                with pytest.raises(ValueError):
                    if isinstance(v, (gm.Gm1On, gm.GmOff, gm.Gm2On)):
                        gm.decode(truncated)
                    elif isinstance(v, timecode.FullFrame):
                        timecode.decode(truncated)
                    else:
                        universal.decode(truncated)


def test_native_profile_values_are_frozen_strict_and_support_checked_replacement() -> (
    None
):
    for value, _ in profile_vectors():
        cls = getattr(
            {
                "meedee.gm": gm,
                "meedee.mpe": mpe,
                "meedee.universal": universal,
                "meedee.timecode": timecode,
            }[type(value).__module__],
            type(value).__name__,
        )
        fields = cls.__annotations__
        v = cls(**{k: getattr(value, k) for k in fields})
        assert v == value
        assert v == v.replace()
        assert hash(v) == hash(v.replace())
        assert not hasattr(v, "__dict__")
        assert cls.__match_args__ == tuple(fields)
        assert repr(v).startswith(cls.__name__ + "(")
        with pytest.raises(TypeError):
            v.replace(unknown=1)
        for f in fields:
            assert (
                inspect.signature(cls).parameters[f].kind
                == inspect.Parameter.KEYWORD_ONLY
            )
            with pytest.raises(AttributeError):
                setattr(v, f, 0)
            if fields[f] is int:
                for x in (True, 1.0, "1", None):
                    with pytest.raises(TypeError):
                        v.replace(**{f: x})
    for kwargs in (
        {"manager_channel": 2, "members": 1},
        {"manager_channel": 1, "members": 16},
    ):
        with pytest.raises(ValueError):
            mpe.ZoneConfiguration(**kwargs)
    match gm.BankProgramSelect(channel=16, bank_msb=121, bank_lsb=3, program=128):
        case gm.BankProgramSelect(16, 121, 3, 128):
            pass
        case _:
            pytest.fail("command pattern did not match")


def test_gm_lookup_tables_are_complete_independent_copies_without_bank_guesses() -> (
    None
):
    assert gm.GM1_DRUM_CHANNEL == 10
    assert gm.GM2_ON_SETTLING_NS == 100_000_000
    all_programs = gm.programs()
    assert len(all_programs) == 128
    assert all_programs[0] == gm.program(program=1)
    assert all_programs[-1].name == "Gunshot"
    assert not hasattr(all_programs[0], "bank_msb")
    assert gm.program(program=65).family == "Reed"
    all_programs.clear()
    assert len(gm.programs()) == 128
    matches = gm.find_programs("  acoustic GRAND piano ")
    assert len(matches) == 2
    assert isinstance(matches[0], gm.Program)
    assert isinstance(matches[1], gm.BankProgram)
    assert gm.find_programs("piano") == []
    voices = gm.gm2_programs()
    assert len({(v.bank_msb, v.bank_lsb, v.program) for v in voices}) == 256
    phase = gm.gm2_program(bank_lsb=4, program=6)
    assert phase is not None and phase.name == "EP Phase"
    assert gm.gm2_program(bank_lsb=127, program=1) is None
    assert len(gm.drum_kits()) == 9
    assert gm.find_drum_kits(" room SET ")[0].program == 9
    power = gm.gm2_percussion(program=17, note=36)
    assert power is not None and power.name == "Power Kick Drum"
    analog = gm.gm2_percussion(program=26, note=75)
    assert analog is not None and analog.name == "Analog Claves"
    assert gm.gm2_percussion(program=57, note=27) is None
    assert gm.gm2_percussion(program=2, note=36) is None
    assert len([n for n in range(128) if gm.percussion(note=n) is not None]) == 47
    for entry in [gm.program(program=1), phase, gm.drum_kits()[0], power]:
        with pytest.raises(AttributeError):
            entry.name = "changed"  # ty: ignore[invalid-assignment]
    assert hash(gm.program(program=1)) == hash(gm.program(program=1))
    with pytest.raises(ValueError):
        gm.program(program=0)
    with pytest.raises(TypeError):
        gm.program(program=True)


def test_identity_reply_keeps_raw_revision_bytes_and_owns_bounded_inputs() -> None:
    for manufacturer in ([65], [0, 32, 51]):
        revision = [0, 1, 2, 127]
        value = universal.IdentityReply(
            device_id=1,
            manufacturer=manufacturer,
            family=257,
            model=16383,
            revision=revision,
        )
        revision.clear()
        wire = codecs.encode(value)
        assert wire[-9:] == bytes([1, 2, 127, 127, 0, 1, 2, 127, 247])
        assert universal.decode(messages.SysEx(data=wire[1:-1])) == value
        assert isinstance(value.manufacturer, tuple)
        assert value.revision == (0, 1, 2, 127)
        assert value == value.replace()
        with pytest.raises(TypeError):
            value.replace(revision=[True, 0, 0, 0])
        with pytest.raises(ValueError):
            value.replace(manufacturer=[0])
    with pytest.raises(ValueError):
        universal.decode(messages.SysEx(data=bytes.fromhex("7f 01 04 04 01 40")))
    assert universal.decode(messages.SysEx(data=b"\x7f\x01\x04\x7f")) is None


def test_named_controls_keep_distinct_units_and_reset_on_explicit_input() -> None:
    cc = controls.Controller.ChannelVolume.message(channel=16, value=100)
    assert cc == messages.ControlChange(channel=16, controller=7, value=100)
    assert controls.Controller.ChannelVolume.number == 7
    assert controls.local_control(channel=1, enabled=False).value == 0
    assert controls.local_control(channel=1, enabled=True).value == 127
    assert (
        controls.pitch_bend_sensitivity(channel=1, semitones=2, cents=50).value == 306
    )
    assert controls.channel_fine_tuning(channel=1, value=0).value == 8192
    assert controls.channel_coarse_tuning(channel=1, semitones=-64).value == 0
    assert controls.modulation_depth_range(channel=1, value=64).value == 64
    assert controls.tuning_program(channel=1, program=127).parameter == 3
    assert controls.tuning_bank(channel=1, bank=0).parameter == 4
    with pytest.raises(TypeError):
        controls.local_control(channel=1, enabled=1)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        controls.mono_mode(channel=1, channels=17)
    interpreter = controls.Interpreter()
    assert interpreter.feed(messages.ControlChange(channel=1, controller=7, value=1))
    reset = controls.reset_all_controllers(channel=1)
    # Constructing a value does not reset interpretation state.
    assert interpreter.feed(messages.ControlChange(channel=1, controller=39, value=2))
    diagnostic = interpreter.feed(reset)[0]
    assert isinstance(diagnostic, controls.Diagnostic)
    assert diagnostic.kind == controls.DiagnosticKind.StateReset
    for mode in (gm.Gm1On(device_id=1), gm.Gm2On(device_id=1), gm.GmOff(device_id=1)):
        interpreter.feed(messages.ControlChange(channel=1, controller=7, value=1))
        diagnostic = interpreter.feed(mode)[0]
        assert isinstance(diagnostic, controls.Diagnostic)
        assert diagnostic.kind == controls.DiagnosticKind.StateReset
        diagnostic = interpreter.feed(
            messages.ControlChange(channel=1, controller=39, value=2)
        )[0]
        assert isinstance(diagnostic, controls.Diagnostic)
        assert diagnostic.kind == controls.DiagnosticKind.MissingCoarse


def test_full_frame_rates_preserve_labels_without_inventing_a_running_clock() -> None:
    for rate, frames in (
        (timecode.FrameRate.Fps24, 23),
        (timecode.FrameRate.Fps25, 24),
        (timecode.FrameRate.DropFrame30, 29),
        (timecode.FrameRate.Fps30, 29),
    ):
        value = timecode.FullFrame(
            device_id=127, rate=rate, hours=23, minutes=59, seconds=59, frames=frames
        )
        wire = codecs.encode(value)
        assert timecode.decode(messages.SysEx(data=wire[1:-1])) == value
        with pytest.raises(ValueError):
            value.replace(frames=frames + 1)
        with pytest.raises(TypeError):
            value.replace(rate=3)  # ty: ignore[invalid-argument-type]
