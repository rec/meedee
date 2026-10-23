"""Exact clocks, semantic commands and verified-prefix archive recovery."""

import struct
import zlib
from collections.abc import Sequence
from io import BytesIO
from pathlib import Path

import pytest

from meedee import archive, events, gm, messages, midi, mpe, ump


def values() -> list[object]:
    clock = events.Clock(
        identity="clock", tick_numerator=1001, tick_denominator=30000, origin="SMPTE"
    )
    time = events.Timestamp(ticks=-(2**63), clock=clock)
    span = events.SourceSpan(
        first_packet=1, last_packet=2, first_timestamp=time, last_timestamp=time
    )
    commands = [
        messages.ControlChange14(channel=1, controller=7, value=16383),
        messages.RpnSet(channel=2, parameter=0, value=8192, resolution=14),
        messages.NrpnSet(channel=3, parameter=16383, value=127, resolution=7),
        messages.RpnIncrement(channel=1, parameter=0),
        messages.RpnDecrement(channel=1, parameter=1),
        messages.NrpnIncrement(channel=1, parameter=2),
        messages.NrpnDecrement(channel=1, parameter=3),
        gm.BankProgramSelect(channel=1, bank_msb=121, bank_lsb=0, program=128),
        mpe.ZoneConfiguration(manager_channel=1, members=15),
    ]
    return [
        clock,
        archive.Stream(id="source", label="keys", clock_identity=clock.identity),
        events.Event(
            stream="source",
            timestamp=time,
            span=span,
            message=messages.NoteOn(channel=16, note=127, velocity=127),
        ),
        events.Event(
            stream="source",
            message=ump.Midi2PitchBend(group=16, channel=16, value=2**31 - 1),
        ),
        *[events.Event(stream="source", message=c) for c in commands],
        archive.Metadata(
            key="tempo", data=b"\x07\xa1\x20", stream="source", timestamp=time
        ),
        archive.Gap(stream="source", lost=2, reason="queue overflow"),
        archive.Discontinuity(stream="source", reason="clock reset"),
        archive.Opaque(kind=100, data=b"\x00\xff"),
    ]


def save(records: Sequence[object], complete: bool = True) -> bytes:
    stream = BytesIO()
    writer = archive.Writer(stream)
    for r in records:
        writer.write(r)  # ty: ignore[invalid-argument-type]
    writer.finish(complete=complete, reason="" if complete else "lost input")
    assert not stream.closed
    return stream.getvalue()


def test_archive_roundtrip_preserves_all_commands_exact_ticks_and_absence() -> None:
    records = values()
    data = save(records)
    assert data.startswith(b"MEEDEE\r\n\x00\x01\x00\x00")
    reader = archive.Reader(BytesIO(data), preserve_unknown=True)
    restored = list(reader)
    assert restored == records and reader.complete and reader.problem is None
    assert isinstance(restored[2], events.Event)
    assert restored[2].timestamp is not None
    assert restored[2].timestamp.ticks == -(2**63)
    assert save(restored) == data
    for cut in range(12, len(data)):
        recovery = archive.Reader(
            BytesIO(data[:cut]), recover=True, preserve_unknown=True
        )
        prefix = list(recovery)
        assert prefix == records[: len(prefix)]
        assert not recovery.complete and recovery.problem
    with pytest.raises(OSError, match="unknown record"):
        list(archive.Reader(BytesIO(data)))


def test_crc_completion_and_limits_cannot_be_mistaken_for_success() -> None:
    data = save(values()[:3])
    corrupt = bytearray(data)
    corrupt[20] ^= 1
    with pytest.raises(OSError, match="CRC"):
        list(archive.Reader(BytesIO(corrupt)))
    with pytest.raises(OSError, match="incomplete capture"):
        list(archive.Reader(BytesIO(save(values()[:3], complete=False))))
    recovery = archive.Reader(BytesIO(save(values()[:3], complete=False)), recover=True)
    assert len(list(recovery)) == 3 and "lost input" in str(recovery.problem)
    with pytest.raises(OSError, match="limit"):
        list(archive.Reader(BytesIO(data[:12] + struct.pack(">I", 2**32 - 1))))
    body = struct.pack(">H", 65535) + struct.pack(">Q", 0) + b"\x01\x00\x00\x00\x00"
    expected = (
        data[:12]
        + struct.pack(">I", len(body))
        + body
        + struct.pack(">I", zlib.crc32(body))
    )
    assert save([]) == expected
    writer = archive.Writer(BytesIO())
    with pytest.raises(OSError, match="undeclared"):
        writer.write(values()[2])  # ty: ignore[invalid-argument-type]
    with pytest.raises(OSError, match="failed"):
        writer.finish()


