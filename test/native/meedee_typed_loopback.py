"""Opt-in CoreMIDI flow. Connect exclusively to this process's virtual endpoint."""

from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Thread
from uuid import uuid4

from meedee import archive, events, messages, midi


def exercise(isolated: bool, destination: Path) -> None:
    name = f"meedee-typed-test-{uuid4().hex}"
    with midi.TypedInput(isolated=isolated).create_virtual(name) as receiver:
        sender = midi.TypedOutput(isolated=isolated)
        matches = [p for p in sender.ports() if p.name == name]
        assert len(matches) == 1, "test-owned endpoint was not discovered uniquely"
        with sender.connect(matches[0]) as output:
            receiver.start_recording(destination)
            notes = [
                messages.NoteOn(channel=1, note=60, velocity=1),
                messages.NoteOff(channel=1, note=60, velocity=0),
            ]
            for n in notes:
                output.send_message(n)
            received = [receiver.receive(timeout=3) for _ in notes]
            assert all(isinstance(e, events.Event) for e in received), received
            assert [e.message for e in received if isinstance(e, events.Event)] == notes
            report = receiver.stop_recording()
            assert report.complete and report.gap_records == 0, report.failure
            reader = archive.Reader(destination)
            records = list(reader)
            assert reader.complete
            reader.close()
            captured = [e for e in records if isinstance(e, events.Event)]
            assert len(captured) == 2 and captured[0].timestamp is not None
            replay = output.play(captured, origin=captured[0].timestamp)
            assert replay.sent_events == 2 and not replay.cancelled
            assert [
                e.message
                for _ in notes
                if isinstance(e := receiver.receive(timeout=3), events.Event)
            ] == notes
            cancel = events.Cancellation()
            failures: list[BaseException] = []

            def wait() -> None:
                try:
                    receiver.receive(cancel=cancel)
                except InterruptedError as error:
                    failures.append(error)

            waiter = Thread(target=wait)
            waiter.start()
            cancel.cancel()
            waiter.join(2)
            assert not waiter.is_alive() and len(failures) == 1
    print(f"typed capture/replay/receive cancellation passed: isolated={isolated}")


def main() -> None:
    with TemporaryDirectory(prefix="meedee-typed-loopback-") as directory:
        for isolated in (True, False):
            exercise(isolated, Path(directory) / f"capture-{isolated}.meedee")


if __name__ == "__main__":
    main()
