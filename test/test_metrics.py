"""Exact bounded statistics and opt-in record/playback integration."""

from pathlib import Path

import pytest

from meedee import archive, events, messages, metrics, midi


def event() -> events.Event:
    return events.Event(
        stream="s",
        message=messages.TimingClock(),
        timestamp=events.Timestamp(ticks=0, clock=events.Clock(identity="source")),
    )


def test_histograms_keep_exact_large_totals_and_independent_frozen_snapshots() -> None:
    h = metrics.Histogram()
    assert h.snapshot().count == 0 and h.snapshot().minimum_ns is None
    for n in (0, 1, 1000, 1001, 1_000_000_000, 1_000_000_001, 2**64 - 1, 2**64 - 1):
        h.observe(n)
    saved = h.snapshot()
    assert saved.count == 8 and saved.sum_ns == 2 * (2**64 - 1) + 2_000_002_003
    assert saved.minimum_ns == 0 and saved.maximum_ns == 2**64 - 1
    assert saved.buckets == [1, 2, 1, 0, 0, 0, 0, 1, 3]
    saved.buckets.clear()
    saved.upper_bounds_ns.clear()
    assert len(h.snapshot().buckets) == 9 and len(h.snapshot().upper_bounds_ns) == 8
    with pytest.raises(AttributeError):
        saved.count = 0  # ty: ignore[invalid-assignment]
    with pytest.raises(ValueError):
        h.observe(2**64)
    with pytest.raises(TypeError):
        h.observe(True)
    h.observe(5)
    assert saved.count == 8 and h.snapshot().count == 9


def test_delivery_uses_host_window_time_and_rejects_regressing_observations() -> None:
    d = metrics.Delivery(start_ns=10)
    assert d.snapshot(10).events == 0
    with pytest.raises(ValueError):
        d.observe(9)
    d.observe(20)
    d.observe(20)
    d.observe(35)
    report = d.snapshot(50)
    assert report.events == 3 and report.elapsed_ns == 40
    assert report.intervals.count == 2 and report.intervals.sum_ns == 15
    with pytest.raises(ValueError):
        d.snapshot(34)
    with pytest.raises(ValueError):
        d.observe(34)
    d.observe(40)
    assert report.events == 3 and d.snapshot(50).events == 4


class Sink:
    def __init__(self, fail: bool = False) -> None:
        self.sent = 0
        self.fail = fail

    def send(self, message: messages.Message) -> None:
        if self.fail and self.sent == 1:
            raise midi.SendError("simulated send failure")
        self.sent += 1


def test_playback_metrics_are_opt_in_and_cover_only_successful_handoffs() -> None:
    from meedee import _native

    origin = events.Timestamp(ticks=0, clock=events.Clock(identity="source"))
    report = _native.play(Sink(), [event()], origin=origin)
    assert report.lateness is None
    sink = Sink()
    report = _native.play(sink, [event(), event()], origin=origin, metrics=True)
    assert report.lateness is not None and report.lateness.count == 2
    assert report.lateness.maximum_ns == report.max_lateness_ns
    assert sum(report.lateness.buckets) == report.sent_events
    with pytest.raises(events.PlaybackError) as result:
        _native.play(Sink(fail=True), [event(), event()], origin=origin, metrics=True)
    retained = result.value.report
    assert retained.sent_events == 1 and retained.lateness is not None
    assert retained.lateness.count == 1
    cancel = events.Cancellation()
    cancel.cancel()
    report = _native.play(Sink(), [event()], origin=origin, cancel=cancel, metrics=True)
    assert (
        report.cancelled and report.lateness is not None and report.lateness.count == 0
    )


def test_recording_metrics_are_opt_in_and_retain_capacity_rejections(
    tmp_path: Path,
) -> None:
    off = archive.Recorder(
        tmp_path / "off.meedee", clock=events.Clock(identity="source"), stream="s"
    )
    assert off.pressure is None and off.stop().pressure is None
    on = archive.Recorder(
        tmp_path / "on.meedee",
        clock=events.Clock(identity="source"),
        stream="s",
        metrics=True,
    )
    assert on.record(event())
    saved = on.pressure
    assert (
        saved is not None
        and saved.accepted_records == 1
        and saved.peak_reserved_records == 1
    )
    assert saved.peak_reserved_bytes > 0
    report = on.stop()
    assert report.pressure is not None and report.pressure.accepted_records == 1
    assert report.pressure.capacity_rejections == 0
    rejected = archive.Recorder(
        tmp_path / "rejected.meedee",
        clock=events.Clock(identity="source"),
        stream="s",
        byte_limit=1,
        metrics=True,
    )
    assert not rejected.record(event())
    report = rejected.stop()
    assert not report.complete and report.pressure is not None
    assert (
        report.pressure.accepted_records == 0
        and report.pressure.capacity_rejections == 1
    )
    assert report.pressure.peak_reserved_records == 0
