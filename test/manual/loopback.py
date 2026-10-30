"""Opt-in software endpoints. These checks never select hardware ports."""

import subprocess
import sys
from collections.abc import Iterator
from contextlib import ExitStack
from functools import partial
from os import environ
from pathlib import Path
from queue import Queue
from unittest.mock import patch
from uuid import uuid4

import pytest

from meedee import midi
from meedee._clients import Input, Output
from meedee.events import ReceivedPacket


@pytest.fixture(params=[True, False], autouse=True)
def isolation(request: pytest.FixtureRequest) -> Iterator[bool]:
    with (
        patch.object(midi, "Input", partial(Input, isolated=request.param)),
        patch.object(midi, "Output", partial(Output, isolated=request.param)),
    ):
        yield request.param


class Collector:
    def __init__(self) -> None:
        self.packets: Queue[ReceivedPacket] = Queue()

    def __call__(self, packet: ReceivedPacket) -> None:
        self.packets.put_nowait(packet)

    def bytes_until(self, size: int) -> bytes:
        received = bytearray()
        while len(received) < size:
            packet = self.packets.get(timeout=5)
            assert isinstance(packet.packet, bytes)
            received.extend(packet.packet)
        return bytes(received)


@pytest.mark.parametrize("direction", ["input", "output"])
@pytest.mark.parametrize("protocol", [midi.Protocol.Midi1, midi.Protocol.Ump])
@pytest.mark.parametrize(
    "timestamps", [midi.TimestampMode.Absolute, midi.TimestampMode.Off]
)
def test_virtual_loopback_preserves_raw_packets_and_clock_metadata(
    direction: str, protocol: midi.Protocol, timestamps: midi.TimestampMode
) -> None:
    require_unix()
    receiver = Collector()
    source = midi.Input(protocol=protocol, timestamps=timestamps)
    target = midi.Output(protocol=protocol)
    token = f"meedee-{uuid4().hex}"
    with ExitStack() as stack:
        if direction == "input":
            incoming = stack.enter_context(source.create_virtual(token, receiver))
            port = next(p for p in target.ports() if p.name == token)
            outgoing = stack.enter_context(target.connect(port))
        else:
            outgoing = stack.enter_context(target.create_virtual(token))
            port = next(p for p in source.ports() if p.name == token)
            incoming = stack.enter_context(source.connect(port, receiver))
        data = (
            b"\x90\x3c\x7f"
            if protocol == midi.Protocol.Midi1
            else midi.UmpPacket(words=[0x40903C00, 0xFFFFFFFF])
        )
        outgoing.send_packet(data)
        packet = receiver.packets.get(timeout=5)
        assert packet.packet == data
        assert packet.clock == incoming.clock
        assert (packet.timestamp_ns is None) == (timestamps == midi.TimestampMode.Off)
        assert incoming.dropped_packets == 0
    assert incoming.closed and outgoing.closed


def test_duplicate_names_have_distinct_snapshots() -> None:
    require_unix()
    token = f"meedee-duplicate-{uuid4().hex}"
    first = midi.Output()
    second = midi.Output()
    observer = midi.Input()
    with first.create_virtual(token), second.create_virtual(token):
        ports = [p for p in observer.ports() if p.name == token]
        assert len(ports) == 2
        assert ports[0] != ports[1]
        for p in ports:
            with pytest.raises(TypeError):
                hash(p)


def test_successful_close_restores_client_and_reopen_uses_a_new_clock() -> None:
    require_unix()
    source = midi.Input()
    target = midi.Output()
    receiver = Collector()
    token = f"meedee-reopen-{uuid4().hex}"
    clocks = []
    for _ in range(2):
        with source.create_virtual(token, receiver) as incoming:
            clocks.append(incoming.clock)
            port = next(p for p in target.ports() if p.name == token)
            with pytest.raises(ValueError, match="body error"):
                with target.connect(port) as outgoing:
                    outgoing.send_packet(b"\xf8")
                    assert receiver.bytes_until(1) == b"\xf8"
                    raise ValueError("body error")
            assert outgoing.closed
    assert clocks[0] != clocks[1]


def test_disappeared_endpoint_fails_open_and_requires_a_fresh_client() -> None:
    require_unix()
    source = midi.Input()
    target = midi.Output()
    receiver = Collector()
    token = f"meedee-disappearance-{uuid4().hex}"
    with source.create_virtual(token, receiver):
        port = next(p for p in target.ports() if p.name == token)
    with pytest.raises((midi.MidiError, ValueError)):
        target.connect(port)
    with pytest.raises(midi.StateError):
        target.ports()
    with source.create_virtual(token, receiver):
        fresh = midi.Output()
        port = next(p for p in fresh.ports() if p.name == token)
        with fresh.connect(port) as outgoing:
            outgoing.send_packet(b"\x90\x3c\x7f")
            assert receiver.bytes_until(3) == b"\x90\x3c\x7f"


@pytest.mark.parametrize("size", [128, 1024, 16384])
def test_sysex_byte_stream_preserves_fragments_without_assembling_in_the_binding(
    size: int,
) -> None:
    require_unix()
    receiver = Collector()
    source = midi.Input()
    target = midi.Output()
    token = f"meedee-sysex-{uuid4().hex}"
    message = b"\xf0\x7d" + bytes(i % 128 for i in range(size - 3)) + b"\xf7"
    with source.create_virtual(token, receiver):
        port = next(p for p in target.ports() if p.name == token)
        with target.connect(port) as outgoing:
            outgoing.send_packet(message)
            assert receiver.bytes_until(size) == message


def test_burst_preserves_raw_byte_stream() -> None:
    require_unix()
    receiver = Collector()
    source = midi.Input()
    target = midi.Output()
    token = f"meedee-burst-{uuid4().hex}"
    messages = [bytes((0xB0, i // 128, i % 128)) for i in range(1000)]
    with source.create_virtual(token, receiver) as incoming:
        port = next(p for p in target.ports() if p.name == token)
        with target.connect(port) as outgoing:
            for m in messages:
                outgoing.send_packet(m)
            assert receiver.bytes_until(3000) == b"".join(messages)
        assert incoming.dropped_packets == 0


def test_native_interpreter_exit_drains_live_connections(isolation: bool) -> None:
    require_unix()
    subprocess.run(
        [sys.executable, str(Path(__file__).with_name("shutdown.py"))],
        check=True,
        timeout=20,
        env={**environ, "MEEDEE_TEST_ISOLATED": "1" if isolation else "0"},
    )


def require_unix() -> None:
    if sys.platform not in ("darwin", "linux"):
        pytest.skip("Virtual loopback requires CoreMIDI or ALSA")
