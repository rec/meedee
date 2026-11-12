"""Structured compound-send progress, preserved across the private worker pipe."""

from typing import cast

from .midi import SendError


class CompoundSendError(SendError):
    def __init__(
        self, message: str, completed_messages: int | None, total_messages: int
    ) -> None:
        super().__init__(message, completed_messages, total_messages)

    @property
    def completed_messages(self) -> int | None:
        return cast(int | None, self.args[1])

    @property
    def total_messages(self) -> int:
        return cast(int, self.args[2])

    def __str__(self) -> str:
        return str(self.args[0])
