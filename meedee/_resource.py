"""Configuration models for identity-bearing live resources."""

from collections.abc import Iterator
from typing import Never, Self

from pydantic import BaseModel, ConfigDict


class ResourceModel(BaseModel, frozen=True):
    # Resources use iteration for I/O, not BaseModel's field-pair iteration.
    def __iter__(self) -> Iterator[object]:  # ty: ignore[invalid-method-override]
        raise TypeError("Use model_dump() to inspect resource settings")

    def __eq__(self, other: object) -> bool:
        return self is other

    def __hash__(self) -> int:
        return id(self)

    def __copy__(self) -> Self:
        raise TypeError("Resource objects cannot be copied; construct a new one")

    def __deepcopy__(self, memo: dict[int, object] | None = None) -> Self:
        raise TypeError("Resource objects cannot be copied; construct a new one")

    def __getstate__(self) -> Never:
        raise TypeError(
            "Resource objects cannot be pickled; serialize settings instead"
        )

    model_config = ConfigDict(
        arbitrary_types_allowed=True,
        extra="forbid",
        strict=True,
        validate_default=True,
    )
