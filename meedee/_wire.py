"""Explicit, immutable receive values on the private local worker pipe."""

from typing import cast

from . import midi
from .events import Clock, ReceivedPacket


def encode(packet: ReceivedPacket) -> tuple[object, ...]:
    data = packet.packet
    return (
        packet.timestamp_ns,
        *encode_clock(packet.clock),
        packet.sequence,
        data.words if isinstance(data, midi.UmpPacket) else data,
    )


def encode_clock(clock: Clock) -> tuple[str, str, str]:
    return (
        clock.identity,
        str(clock.backend).rpartition(".")[2],
        str(clock.mode).rpartition(".")[2],
    )


def decode_clock(wire: tuple[object, ...]) -> Clock:
    identity, backend, mode = wire
    return Clock(
        identity=cast(str, identity),
        backend=getattr(midi.Backend, cast(str, backend)),
        mode=getattr(midi.TimestampMode, cast(str, mode)),
    )


def decode(wire: tuple[object, ...]) -> ReceivedPacket:
    timestamp, identity, backend, mode, sequence, data = wire
    clock = decode_clock((identity, backend, mode))
    return ReceivedPacket(
        timestamp_ns=cast(int | None, timestamp),
        sequence=cast(int | None, sequence),
        clock=clock,
        packet=midi.UmpPacket(words=cast(tuple[int, ...], data))
        if isinstance(data, tuple)
        else cast(bytes, data),
    )


def payload_size(wire: tuple[object, ...]) -> int:
    data = cast(bytes | tuple[int, ...], wire[-1])
    return len(data) * (4 if isinstance(data, tuple) else 1)
