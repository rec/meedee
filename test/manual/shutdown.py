"""Native interpreter-exit fixture, invoked only by the opt-in loopback test."""

import atexit
from os import environ
from threading import Event
from uuid import uuid4

from middi import midi
from middi.events import ReceivedPacket

started = Event()
release = Event()


def receive(packet: ReceivedPacket) -> None:
    started.set()
    if not release.wait(10):
        raise RuntimeError("native shutdown test release deadline exceeded")


def open_connections() -> tuple[midi.Input[ReceivedPacket], midi.Output]:
    token = f"middi-shutdown-{uuid4().hex}"
    run_in_subprocess = environ.get("MIDDI_TEST_ISOLATED", "1") == "1"
    source = midi.Input(raw=True, run_in_subprocess=run_in_subprocess)
    incoming = source.create_virtual(token, receive)
    target = midi.Output(run_in_subprocess=run_in_subprocess)
    (port,) = (p for p in target.ports() if token in p.name)
    outgoing = target.connect(port)
    outgoing(b"\x90\x3c\x7f")
    if not started.wait(5):
        raise RuntimeError("native callback did not start")
    for _ in range(100):
        outgoing(b"\x90\x3d\x01")
    # This handler runs before the extension's earlier cleanup handler.
    atexit.register(release.set)
    return incoming, outgoing


if __name__ == "__main__":
    # Keep both native connections live until CPython runs its exit handlers.
    incoming, outgoing = open_connections()
