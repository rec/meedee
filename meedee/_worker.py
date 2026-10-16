"""Fresh-interpreter transport owner for the private, deadline-bounded pipe."""

import pickle
import sys
from queue import Empty, Full
from threading import Event, Lock, Thread
from typing import BinaryIO, cast

from . import messages, midi

# isort: split
from . import _native
from ._transport import ReceiveQueue
from ._wire import encode, encode_clock
from .events import ReceivedPacket


class BusyError(RuntimeError):
    pass


class Sender:
    def __init__(self, stream: BinaryIO) -> None:
        self.stream = stream
        self.lock = Lock()

    def send(self, tag: str, payload: object) -> None:
        with self.lock:
            pickle.dump((tag, payload), self.stream, protocol=5)
            self.stream.flush()


class Producer:
    def __init__(self, sender: Sender, generation: int, byte_limit: int) -> None:
        self.sender = sender
        self.generation = generation
        self.messages = ReceiveQueue(byte_limit)
        self.stopped = Event()
        self.dropped = 0
        self.connection: _native.InputConnection | None = None
        self.thread = Thread(target=self.emit, daemon=True, name="meedee forwarding")
        self.thread.start()

    def __call__(self, packet: ReceivedPacket) -> None:
        if not self.stopped.is_set():
            try:
                self.messages.put_nowait(encode(packet))
            except Full:
                self.dropped += 1
        else:
            self.dropped += 1

    def emit(self) -> None:
        try:
            while not self.stopped.is_set():
                try:
                    packet = self.messages.get(timeout=0.1)
                except Empty:
                    continue
                native_dropped = (
                    0 if self.connection is None else self.connection.dropped_packets
                )
                self.sender.send(
                    "message", (self.generation, packet, self.dropped + native_dropped)
                )
        except (OSError, ValueError):
            self.stopped.set()

    def stop(self) -> None:
        self.stopped.set()
        self.thread.join()
        while True:
            try:
                self.messages.get_nowait()
            except Empty:
                break
            self.dropped += 1


class Worker:
    def __init__(self, sender: Sender) -> None:
        self.sender = sender
        self.client: _native.Input | _native.Output | None = None
        self.connection: _native.InputConnection | _native.OutputConnection | None = (
            None
        )
        self.ports: list[midi.Port] = []
        self.producer: Producer | None = None
        self.generation = 0
        self.native_dropped = 0

    def perform(self, operation: str, args: list[object]) -> object:
        if operation == "init":
            input_port, settings = cast(tuple[bool, dict[str, object]], tuple(args))
            for k, c in (
                ("protocol", midi.Protocol),
                ("backend", midi.Backend),
                ("timestamps", midi.TimestampMode),
            ):
                if settings.get(k) is not None:
                    settings[k] = getattr(c, cast(str, settings[k]))
            self.client = (
                _native.Input(**settings) if input_port else _native.Output(**settings)
            )
            return None
        assert self.client is not None
        if operation in ("close", "shutdown"):
            if operation == "close" and args[0] != self.generation:
                return None
            if self.producer is not None:
                self.producer.stopped.set()
            if self.connection is not None:
                self.connection.close()
                if isinstance(self.connection, _native.InputConnection):
                    self.native_dropped = self.connection.dropped_packets
                self.connection = None
            if self.producer is not None:
                self.producer.stop()
            return None
        if operation in ("send", "send_command"):
            if args[1] != self.generation or not isinstance(
                self.connection, _native.OutputConnection
            ):
                raise BusyError("MIDI connection is closed")
            if operation == "send_command":
                name, fields, _ = cast(tuple[str, dict[str, int], int], args[0])
                if name == "BankProgramSelect":
                    from .gm import BankProgramSelect

                    command = BankProgramSelect(**fields)
                elif name == "ZoneConfiguration":
                    from .mpe import ZoneConfiguration

                    command = ZoneConfiguration(**fields)
                else:
                    command = getattr(messages, name)(**fields)
                self.connection.send_command(command)
                return None
            data = args[0]
            self.connection.send(
                midi.UmpPacket(words=cast(tuple[int, ...], data))
                if isinstance(data, tuple)
                else cast(bytes, data)
            )
            return None
        if self.connection is not None:
            raise BusyError("MIDI client is unavailable while connected")
        if operation == "ports":
            self.ports = self.client.ports()
            return [p._wire for p in self.ports]
        if operation in ("connect", "virtual"):
            wire, name, generation, byte_limit, clock_identity = cast(
                tuple[tuple[object, ...] | None, str, int, int, str], tuple(args)
            )
            self.generation = generation
            self.native_dropped = 0
            port = None
            if operation == "connect":
                matches = [p for p in self.ports if p._wire == wire]
                if len(matches) != 1:
                    raise midi.ConnectError(
                        "port belongs to another client "
                        "or an expired discovery snapshot"
                    )
                port = matches[0]
            if isinstance(self.client, _native.Input):
                self.producer = Producer(self.sender, generation, byte_limit)
                self.connection = (
                    self.client.create_virtual(name, self.producer, clock_identity)
                    if port is None
                    else self.client.connect(port, self.producer, clock_identity)
                )
                self.producer.connection = self.connection
                return encode_clock(self.connection.clock)
            self.connection = (
                self.client.create_virtual(name)
                if port is None
                else self.client.connect(port)
            )
            return None
        raise ValueError(f"Unknown MIDI operation: {operation}")


def serve(worker: Worker, source: BinaryIO) -> None:
    while True:
        try:
            operation, args = cast(tuple[str, list[object]], pickle.load(source))
        except EOFError:
            return
        try:
            result = worker.perform(operation, args)
        except BusyError as error:
            worker.sender.send(
                "error", ("StateError", error.args, False, dropped(worker))
            )
        except (
            midi.MidiError,
            RuntimeError,
            NotImplementedError,
            TypeError,
            ValueError,
            MemoryError,
        ) as error:
            fatal = (
                operation in ("connect", "virtual")
                or isinstance(error, (midi.MidiError, RuntimeError, MemoryError))
                and not isinstance(error, NotImplementedError)
            )
            if (
                operation == "ports"
                and sys.platform == "darwin"
                and isinstance(error, midi.PortInfoError)
            ):
                fatal = False
            kind = (
                "ResourceError"
                if isinstance(error, MemoryError)
                else type(error).__name__
            )
            worker.sender.send("error", (kind, error.args, fatal, dropped(worker)))
            if fatal:
                return
        else:
            worker.sender.send(
                "shutdown" if operation == "shutdown" else "ok",
                (result, dropped(worker)),
            )
            if operation == "shutdown":
                return


def dropped(worker: Worker) -> int:
    native = (
        worker.connection.dropped_packets
        if isinstance(worker.connection, _native.InputConnection)
        else worker.native_dropped
    )
    return native + (0 if worker.producer is None else worker.producer.dropped)


def main() -> None:
    serve(Worker(Sender(sys.stdout.buffer)), sys.stdin.buffer)


if __name__ == "__main__":
    main()
