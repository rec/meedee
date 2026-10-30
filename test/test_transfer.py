"""Shared transfer contracts and controlled caller-thread output delivery."""

from io import BytesIO
from typing import cast

import pytest

from meedee import codecs, events, messages, midi, transfer, transfer_output


def plan(gap_ns: int = 10) -> transfer.Plan:
    return transfer.read_file(
        BytesIO(bytes.fromhex("f0 01 f7 f0 02 03 f7")), gap_ns=gap_ns
    )


def test_plan_is_immutable_and_keeps_complete_wire_messages() -> None:
    source = BytesIO(bytes.fromhex("f0 01 f7 f0 02 03 f7"))
    p = transfer.read_file(
        source, gap_ns=10, message_limit=2, byte_limit=7, payload_limit=2
    )
    assert not source.closed
    assert p.message_count == 2 and p.wire_bytes == 7 and p.gap_ns == 10
    assert codecs.encode(p.message(1)) == bytes.fromhex("f0 02 03 f7")
    with pytest.raises(AttributeError):
        p.gap_ns = 0  # ty: ignore[invalid-assignment]
    with pytest.raises(IndexError):
        p.message(2)
    m = [messages.SysEx(data=b"\x7d")]
    p = transfer.Plan(m, gap_ns=0)
    m.clear()
    assert p.message_count == 1 and p.message(0).data == b"\x7d"
    with pytest.raises(ValueError):
        transfer.Plan([], gap_ns=0)
    with pytest.raises(TypeError):
        transfer.Plan([messages.SysEx(data=b"")], gap_ns=True)


@pytest.mark.parametrize(
    ("data", "options"),
    [
        ("f0 01", {}),
        ("f0 01 f7 f8", {}),
        ("", {}),
        ("f0 01 f7 f0 02 f7", {"message_limit": 1}),
        ("f0 01 02 f7", {"byte_limit": 3}),
        ("f0 01 02 f7", {"payload_limit": 1}),
    ],
)
def test_invalid_or_oversized_files_never_yield_a_partial_plan(
    data: str, options: dict[str, int]
) -> None:
    with pytest.raises(OSError):
        transfer.read_file(BytesIO(bytes.fromhex(data)), gap_ns=0, **options)


def test_session_pacing_and_uncertain_handoff_preserve_exact_progress() -> None:
    s = transfer.Session(plan())
    step = s.prepare(0)
    assert step is not None and step.index == 0 and step.message.data == b"\x01"
    with pytest.raises(ValueError, match="in flight"):
        s.prepare(1)
    s.confirm(0, 100)
    saved = s.snapshot()
    assert s.prepare(109) is None and saved.next_ready_ns == 110
    with pytest.raises(ValueError, match="regressed"):
        s.prepare(108)
    assert (next_step := s.prepare(110)) is not None and next_step.index == 1
    s.fail(1, "unknown delivery")
    r = s.snapshot()
    assert r.status == "failed" and r.confirmed_messages == 1 and r.confirmed_bytes == 3
    assert (
        r.remaining_messages == 1 and r.uncertain_message == 1 and r.in_flight is None
    )
    assert saved.status == "active" and saved.uncertain_message is None
    with pytest.raises(ValueError, match="terminal"):
        s.prepare(111)


class Output:
    def __init__(self, clock: list[int], error: BaseException | None = None) -> None:
        self.clock = clock
        self.error = error
        self.sent: list[tuple[int, bytes]] = []

    def send(self, message: messages.SysEx) -> None:
        self.sent.append((self.clock[0], codecs.encode(message)))
        self.clock[0] += 100
        if self.error is not None and len(self.sent) == 2:
            raise self.error


def test_output_waits_from_send_completion_and_never_retries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = [0]
    monkeypatch.setattr(transfer_output, "monotonic_ns", lambda: clock[0])

    def advance(seconds: float) -> None:
        clock[0] += round(seconds * 1_000_000_000)

    monkeypatch.setattr(transfer_output, "sleep", advance)
    output = Output(clock, midi.SendError("failed"))
    s = transfer.Session(plan())
    r = transfer_output.run(s, cast(midi.OutputConnection, output))
    assert output.sent == [
        (0, bytes.fromhex("f0 01 f7")),
        (110, bytes.fromhex("f0 02 03 f7")),
    ]
    assert (
        r.status == "failed" and r.confirmed_messages == 1 and r.uncertain_message == 1
    )
    assert (
        transfer_output.run(s, cast(midi.OutputConnection, output)).status == "failed"
    )
    assert len(output.sent) == 2


def test_cancellation_before_or_during_send_keeps_uncertainty_visible(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    token = events.Cancellation()
    token.cancel()
    output = Output([0])
    r = transfer_output.run(
        transfer.Session(plan()), cast(midi.OutputConnection, output), cancel=token
    )
    assert r.status == "cancelled" and r.uncertain_message is None and output.sent == []
    clock = [0]
    monkeypatch.setattr(transfer_output, "monotonic_ns", lambda: clock[0])

    def advance(seconds: float) -> None:
        clock[0] += round(seconds * 1_000_000_000)

    monkeypatch.setattr(transfer_output, "sleep", advance)
    output = Output(clock, KeyboardInterrupt())
    r = transfer_output.run(
        transfer.Session(plan()), cast(midi.OutputConnection, output)
    )
    assert (
        r.status == "cancelled"
        and r.confirmed_messages == 1
        and r.uncertain_message == 1
    )


def test_cancellation_during_gap_does_not_reserve_the_next_message(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    token = events.Cancellation()
    clock = [0]
    monkeypatch.setattr(transfer_output, "monotonic_ns", lambda: clock[0])
    waits: list[float] = []

    def cancel_wait(seconds: float) -> None:
        waits.append(seconds)
        token.cancel()

    monkeypatch.setattr(transfer_output, "sleep", cancel_wait)
    output = Output(clock)
    r = transfer_output.run(
        transfer.Session(plan(1_000_000_000)),
        cast(midi.OutputConnection, output),
        cancel=token,
    )
    assert waits == [0.05] and len(output.sent) == 1
    assert (
        r.status == "cancelled"
        and r.confirmed_messages == 1
        and r.uncertain_message is None
    )


def test_unexpected_sender_failure_propagates_with_inspectable_uncertainty(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    output = Output([0], LookupError("caller bug"))
    s = transfer.Session(plan(0))
    with pytest.raises(LookupError, match="caller bug"):
        transfer_output.run(s, cast(midi.OutputConnection, output))
    assert s.snapshot().status == "failed" and s.snapshot().uncertain_message == 1
    assert s.snapshot().confirmed_messages == 1
