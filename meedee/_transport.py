"""Python-facing receive admission. Subprocess transport lives in the Rust toolkit."""

from __future__ import annotations

from collections.abc import Callable
from queue import Full, Queue
from typing import TypeVar

DEFAULT_RECEIVE_BYTE_LIMIT = 8 * 1024 * 1024
RECEIVE_CAPACITY = 128
T = TypeVar("T")


class ReceiveQueue(Queue[T]):
    """Bound queued message payloads under the queue's existing mutex."""

    def __init__(
        self,
        byte_limit: int,
        *,
        capacity: int | None = None,
        size_of: Callable[[T], int],
    ) -> None:
        super().__init__(RECEIVE_CAPACITY if capacity is None else capacity)
        self.size_of = size_of
        self.byte_limit = byte_limit
        self.payload_bytes = 0

    def _put(self, item: T) -> None:
        size = self.size_of(item)
        if self.payload_bytes + size > self.byte_limit:
            raise Full
        super()._put(item)
        self.payload_bytes += size

    def _get(self) -> T:
        item = super()._get()
        self.payload_bytes -= self.size_of(item)
        return item
