"""Paced SysEx uses ordinary output with confirmed progress on failure."""

from typing import cast

import pytest

from meedee import events, messages, midi, sysex, testing


def test_pacing_starts_after_successful_send_and_never_waits_after_last(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    time = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: time.now_ns)
    monkeypatch.setattr(
        events.Cancellation,
        "wait_for",
        lambda self, seconds: time.advance(round(seconds * 1_000_000_000)),
    )
    sent: list[int] = []

    class Output:
        def send(self, message: messages.SysEx) -> None:
            sent.append(time.now_ns)
            time.advance(100)

    report = sysex.send(
        cast(midi.Output, Output()), [messages.SysEx(data=b"\x01")] * 3, gap="10 ns"
    )
    assert sent == [0, 110, 220]
    assert time.now_ns == 320 and report.sent_events == 3


def test_failure_retains_typed_exception_and_confirmed_prefix() -> None:
    output = testing.Output(protocol="midi1")
    output.fail_next_send(accepted=True)
    message = messages.SysEx(data=b"\x7d")
    with pytest.raises(midi.SendError) as failure:
        sysex.send(output, [message, message], gap=0)  # ty: ignore[invalid-argument-type]
    assert failure.value.completed_events == 0  # ty: ignore[unresolved-attribute]
    assert failure.value.report.sent_events == 0  # ty: ignore[unresolved-attribute]
    assert output.snapshot().accepted == 1
    output.close()


def test_invalid_source_keeps_confirmed_progress_without_closing_output() -> None:
    output = testing.Output(protocol="midi1")
    with pytest.raises(TypeError) as failure:
        sysex.send(output, [messages.SysEx(data=b"\x01"), messages.NoteOn(60)], gap=0)  # ty: ignore[invalid-argument-type]
    assert failure.value.completed_events == 1  # ty: ignore[unresolved-attribute]
    assert not output.closed
    output.close()


def test_empty_source_does_not_open_a_device() -> None:
    output = midi.Output()
    assert sysex.send(output, [], gap=0).sent_events == 0
    output.close()
