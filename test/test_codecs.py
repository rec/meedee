"""Independent MIDI vectors and observable stream recovery across feed splits."""

import inspect
from collections.abc import Iterator
from pathlib import Path
from typing import cast

import pytest

from meedee import codecs, messages


def wire_vectors() -> Iterator[tuple[messages.Message, bytes]]:
    fixture = (
        Path(__file__).resolve().parents[1] / "crates/meedee/tests/fixtures/midi1.txt"
    )
    for line in fixture.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        fields, wire = line.split("|")
        name, *values = fields.split()
        constructor = getattr(messages, name)
        names = inspect.signature(constructor).parameters
        arguments = (
            {"data": bytes.fromhex(" ".join(values))}
            if name == "SysEx"
            else dict(zip(names, (int(v) for v in values), strict=True))
        )
        yield constructor(**arguments), bytes.fromhex(wire)


def observations(items: list[messages.Message | codecs.Diagnostic]) -> list[object]:
    return [
        (i.kind, i.offset, i.byte) if isinstance(i, codecs.Diagnostic) else i
        for i in items
    ]


def test_shared_wire_vectors_encode_and_decode_with_every_two_part_split() -> None:
    for m, b in wire_vectors():
        assert codecs.encode(m) == b
        for s in range(len(b) + 1):
            decoder = codecs.Decoder()
            assert decoder.feed(b[:s]) + decoder.feed(b[s:]) == [m]
            assert decoder.finish() == []


def test_new_messages_preserve_the_frozen_field_and_replacement_contract() -> None:
    class Integer(int):
        pass

    for m, _ in wire_vectors():
        if isinstance(m, (messages.NoteOn, messages.SysEx)):
            continue  # Their complete representation contract already has a fixture.
        constructor = getattr(messages, type(m).__name__)
        fields = constructor.__annotations__
        assert isinstance(m, messages.Message)
        assert constructor.__module__ == "meedee.messages"
        assert isinstance(constructor.__match_args__, tuple)
        assert constructor.__match_args__ == tuple(fields)
        assert not hasattr(m, "__dict__")
        values = {k: getattr(m, k) for k in fields}
        rendered = ", ".join(f"{k}={v}" for k, v in values.items())
        assert repr(m) == f"{constructor.__name__}({rendered})"
        assert hash(m) == hash(m.replace())
        assert m == m.replace()
        assert m != object()
        with pytest.raises(TypeError):
            m.replace(unknown=1)
        with pytest.raises(AttributeError):
            m.extra = 1
        for f in fields:
            assert fields[f] is int
            assert (
                inspect.signature(constructor).parameters[f].kind
                == inspect.Parameter.KEYWORD_ONLY
            )
            with pytest.raises(AttributeError):
                setattr(m, f, 1)
            for v in (True, False, 1.5, "1", None, Integer(1)):
                with pytest.raises(TypeError):
                    constructor(**(values | {f: v}))
                with pytest.raises(TypeError):
                    m.replace(**{f: v})
        if fields:
            with pytest.raises(TypeError):
                constructor(*values.values())
        with pytest.raises(TypeError):
            type("Subclass", (constructor,), {})
    note = messages.NoteOff(channel=1, note=60, velocity=64)
    assert note != messages.NoteOn(channel=1, note=60, velocity=64)
    assert note.replace(note=61).note == 61
    assert note.note == 60
    match note:
        case messages.NoteOff(1, 60, 64):
            pass
        case _:
            pytest.fail("message pattern did not match")


@pytest.mark.parametrize(
    ("name", "fields", "ranges"),
    [
        (
            "NoteOff",
            {"channel": 1, "note": 60, "velocity": 64},
            {"channel": (1, 16), "note": (0, 127), "velocity": (0, 127)},
        ),
        (
            "PolyPressure",
            {"channel": 1, "note": 60, "pressure": 64},
            {"channel": (1, 16), "note": (0, 127), "pressure": (0, 127)},
        ),
        (
            "ControlChange",
            {"channel": 1, "controller": 7, "value": 64},
            {"channel": (1, 16), "controller": (0, 127), "value": (0, 127)},
        ),
        (
            "ProgramChange",
            {"channel": 1, "program": 1},
            {"channel": (1, 16), "program": (1, 128)},
        ),
        (
            "ChannelPressure",
            {"channel": 1, "pressure": 64},
            {"channel": (1, 16), "pressure": (0, 127)},
        ),
        (
            "PitchBend",
            {"channel": 1, "value": 0},
            {"channel": (1, 16), "value": (-8192, 8191)},
        ),
        (
            "QuarterFrame",
            {"message_type": 0, "value": 0},
            {"message_type": (0, 7), "value": (0, 15)},
        ),
        ("SongPosition", {"position": 0}, {"position": (0, 16383)}),
        ("SongSelect", {"song": 0}, {"song": (0, 127)}),
    ],
)
def test_message_ranges_are_checked_in_construction_and_replacement(
    name: str, fields: dict[str, int], ranges: dict[str, tuple[int, int]]
) -> None:
    constructor = getattr(messages, name)
    message = constructor(**fields)
    for f, (low, high) in ranges.items():
        for v in (low, high):
            assert getattr(constructor(**(fields | {f: v})), f) == v
            assert getattr(message.replace(**{f: v}), f) == v
        for v in (low - 1, high + 1, 10**100, -(10**100)):
            with pytest.raises(ValueError):
                constructor(**(fields | {f: v}))
            with pytest.raises(ValueError):
                message.replace(**{f: v})


