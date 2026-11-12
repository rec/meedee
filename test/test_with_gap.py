"""Gap insertion is lazy, generic and independent of MIDI transport."""

from collections.abc import Iterator

import pytest

from meedee.events import with_gap


def test_each_item_is_followed_by_gap_including_the_last() -> None:
    assert list(with_gap(iter(["a", "b"]), 0.1)) == ["a", 0.1, "b", 0.1]
    item = object()
    values = list(with_gap(iter([item]), 0.0))
    assert values[0] is item and values[1] == 0.0
    assert list(with_gap(iter([]), 1.0)) == []


def test_source_is_consumed_only_when_the_next_item_is_requested() -> None:
    consumed: list[int] = []

    def source() -> Iterator[int]:
        for i in range(2):
            consumed.append(i)
            yield i

    values = with_gap(source(), 0.1)
    assert consumed == []
    assert next(values) == 0 and consumed == [0]
    assert next(values) == 0.1 and consumed == [0]
    assert next(values) == 1 and consumed == [0, 1]


def test_source_failure_propagates_after_the_preceding_gap() -> None:
    def source() -> Iterator[str]:
        yield "a"
        raise ValueError("source failed")

    values = with_gap(source(), 0.1)
    assert next(values) == "a"
    assert next(values) == 0.1
    with pytest.raises(ValueError, match="source failed"):
        next(values)
