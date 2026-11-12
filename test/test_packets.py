"""Immutable transport values and pre-driver send validation, without MIDI I/O."""

import inspect

import pytest

from meedee import midi
from meedee._native import prepare_send
from meedee.events import Clock, ReceivedPacket


def test_ump_owns_words_and_preserves_every_bit() -> None:
    words = [0x40903C00, 0xFFFFFFFF]
    packet = midi.UmpPacket(words=words)
    words[0] = 0
    assert packet.words == (0x40903C00, 0xFFFFFFFF)
    assert packet == midi.UmpPacket(words=packet.words)
    assert hash(packet) == hash(midi.UmpPacket(words=packet.words))
    assert str(inspect.signature(midi.UmpPacket)) == "(*, words)"
    with pytest.raises(AttributeError):
        packet.words = ()  # ty: ignore[invalid-assignment]
    with pytest.raises(TypeError):
        packet.words[0] = 0  # ty: ignore[invalid-assignment]


@pytest.mark.parametrize("word", [True, 1.0, "1", None])
def test_ump_rejects_integer_coercion(word: object) -> None:
    with pytest.raises(TypeError):
        midi.UmpPacket(words=[word])  # ty: ignore[invalid-argument-type]


@pytest.mark.parametrize("word", [-1, 2**32, 2**100])
def test_ump_rejects_words_outside_u32(word: int) -> None:
    with pytest.raises(ValueError):
        midi.UmpPacket(words=[word])


@pytest.mark.parametrize("timestamp", [None, 0, -1, -(2**63), 2**63 - 1])
@pytest.mark.parametrize(
    "packet", [b"\xf0\x01", midi.UmpPacket(words=[0x40903C00, 0xFFFFFFFF])]
)
def test_receive_values_preserve_signed_time_absence_and_raw_payload(
    timestamp: int | None, packet: bytes | midi.UmpPacket
) -> None:
    clock = Clock(
        identity="stream-one",
        backend=midi.Backend.CoreMidi,
        mode=midi.TimestampMode.Absolute,
    )
    event = ReceivedPacket(timestamp_ns=timestamp, clock=clock, packet=packet)
    assert event.timestamp_ns == timestamp and event.packet == packet
    assert event.clock == clock
    assert (clock.tick_numerator, clock.tick_denominator, clock.resolution_ns) == (
        1,
        1_000_000_000,
        None,
    )
    with pytest.raises(AttributeError):
        event.timestamp_ns = 99  # ty: ignore[invalid-assignment]
    with pytest.raises(AttributeError):
        event.clock = clock  # ty: ignore[invalid-assignment]


def test_disabled_timestamps_require_absence_and_clocks_distinguish_generations() -> (
    None
):
    one = Clock(
        identity="one", backend=midi.Backend.CoreMidi, mode=midi.TimestampMode.Off
    )
    two = Clock(
        identity="two", backend=midi.Backend.CoreMidi, mode=midi.TimestampMode.Off
    )
    assert one != two
    assert ReceivedPacket(timestamp_ns=None, clock=one, packet=b"").timestamp_ns is None
    with pytest.raises(ValueError):
        ReceivedPacket(timestamp_ns=0, clock=one, packet=b"")


@pytest.mark.parametrize("value", [True, 1.0, 2**63, -(2**63) - 1])
def test_receive_time_rejects_coercion_and_overflow(value: object) -> None:
    clock = Clock(
        identity="one", backend=midi.Backend.CoreMidi, mode=midi.TimestampMode.Absolute
    )
    with pytest.raises((TypeError, ValueError)):
        ReceivedPacket(timestamp_ns=value, clock=clock, packet=b"\xf8")  # ty: ignore[invalid-argument-type]


def test_send_snapshots_unsigned_byte_buffers_including_strides() -> None:
    source = bytearray(b"abc")
    packet = prepare_send(midi.Encoding.Bytes, source)
    source[0] = 0
    assert packet == b"abc"
    assert prepare_send(midi.Encoding.Bytes, memoryview(b"axbxc")[::2]) == b"abc"


@pytest.mark.parametrize(
    "encoding, packet",
    [
        (midi.Encoding.Bytes, b""),
        (midi.Encoding.Ump, b"\xf8"),
        (midi.Encoding.Bytes, midi.UmpPacket(words=[0x20903C64])),
        (midi.Encoding.Ump, midi.UmpPacket(words=[0x40903C00])),
    ],
)
def test_send_rejects_wrong_protocol_empty_payload_and_truncated_ump(
    encoding: midi.Encoding, packet: object
) -> None:
    with pytest.raises(ValueError):
        prepare_send(encoding, packet)
