"""One-shot MTS queries preserve raw replies and never retry uncertain sends."""

from io import BytesIO
from pathlib import Path

import pytest

from meedee import codecs, events, messages, testing, tuning, tuning_live


def query(bank: int | None = None) -> tuning.Query:
    request = (
        tuning.BulkDumpRequest(device_id=1, program=2)
        if bank is None
        else tuning.BankedDumpRequest(device_id=1, bank=bank, program=2)
    )
    return tuning.Query(request=request, stream="s", timeout_ns=10)


def reply(bank: int | None = None) -> bytes:
    table = tuning.TuningTable(values=[tuning.NoChange() for _ in range(128)])
    value = (
        tuning.BulkDump(device_id=1, program=2, name="dump", table=table)
        if bank is None
        else tuning.KeyBasedDump(
            device_id=1, bank=bank, program=2, name="dump", table=table
        )
    )
    return codecs.encode(value)


def test_query_exports_original_checksum_bytes_and_freezes_snapshots(
    tmp_path: Path,
) -> None:
    q = query()
    path = tmp_path / "reply.syx"
    with pytest.raises(ValueError, match="completed"):
        q.write(path)
    assert not path.exists()
    assert codecs.encode(q.prepare(0)) == b"\xf0\x7e\x01\x08\x00\x02\xf7"
    q.confirm(1)
    q.feed(events.Event(stream="s", message=messages.TimingClock()), 2)
    saved = q.snapshot()
    raw = bytearray(reply())
    raw[-2] ^= 1
    q.feed(events.Event(stream="s", message=messages.SysEx(data=raw[1:-1])), 3)
    report = q.snapshot()
    assert (
        report.status == "completed"
        and report.request_sent
        and not report.uncertain_send
    )
    assert report.reply is not None and codecs.encode(report.reply) == raw
    assert (
        report.decoded is not None
        and report.decoded.checksum == tuning.ChecksumStatus.LegacyMismatch
    )
    assert (
        saved.status == "waiting" and saved.reply is None and saved.ignored_events == 1
    )
    with pytest.raises(AttributeError):
        report.status = "ready"  # ty: ignore[invalid-assignment]
    q.write(path)
    assert path.read_bytes() == raw
    with pytest.raises(OSError):
        q.write(path)
    stream = BytesIO()
    q.write(stream)
    assert stream.getvalue() == raw and not stream.closed


@pytest.mark.parametrize("bank", [None, 3])
def test_host_reuses_typed_decoding_and_existing_reply_matcher(
    bank: int | None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    output = testing.Output(protocol="midi1")
    clock = events.Clock(identity="c")
    raw = reply(bank)
    receiver.inject(b"\xf0\x7d\xf7\xf8" + raw[:30], clock=clock)
    receiver.inject(raw[30:], clock=clock)
    monkeypatch.setattr(tuning_live, "monotonic_ns", lambda: 0)
    q = query(bank)
    report = tuning_live.run(q, receiver, output)  # ty: ignore[invalid-argument-type]
    assert report.status == "completed" and report.ignored_events == 2
    assert not receiver.closed and not output.snapshot().closed
    assert output.snapshot().accepted == 1 and output.take_packet() is not None
    assert report.reply is not None and codecs.encode(report.reply) == raw
    with pytest.raises(ValueError, match="fresh"):
        tuning_live.run(q, receiver, output)  # ty: ignore[invalid-argument-type]
    receiver.close()
    output.close()


@pytest.mark.parametrize("accepted", [False, True])
def test_send_failure_is_uncertain_even_when_test_transport_knows_acceptance(
    accepted: bool,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    output = testing.Output(protocol="midi1")
    output.fail_next_send(accepted=accepted)
    monkeypatch.setattr(tuning_live, "monotonic_ns", lambda: 0)
    report = tuning_live.run(query(), receiver, output)  # ty: ignore[invalid-argument-type]
    assert (
        report.status == "failed" and report.uncertain_send and not report.request_sent
    )
    assert output.snapshot().accepted == int(accepted)
    assert output.snapshot().attempts == 1
    receiver.close()
    output.close()


@pytest.mark.parametrize("fault", ["loss", "malformed", "timeout", "cancel", "worker"])
def test_host_stops_without_export_on_input_faults_and_cancellation(
    fault: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    output = testing.Output(protocol="midi1")
    token = events.Cancellation()
    if fault == "loss":
        receiver.lose(1)
    elif fault == "worker":
        receiver.fail("failed input")
    elif fault == "malformed":
        receiver.inject(b"\xf0\x7e\x01\x08\x01\xf7", clock=events.Clock(identity="c"))
    elif fault == "cancel":
        token.cancel()
    ticks = iter([0, 0])
    monkeypatch.setattr(
        tuning_live,
        "monotonic_ns",
        lambda: next(ticks, 10 if fault == "timeout" else 0),
    )
    q = query()
    report = tuning_live.run(q, receiver, output, cancel=token)  # ty: ignore[invalid-argument-type]
    assert report.status == {"timeout": "timed_out", "cancel": "cancelled"}.get(
        fault, "failed"
    )
    assert not report.uncertain_send and report.reply is None
    assert report.request_sent == (fault in ("malformed", "timeout"))
    with pytest.raises(ValueError):
        q.write(BytesIO())
    receiver.close()
    output.close()


@pytest.mark.parametrize("fault", ["loss", "worker", "interrupt"])
def test_fault_at_final_receive_or_during_send_cannot_complete_a_query(
    fault: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import midi

    class Receiver:
        dropped_packets = 0
        failure: Exception | None = None

        def receive(self, *, timeout: float) -> events.Event:
            if fault == "loss":
                self.dropped_packets = 1
            else:
                self.failure = midi.WorkerError("failed during receive")
            return events.Event(stream="s", message=messages.SysEx(data=reply()[1:-1]))

    class Output:
        def send(self, message: messages.Message) -> None:
            if fault == "interrupt":
                raise KeyboardInterrupt

    monkeypatch.setattr(tuning_live, "monotonic_ns", lambda: 0)
    report = tuning_live.run(query(), Receiver(), Output())  # ty: ignore[invalid-argument-type]
    assert report.status == ("cancelled" if fault == "interrupt" else "failed")
    assert report.reply is None
    assert report.uncertain_send == (fault == "interrupt")
    assert report.request_sent == (fault != "interrupt")
