"""Controlled Rust-binding stand-ins for subprocess tests; no OS MIDI."""

from __future__ import annotations

import os
import sys
from collections.abc import Callable
from threading import Event, Thread
from typing import cast
from unittest.mock import patch

from meedee import codecs, midi
from meedee._worker import Sender, Worker, serve
from meedee.events import Clock, ReceivedPacket


class Input:
    def __init__(self, **settings: object) -> None:
        self.mode = os.environ.get("MEEDEE_TEST_MODE", "input")
        self.identity = str(settings["identity"])
        self.snapshot = 0
        self.protocol = settings.get("protocol", midi.Protocol.Midi1)
        self.timestamps = settings.get("timestamps", midi.TimestampMode.Absolute)
        if self.mode == "hang_init":
            Event().wait()

    def ports(self) -> list[midi.Port]:
        self.snapshot += 1
        if self.mode == "discovery_error_once" and self.snapshot == 1:
            raise midi.PortInfoError("endpoint disappeared during metadata read")
        return [
            midi.Port._from_wire(
                self.identity,
                self.snapshot,
                i,
                "input port",
                midi.Backend.CoreMidiUmp
                if self.protocol == midi.Protocol.Ump
                else midi.Backend.CoreMidi,
                0,
                True,
            )
            for i in range(2 if self.mode == "ambiguous" else 1)
        ]

    def connect(
        self,
        port: midi.Port,
        callback: Callable[[ReceivedPacket], object],
        clock_identity: str,
    ) -> InputConnection:
        connection = InputConnection(
            self.mode,
            clock_identity,
            self.timestamps,
            midi.Backend.CoreMidiUmp
            if self.protocol == midi.Protocol.Ump
            else midi.Backend.CoreMidi,
        )
        if self.mode == "failed_open":
            raise midi.ConnectError("device vanished during connect")
        if self.mode == "native_drop":

            def deliver_after_open_reply() -> None:
                assert connection.observed.wait(3)
                connection.native_dropped = 7
                callback(
                    ReceivedPacket(
                        timestamp_ns=17, clock=connection.clock, packet=b"\xf8"
                    )
                )

            Thread(target=deliver_after_open_reply, daemon=True).start()
        if self.mode in ("early", "twice", "burst"):
            payload = (
                midi.UmpPacket(words=[0x40903C00, 0xC0000000])
                if self.protocol == midi.Protocol.Ump
                else b"\x90\x3c\x7f"
            )
            callback(
                ReceivedPacket(
                    timestamp_ns=None
                    if self.timestamps == midi.TimestampMode.Off
                    else 17,
                    clock=connection.clock,
                    packet=payload,
                )
            )
        if self.mode == "twice":
            callback(
                ReceivedPacket(timestamp_ns=18, clock=connection.clock, packet=b"\xf8")
            )
        if self.mode == "burst":
            for i in range(2048):
                callback(
                    ReceivedPacket(
                        timestamp_ns=i, clock=connection.clock, packet=b"\xf8"
                    )
                )
        return connection

    def create_virtual(
        self,
        name: str,
        callback: Callable[[ReceivedPacket], object],
        clock_identity: str,
    ) -> InputConnection:
        return self.connect(self.ports()[0], callback, clock_identity)


class Output:
    def __init__(self, **settings: object) -> None:
        self.mode = os.environ.get("MEEDEE_TEST_MODE", "output")
        self.identity = str(settings["identity"])
        self.snapshot = 0
        self.protocol = settings.get("protocol", midi.Protocol.Midi1)
        if self.mode == "hang_init":
            Event().wait()

    def ports(self) -> list[midi.Port]:
        self.snapshot += 1
        if self.mode == "discovery_error_once" and self.snapshot == 1:
            raise midi.PortInfoError("endpoint disappeared during metadata read")
        return [
            midi.Port._from_wire(
                self.identity,
                self.snapshot,
                i,
                "output port",
                midi.Backend.CoreMidiUmp
                if self.protocol == midi.Protocol.Ump
                else midi.Backend.CoreMidi,
                0,
                False,
            )
            for i in range(2 if self.mode == "ambiguous" else 1)
        ]

    def connect(self, port: midi.Port) -> OutputConnection:
        if self.mode == "failed_open":
            raise midi.ConnectError("device vanished during connect")
        return OutputConnection(self.mode)

    def create_virtual(self, name: str) -> OutputConnection:
        return self.connect(self.ports()[0])


class InputConnection:
    def __init__(
        self, mode: str, identity: str, timestamps: object, backend: midi.Backend
    ) -> None:
        self.mode = mode
        self.closed = False
        self.native_dropped = 0
        self.observed = Event()
        self.clock = Clock(
            identity=identity,
            backend=backend,
            mode=cast(midi.TimestampMode, timestamps),
        )

    @property
    def dropped_packets(self) -> int:
        if self.mode == "native_drop" and not self.observed.is_set():
            self.observed.set()
            return 0
        return self.native_dropped

    def close(self) -> None:
        if self.mode == "hang_close":
            Event().wait()
        self.closed = True


class OutputConnection:
    def __init__(self, mode: str) -> None:
        self.mode = mode
        self.closed = False

    def send(self, packet: bytes | midi.UmpPacket) -> None:
        if self.mode == "hang_send":
            Event().wait()
        if self.mode == "crash_send":
            os._exit(19)
        if self.mode == "native_error":
            raise midi.SendError("device disconnected")
        if self.mode == "check_bytes" and packet != b"\xf0\x7d\x00\xff\xf7":
            raise midi.SendError("bytes changed in transport")

    def send_command(self, command: object) -> None:
        packets = [codecs.encode(p) for p in codecs.expand(command)]  # ty: ignore[invalid-argument-type]
        if self.mode.startswith("partial_command_"):
            accepted = int(self.mode.rpartition("_")[2])
            raise midi.CompoundSendError("device disconnected", accepted, len(packets))
        if self.mode in ("check_command", "check_bank", "check_mpe"):
            expected = bytes.fromhex(
                {
                    "check_command": (
                        "b0 63 09 b0 62 52 b0 06 4e b0 26 10 b0 65 7f b0 64 7f"
                    ),
                    "check_bank": "bf 00 79 bf 20 03 cf 7f",
                    "check_mpe": "bf 65 00 bf 64 06 bf 06 0f bf 65 7f bf 64 7f",
                }[self.mode]
            )
            if b"".join(packets) != expected:
                raise midi.CompoundSendError(
                    "command changed in transport", 0, len(packets)
                )
            return
        for index, packet in enumerate(packets):
            try:
                self.send(packet)
            except midi.SendError as error:
                raise midi.CompoundSendError(str(error), index, len(packets)) from error

    def close(self) -> None:
        if self.mode == "hang_close":
            Event().wait()
        self.closed = True


def main() -> None:
    from meedee import _native

    with (
        patch.object(_native, "Input", Input),
        patch.object(_native, "Output", Output),
        patch.object(_native, "InputConnection", InputConnection),
        patch.object(_native, "OutputConnection", OutputConnection),
    ):
        serve(Worker(Sender(sys.stdout.buffer)), sys.stdin.buffer)


if __name__ == "__main__":
    main()
