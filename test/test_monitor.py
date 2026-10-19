"""Passive monitor logic with pure events and a controlled receive source."""

import json
from types import SimpleNamespace

import pytest

pytest.importorskip("tyro", reason="install meedee[cli] for monitor tests")
pytest.importorskip("pydantic", reason="install meedee[cli] for monitor tests")

from meedee import events, messages, midi, ump
from meedee._cli import Monitor
from meedee._monitor import Session, select_port, watch


def event(message: messages.Message, ticks: int = 0) -> events.Event:
    return events.Event(
        stream="s",
        message=message,
        timestamp=events.Timestamp(ticks=ticks, clock=events.Clock(identity="c")),
    )


def test_filters_keep_interpretation_context_but_loss_resets_it() -> None:
    config = Monitor(port="keys", protocol="midi1", types=["NoteOn"], history_limit=2)
    session = Session(config, "s", events.Clock(identity="c"))
    prefix = event(messages.ControlChange(channel=1, controller=88, value=13))
    note = event(messages.NoteOn(channel=1, note=60, velocity=100))
    assert session.feed(prefix) == []
    row = session.feed(note)[0]
    assert "12813" in str(row["observations"])
    session.feed(prefix)
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    row = session.feed(decoder.loss(lost=2, reason="lost input"))[0]
    assert row["kind"] == "diagnostic" and row["lost"] == 2
    row = session.feed(note)[0]
    assert "resolution=7" in str(row["observations"])
    assert session.counter(7)[0]["transport_drop_total"] == 7
    assert session.counter(7) == []
    report = session.observer.snapshot()
    assert report.lost_events == 2 and report.gap_records == 1
    assert session.filtered == 2 and len(session.history) == 2
    session.feed(note)
    assert report.records == 5
    assert session.observer.snapshot().records == 6
    with pytest.raises(ValueError, match="one source"):
        session.observer.feed(note.replace(stream="other"))
    assert session.observer.snapshot().records == 6


def test_group_channel_filters_and_sysex_previews_are_bounded() -> None:
    config = Monitor(port="keys", protocol="ump", group=16, channel=16)
    session = Session(config, "s", events.Clock(identity="c"))
    message = ump.Midi1Packet(
        group=16, message=messages.NoteOn(channel=16, note=60, velocity=100)
    )
    assert session.feed(event(message))[0]["channel"] == 16
    assert session.feed(event(message.replace(group=1))) == []
    config = Monitor(port="keys", protocol="midi1", preview_bytes=4)
    session = Session(config, "s", events.Clock(identity="c"))
    preview = session.feed(event(messages.SysEx(data=bytes(range(20)))))[0]["preview"]
    assert "00010203" in str(preview) and "20 bytes" in str(preview)
    assert "04050607" not in str(preview)
    with pytest.raises(ValueError, match="group filter"):
        Monitor(port="keys", protocol="midi1", group=1)


def test_filtered_notes_update_state_and_loss_invalidates_it() -> None:
    session = Session(
        Monitor(port="keys", protocol="midi1", types=["NoteOff"]),
        "s",
        events.Clock(identity="c"),
    )
    assert session.feed(event(messages.NoteOn(channel=1, note=60, velocity=1))) == []
    row = session.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))[0]
    state = row["note_state"]
    assert isinstance(state, list) and state[0]["notes"][0]["released"] == 1
    session.counter(1)
    channel = session.observer.snapshot().note_state.channels[0]
    assert channel.uncertain and channel.notes == []


class Feed:
    def __init__(self, items: list[events.Event]) -> None:
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

    def receive(self, *, timeout: float) -> events.Event:
        self.elapsed += 1
        if not self.items:
            raise KeyboardInterrupt
        return self.items.pop(0)


def test_monitor_duration_interrupt_and_terminal_failure_close_the_input(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    feed = Feed([event(messages.TimingClock())])
    monkeypatch.setattr("meedee._monitor.monotonic", lambda: feed.elapsed)
    config = Monitor(port="keys", protocol="midi1", duration=0.5, output="json")
    assert watch(feed, config) == 0  # ty: ignore[invalid-argument-type]
    assert feed.closed
    data = json.loads(capsys.readouterr().out.splitlines()[-1])
    assert data["records"] == 1 and data["live"] and "complete" not in data
    feed = Feed([])
    config = Monitor(port="keys", protocol="midi1", output="json")
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
