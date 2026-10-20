"""Passive captures use bounded shared state and existing production decoders."""

from io import BytesIO
from pathlib import Path

import pytest

from meedee import events, messages, testing, transfer, transfer_input, ump


def event(message: messages.Message, stream: str = "s") -> events.Event:
    return events.Event(stream=stream, message=message)


def capture(count: int = 2, timeout: int = 100) -> transfer.Capture:
    return transfer.Capture(stream="s", expected_messages=count, timeout_ns=timeout)


def test_capture_requires_count_before_export_and_refuses_overwrite(
    tmp_path: Path,
) -> None:
    c = capture()
    path = tmp_path / "dump.syx"
    with pytest.raises(ValueError, match="completed"):
        c.write(path)
    assert not path.exists()
    c.start(0)
    c.feed(event(messages.TimingClock()), 1)
    c.feed(event(messages.SysEx(data=b"\x7d\x00")), 2)
    saved = c.snapshot()
    c.feed(event(messages.SysEx(data=b"")), 3)
    report = c.snapshot()
    assert report.status == "completed" and report.wire_bytes == 6
    assert report.ignored_events == 1 and saved.received_messages == 1
    assert c.message(0).data == b"\x7d\x00"
    with pytest.raises(IndexError):
        c.message(2)
    stream = BytesIO()
    c.write(stream)
    assert not stream.closed and stream.getvalue() == b"\xf0\x7d\x00\xf7\xf0\xf7"
    c.write(path)
    with pytest.raises(OSError):
        c.write(path)
    assert path.read_bytes() == stream.getvalue()
    with pytest.raises(AttributeError):
        report.status = "active"  # ty: ignore[invalid-assignment]


def test_capture_deadline_and_protocol_failures_preserve_the_prefix() -> None:
    c = capture()
    c.start(5)
    c.feed(event(messages.SysEx(data=b"\x01")), 6)
    with pytest.raises(ValueError, match="regressed"):
        c.poll(5)
    c.feed(event(messages.SysEx(data=b"\x02")), 105)
    assert c.snapshot().status == "timed_out"
    assert c.snapshot().received_messages == 1
    with pytest.raises(ValueError):
        c.write(BytesIO())
    for item in (
        event(messages.SysEx(data=b""), stream="other"),
        event(ump.Midi1Packet(group=1, message=messages.TimingClock())),
    ):
        c = capture(1)
        c.start(0)
        c.feed(item, 1)
        assert c.snapshot().status == "failed" and c.snapshot().received_messages == 0
    with pytest.raises(ValueError):
        transfer.Capture(stream="s", expected_messages=3, timeout_ns=1, byte_limit=4)
    with pytest.raises(TypeError):
        transfer.Capture(stream="s", expected_messages=True, timeout_ns=1)


def test_receive_adapter_reuses_fragment_assembly_without_closing_input(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    clock = events.Clock(identity="source")
    for p in (b"\xf0\x7d", b"\xf8\x01\xf7\xf0\x02\xf7"):
        receiver.inject(p, clock=clock)
    monkeypatch.setattr(transfer_input, "monotonic_ns", lambda: 0)
    c = capture()
    report = transfer_input.run(c, receiver)  # ty: ignore[invalid-argument-type]
    assert report.status == "completed" and report.ignored_events == 1
    assert c.message(0).data == b"\x7d\x01" and c.message(1).data == b"\x02"
    assert not receiver.closed
    receiver.close()


@pytest.mark.parametrize("fault", ["loss", "malformed", "worker", "cancel", "timeout"])
def test_receive_stops_on_faults_cancellation_and_silence(
    fault: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    clock = events.Clock(identity="source")
    token = events.Cancellation()
    time = testing.ManualClock()
    if fault == "loss":
        receiver.lose(1)
    elif fault == "malformed":
        receiver.inject(b"\xf0\x01\x90\x40\x01", clock=clock)
    elif fault == "worker":
        receiver.fail("input failed")
    elif fault == "cancel":
        token.cancel()
    else:
        time.advance(100)
    ticks = iter([0])
    monkeypatch.setattr(
        transfer_input, "monotonic_ns", lambda: next(ticks, time.now_ns)
    )
    c = capture(1)
    report = transfer_input.run(c, receiver, cancel=token)  # ty: ignore[invalid-argument-type]
    assert report.status == {"cancel": "cancelled", "timeout": "timed_out"}.get(
        fault, "failed"
    )
    assert report.received_messages == 0 and not receiver.closed
    receiver.close()


@pytest.mark.parametrize("fault", ["loss", "worker", "interrupt"])
def test_final_receive_cannot_hide_input_failure(
    fault: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import midi

    class Receiver:
        dropped_packets = 0
        failure: Exception | None = None

        def receive(self, *, timeout: float) -> events.Event:
            if fault == "interrupt":
                raise KeyboardInterrupt
            if fault == "loss":
                self.dropped_packets = 1
            else:
                self.failure = midi.WorkerError("failed during receive")
            return event(messages.SysEx(data=b"\x01"))

    monkeypatch.setattr(transfer_input, "monotonic_ns", lambda: 0)
    c = capture(1)
    report = transfer_input.run(c, Receiver())  # ty: ignore[invalid-argument-type]
    assert report.status == ("cancelled" if fault == "interrupt" else "failed")
    assert report.received_messages == 0