def test_running_status_and_all_realtime_statuses_preserve_partial_messages() -> None:
    decoder = codecs.Decoder()
    assert decoder.feed(b"\x90\x3c") == []
    events = decoder.feed(b"\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff\x64\x3d\x00")
    assert observations(events) == [
        messages.TimingClock(),
        (codecs.DiagnosticKind.UndefinedStatus, 3, 0xF9),
        messages.Start(),
        messages.Continue(),
        messages.Stop(),
        (codecs.DiagnosticKind.UndefinedStatus, 7, 0xFD),
        messages.ActiveSensing(),
        messages.SystemReset(),
        messages.NoteOn(channel=1, note=60, velocity=100),
        messages.NoteOn(channel=1, note=61, velocity=0),
    ]
    assert decoder.finish() == []


def test_common_and_reserved_statuses_clear_running_status_and_recover() -> None:
    decoder = codecs.Decoder()
    events = decoder.feed(
        bytes.fromhex("90 3c c0 07 f4 01 f5 02 f7 f1 7f 03 f2 01 02 f6")
    )
    assert observations(events) == [
        (codecs.DiagnosticKind.InterruptedMessage, 2, 0xC0),
        messages.ProgramChange(channel=1, program=8),
        (codecs.DiagnosticKind.UndefinedStatus, 4, 0xF4),
        (codecs.DiagnosticKind.UnexpectedData, 5, 1),
        (codecs.DiagnosticKind.UndefinedStatus, 6, 0xF5),
        (codecs.DiagnosticKind.UnexpectedData, 7, 2),
        (codecs.DiagnosticKind.UnexpectedEnd, 8, 0xF7),
        messages.QuarterFrame(message_type=7, value=15),
        (codecs.DiagnosticKind.UnexpectedData, 11, 3),
        messages.SongPosition(position=257),
        messages.TuneRequest(),
    ]


def test_one_data_byte_running_status_ends_at_system_common() -> None:
    decoder = codecs.Decoder()
    assert observations(
        decoder.feed(bytes.fromhex("cf 7f 00 df 00 7f f2 00 01 02"))
    ) == [
        messages.ProgramChange(channel=16, program=128),
        messages.ProgramChange(channel=16, program=1),
        messages.ChannelPressure(channel=16, pressure=0),
        messages.ChannelPressure(channel=16, pressure=127),
        messages.SongPosition(position=128),
        (codecs.DiagnosticKind.UnexpectedData, 9, 2),
    ]


def test_sysex_limit_is_exact_and_overflow_never_emits_a_truncated_payload() -> None:
    decoder = codecs.Decoder(sysex_limit=2)
    assert decoder.feed(b"\xf0\x01") == []
    assert decoder.feed(b"\xf8\x02\xf7") == [
        messages.TimingClock(),
        messages.SysEx(data=b"\x01\x02"),
    ]
    assert decoder.feed(b"\xf0\x01\x02") == []
    assert observations(decoder.feed(b"\x03\x04\xf8\x05\xf7\xf0\x06\xf7")) == [
        (codecs.DiagnosticKind.SysExTooLarge, 8, 3),
        messages.TimingClock(),
        messages.SysEx(data=b"\x06"),
    ]
    assert decoder.finish() == []


def test_oversized_sysex_finish_does_not_repeat_the_overflow_diagnostic() -> None:
    decoder = codecs.Decoder(sysex_limit=1)
    assert observations(decoder.feed(b"\xf0\x01\x02")) == [
        (codecs.DiagnosticKind.SysExTooLarge, 2, 2),
    ]
    assert decoder.finish() == []
    assert decoder.feed(b"\xf0\x03\xf7") == [messages.SysEx(data=b"\x03")]


