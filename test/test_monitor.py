"""Passive monitor logic with pure events and a controlled receive source."""

import json
from types import SimpleNamespace

import pytest

pytest.importorskip("tyro", reason="install meedee[cli] for monitor tests")
pytest.importorskip("pydantic", reason="install meedee[cli] for monitor tests")

from meedee import events, inspection, messages, midi
from meedee._cli import Monitor
from meedee._monitor import Session, select_port, watch


def event(message: messages.Message, ticks: int = 0) -> messages.Message:
    return (message).replace(
        stream="s",
        timestamp=events.Timestamp(ticks=ticks, clock=events.Clock(identity="c")),
    )


def test_filters_keep_interpretation_context_but_loss_resets_it() -> None:
    config = Monitor(port="keys", encoding="bytes", types=["NoteOn"], history_limit=2)
    session = Session(config, "s", events.Clock(identity="c"))
    monitor = session.monitor
    prefix = event(messages.ControlChange(channel=1, controller=88, value=13))
    note = event(messages.NoteOn(channel=1, note=60, velocity=100))
    assert monitor.feed(prefix) is None
    entry = monitor.feed(note)
    assert entry is not None and "12813" in str(entry.observations)
    session.present(entry)
    monitor.feed(prefix)
    decoder = events.PacketDecoder(stream="s", encoding=midi.Encoding.Bytes)
    entry = monitor.feed(decoder.loss(lost=2, reason="lost input"))
    assert entry is not None and entry.diagnostic is not None
    assert entry.kind == "diagnostic" and entry.diagnostic.lost == 2
    session.present(entry)
    entry = monitor.feed(note)
    assert entry is not None and "resolution=7" in str(entry.observations)
    session.present(entry)
    entry = monitor.counter(7)
    assert entry is not None and entry.transport_drop_total == 7
    assert monitor.counter(7) is None
    report = monitor.snapshot().inspection
    assert report.lost_events == 2 and report.gap_records == 1
    assert monitor.snapshot().filtered == 2 and len(session.history) == 2
    monitor.feed(note)
    assert report.records == 5
    assert monitor.snapshot().inspection.records == 6
    with pytest.raises(ValueError, match="one source"):
        monitor.feed(note.replace(stream="other"))
    assert monitor.snapshot().inspection.records == 6


def test_group_channel_filters_and_sysex_previews_are_bounded() -> None:
    config = Monitor(port="keys", encoding="ump", group=16, channel=16)
    session = Session(config, "s", events.Clock(identity="c"))
    message = messages.NoteOn(channel=16, note=60, velocity=100).replace(group=16)
    entry = session.monitor.feed(event(message))
    assert entry is not None and entry.message == message
    assert session.monitor.feed(event(message.replace(group=1))) is None
    config = Monitor(port="keys", encoding="bytes", preview_bytes=4)
    session = Session(config, "s", events.Clock(identity="c"))
    entry = session.monitor.feed(event(messages.SysEx(data=bytes(range(20)))))
    assert entry is not None
    session.present(entry)
    preview = session.history[-1]["preview"]
    assert "00010203" in str(preview) and "20 bytes" in str(preview)
    assert "04050607" not in str(preview)
    with pytest.raises(ValueError, match="group filter"):
        Monitor(port="keys", encoding="bytes", group=1)


def test_filtered_notes_update_state_and_loss_invalidates_it() -> None:
    session = inspection.Monitor(
        types=["NoteOff"],
        stream="s",
        clock=events.Clock(identity="c"),
    )
    assert session.feed(event(messages.NoteOn(channel=1, note=60, velocity=1))) is None
    entry = session.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))
    assert entry is not None and entry.note_state[0].notes[0].released == 1
    session.counter(1)
    channel = session.snapshot().inspection.note_state.channels[0]
    assert channel.uncertain and channel.notes == []


class Feed:
    def __init__(self, items: list[messages.Message]) -> None:
        self.stream = "s"
        self.clock = events.Clock(identity="c")
        self.dropped_packets = 0
        self.failure: Exception | None = None
        self.items = items
        self.closed = False
        self.elapsed = 0.0

    def __enter__(self) -> "Feed":
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True

    def receive(self, *, timeout: float) -> messages.Message:
        self.elapsed += 1
        if not self.items:
            raise KeyboardInterrupt
        return self.items.pop(0)


def test_monitor_duration_interrupt_and_terminal_failure_close_the_input(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    feed = Feed([event(messages.TimingClock())])
    monkeypatch.setattr(
        "meedee.inspection.monotonic_ns", lambda: int(feed.elapsed * 1_000_000_000)
    )
    config = Monitor(port="keys", encoding="bytes", duration=0.5, output="json")
    assert watch(feed, config) == 0  # ty: ignore[invalid-argument-type]
    assert feed.closed
    data = json.loads(capsys.readouterr().out.splitlines()[-1])
    assert data["records"] == 1 and data["live"] and "complete" not in data
    assert "metrics" not in data
    feed = Feed([])
    config = Monitor(port="keys", encoding="bytes", output="json")
    assert watch(feed, config) == 0  # ty: ignore[invalid-argument-type]
    assert feed.closed
    assert json.loads(capsys.readouterr().out.splitlines()[-1])["interrupted"]
    feed = Feed([])
    feed.failure = midi.WorkerError("worker exited")
    assert watch(feed, config) == 2  # ty: ignore[invalid-argument-type]
    assert feed.closed
    assert "worker exited" in str(
        json.loads(capsys.readouterr().out.splitlines()[-1])["failure"]
    )


def test_port_selection_requires_an_exact_unique_name() -> None:
    a = SimpleNamespace(name="keys")
    assert select_port([a], "keys") is a  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError, match="found 0"):
        select_port([a], "key")
    with pytest.raises(ValueError, match="found 2"):
        select_port([a, a], "keys")


def test_monitor_metrics_count_filtered_events_without_comparing_source_clocks(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    feed = Feed(
        [
            event(messages.TimingClock(), ticks=10_000_000_000),
            event(messages.TimingClock(), ticks=1),
        ]
    )
    monkeypatch.setattr(
        "meedee.inspection.monotonic_ns", lambda: int(feed.elapsed * 1_000_000_000)
    )
    config = Monitor(
        port="keys", encoding="bytes", metrics=True, types=["NoteOn"], output="json"
    )
    assert watch(feed, config) == 1  # ty: ignore[invalid-argument-type]
    data = json.loads(capsys.readouterr().out.splitlines()[-1])
    summary = data["metrics"]
    assert summary["clock"] == "host_monotonic" and summary["events"] == 2
    assert summary["elapsed_ns"] == 3_000_000_000
    assert summary["events_per_second"] == pytest.approx(2 / 3)
    assert summary["delivery_intervals"]["count"] == 1
    assert summary["delivery_intervals"]["sum_ns"] == 1_000_000_000
    assert data["filtered"] == 2 and data["timestamp_regressions"] == 1