def test_clock_units_and_checked_values_are_distinct_from_raw_nanoseconds() -> None:
    clock = events.Clock(
        identity="c", tick_numerator=1001, tick_denominator=30000, origin="SMPTE"
    )
    time = events.Timestamp(ticks=30000, clock=clock)
    assert time.seconds == events.Rational(numerator=1001, denominator=1)
    with pytest.raises(TypeError):
        time.replace(ticks=True)
    with pytest.raises(AttributeError):
        time.ticks = 0  # ty: ignore[invalid-assignment]
    half = events.Rational(numerator=3, denominator=2)
    with pytest.raises(ValueError, match="rounding"):
        half.round()
    assert half.round(allow_rounding=True) == (
        2,
        events.Rational(numerator=1, denominator=2),
    )
    with pytest.raises(ValueError, match="nanosecond"):
        events.ReceivedPacket(timestamp_ns=1, clock=clock, packet=b"\xf8")
    off = events.Clock(
        identity="off", backend=midi.Backend.CoreMidi, mode=midi.TimestampMode.Off
    )
    with pytest.raises(ValueError, match="disabled"):
        events.Timestamp(ticks=1, clock=off)


def test_v1_independent_vectors_check_future_headers_and_preserve_exact_bytes() -> None:
    fixture = (
        Path(__file__).resolve().parents[1]
        / "crates/meedee/tests/fixtures/archive-v1.txt"
    )
    for line in fixture.read_text().splitlines():
        if line.startswith("#"):
            continue
        name, expected, encoded = line.split("|")
        data = bytes.fromhex(encoded)
        reader = archive.Reader(BytesIO(data), preserve_unknown=True)
        if expected != "valid":
            with pytest.raises(OSError, match=expected):
                list(reader)
            assert not reader.complete
            recovery = archive.Reader(
                BytesIO(data), preserve_unknown=True, recover=True
            )
            list(recovery)
            assert not recovery.complete and expected in str(recovery.problem)
            continue
        restored = list(reader)
        assert reader.complete and reader.problem is None, name
        assert save(restored) == data, name
        if name.startswith("future-"):
            assert isinstance(restored[-1], archive.Opaque)
            assert restored[-1].kind == 3
            with pytest.raises(OSError, match="unknown"):
                list(archive.Reader(BytesIO(data)))
            continue
        assert len(restored) == 19
        clock = restored[0]
        assert isinstance(clock, events.Clock)
        assert clock.tick_numerator == 1001 and clock.tick_denominator == 30000
        assert clock.backend == midi.Backend.CoreMidi
        assert clock.mode == midi.TimestampMode.Absolute and clock.resolution_ticks == 1
        note = restored[2]
        assert isinstance(note, events.Event) and note.timestamp is not None
        assert note.timestamp.ticks == -(2**63)
        assert note.message == messages.NoteOn(channel=16, note=127, velocity=127)
        assert note.span is not None and note.span.last_timestamp is not None
        assert note.span.first_packet == 1 and note.span.last_packet == 2
        assert note.span.last_timestamp.ticks == -1
        wide = restored[4]
        assert isinstance(wide, events.Event)
        assert wide.message == ump.Midi2ControlChange(
            group=16, channel=16, controller=7, value=0xFEDCBA98
        )
        assert [r.message for r in restored[5:14] if isinstance(r, events.Event)] == [
            r.message for r in values()[4:13] if isinstance(r, events.Event)
        ]
