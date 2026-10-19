"""Complete discovery observations do not infer persistent physical identity."""

import pytest

from meedee import topology


def endpoint(name: str = "keys") -> topology.Endpoint:
    return topology.Endpoint(
        backend="test", protocol="midi1", direction="input", backend_id=1, name=name
    )


def test_baseline_and_duplicate_counts_are_explicit_and_immutable() -> None:
    tracker = topology.Tracker()
    assert tracker.snapshot().generation == 0 and tracker.snapshot().last is None
    p = endpoint()
    first = tracker.observe([p, endpoint()])
    assert first.generation == 1 and first.port_count == 2
    assert first.ports[0].count == 2 and first.ports[0].endpoint == p
    assert hash(endpoint()) == hash(p)
    assert first.last is not None and first.last.added == 0 and first.last.removed == 0
    second = tracker.observe([p])
    assert second.last is not None and second.last.removed == 1
    assert second.last.changes[0].count == 1
    assert first.port_count == 2
    first.ports.clear()
    assert first.ports[0].count == 2
    with pytest.raises(AttributeError):
        p.name = "other"  # ty: ignore[invalid-assignment]


def test_rename_is_removal_and_addition_even_with_reused_backend_id() -> None:
    tracker = topology.Tracker()
    tracker.observe([endpoint()])
    report = tracker.observe([endpoint("renamed")])
    assert report.last is not None
    assert report.last.added == report.last.removed == 1
    assert len(report.last.changes) == 2
    assert report.ports[0].endpoint.backend_id == 1
    same_name_other_direction = topology.Endpoint(
        backend="test", protocol="ump", direction="output", backend_id=1, name="renamed"
    )
    report = tracker.observe([same_name_other_direction])
    assert report.last is not None and report.last.added == report.last.removed == 1


def test_overflow_and_failure_preserve_the_last_complete_snapshot() -> None:
    tracker = topology.Tracker(port_limit=1)
    tracker.observe([endpoint()])
    with pytest.raises(ValueError, match="port limit"):
        tracker.observe([endpoint(), endpoint()])
    tracker.stop("later failure")
    report = tracker.snapshot()
    assert (
        report.stopped and report.reason is not None and "port limit" in report.reason
    )
    assert report.generation == 1 and report.port_count == 1
    assert report.last is not None and report.last.removed == 0
    with pytest.raises(ValueError, match="stopped"):
        tracker.observe([])
    failed = topology.Tracker()
    failed.stop("x" * 2000)
    assert len(failed.snapshot().reason or "") == 1024
    assert failed.snapshot().generation == 0 and failed.snapshot().last is None