def test_reserved_realtime_and_reset_preserve_sysex_payload() -> None:
    decoder = codecs.Decoder(sysex_limit=2)
    assert observations(decoder.feed(b"\xf0\x01\xf9\xff\xfd\x02\xf7")) == [
        (codecs.DiagnosticKind.UndefinedStatus, 2, 0xF9),
        messages.SystemReset(),
        (codecs.DiagnosticKind.UndefinedStatus, 4, 0xFD),
        messages.SysEx(data=b"\x01\x02"),
    ]


def test_nested_and_interrupted_sysex_restart_at_the_new_status() -> None:
    decoder = codecs.Decoder(sysex_limit=1)
    assert observations(
        decoder.feed(bytes.fromhex("f0 01 f0 02 90 3c 01 f0 01 02 c0 00"))
    ) == [
        (codecs.DiagnosticKind.InterruptedSysEx, 2, 0xF0),
        (codecs.DiagnosticKind.InterruptedSysEx, 4, 0x90),
        messages.NoteOn(channel=1, note=60, velocity=1),
        (codecs.DiagnosticKind.SysExTooLarge, 9, 2),
        messages.ProgramChange(channel=1, program=1),
    ]
    assert decoder.finish() == []


@pytest.mark.parametrize(
    "wire", [b"\x90", b"\x90\x3c", b"\xf1", b"\xf2\x01", b"\xf0\x7d"]
)
def test_finish_reports_truncation_once_and_clears_stream_state(wire: bytes) -> None:
    decoder = codecs.Decoder()
    assert decoder.feed(wire) == []
    kind = (
        codecs.DiagnosticKind.TruncatedSysEx
        if wire[0] == 0xF0
        else codecs.DiagnosticKind.TruncatedMessage
    )
    assert observations(decoder.finish()) == [(kind, len(wire), None)]
    assert decoder.finish() == []
    assert observations(decoder.feed(b"\x01")) == [
        (codecs.DiagnosticKind.UnexpectedData, len(wire), 1)
    ]


def test_reset_after_observed_loss_discards_running_status_and_partial_sysex() -> None:
    decoder = codecs.Decoder(sysex_limit=1)
    for b in (b"\x90\x3c", b"\xf0\x01", b"\xf0\x01\x02"):
        decoder.feed(b)
        decoder.reset()
        assert decoder.finish() == []
        assert all(isinstance(i, codecs.Diagnostic) for i in decoder.feed(b"\x01\x02"))
        assert decoder.feed(b"\x90\x3c\x01") == [
            messages.NoteOn(channel=1, note=60, velocity=1)
        ]


def test_byte_buffers_are_snapshotted_and_invalid_input_does_not_change_state() -> None:
    decoder = codecs.Decoder()
    assert decoder.feed(bytearray(b"\x90\x3c")) == []
    assert decoder.feed(b"") == []
    for d in ("text", [1], memoryview(b"abcd").cast("B", (2, 2))):
        with pytest.raises((TypeError, BufferError)):
            decoder.feed(d)  # ty: ignore[invalid-argument-type]
    assert decoder.feed(memoryview(b"\x64\x00")[::2]) == [
        messages.NoteOn(channel=1, note=60, velocity=100)
    ]
    source = bytearray(b"\xf0\x01")
    assert decoder.feed(source) == []
    source[1] = 2
    assert decoder.feed(b"\xf7") == [messages.SysEx(data=b"\x01")]


def test_diagnostics_are_frozen_values_and_not_constructible() -> None:
    first = codecs.Decoder().feed(b"\x01")[0]
    second = codecs.Decoder().feed(b"\x01")[0]
    assert isinstance(first, codecs.Diagnostic)
    assert first == second and hash(first) == hash(second)
    assert first != object()
    assert (
        repr(first)
        == "Diagnostic(kind=DiagnosticKind.UnexpectedData, offset=0, byte=1)"
    )
    with pytest.raises(AttributeError):
        first.offset = 7  # ty: ignore[invalid-assignment]
    with pytest.raises(TypeError):
        codecs.Diagnostic()


def test_codec_settings_and_encoder_reject_invalid_types() -> None:
    assert codecs.Decoder().sysex_limit == 1048576
    for v in (True, 1.0, "1", None):
        with pytest.raises(TypeError):
            codecs.Decoder(sysex_limit=v)  # ty: ignore[invalid-argument-type]
    for v in (0, -1, 10**100):
        with pytest.raises(ValueError):
            codecs.Decoder(sysex_limit=v)
    for m in (b"\x90\x3c\x64", object(), 1):
        with pytest.raises(TypeError):
            codecs.encode(cast(messages.Message, m))
