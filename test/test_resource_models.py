"""Configuration models preserve identity and never copy resource ownership."""

import copy
import pickle
from collections.abc import Callable

import pytest
from pydantic import ValidationError

from middi import discovery, midi
from middi._resource import ResourceModel


@pytest.mark.parametrize(
    "create",
    [midi.Input, midi.Output, lambda: discovery.Poller(encoding="bytes")],
)
def test_settings_roundtrip_creates_a_distinct_resource_without_runtime_state(
    create: Callable[[], ResourceModel],
) -> None:
    resource = create()
    settings = resource.model_dump()
    assert not any(k.startswith("_") for k in settings)
    other = type(resource)(**settings)
    assert resource != other
    assert len({resource, other}) == 2
    with pytest.raises(ValidationError, match="frozen"):
        resource.encoding = settings["encoding"]  # ty: ignore[invalid-assignment]
    for operation in (copy.copy, copy.deepcopy, pickle.dumps):
        with pytest.raises(TypeError, match="Resource objects cannot"):
            operation(resource)
    with pytest.raises(TypeError, match="Resource objects cannot"):
        resource.model_copy()


def test_unit_values_and_iterable_channels_become_validated_fields() -> None:
    source = [1, 3]
    resource = midi.Input(
        channel=iter(source), timeout="250 ms", receive_byte_limit="1 KiB"
    )
    source.append(5)
    assert resource.channel == [1, 3]
    assert resource.timeout == 0.25 and resource.receive_byte_limit == 1024
    assert resource.model_dump()["channel"] == [1, 3]
    output = midi.Output(bpm=2**53 + 1)
    assert output.bpm == 2**53 + 1


@pytest.mark.parametrize(
    "create,field",
    [
        (lambda: midi.Input(raw="yes"), "raw"),  # ty: ignore[no-matching-overload]
        (lambda: midi.Input(channel=[True]), "channel"),
        (lambda: midi.Input(event_limit="128"), "event_limit"),  # ty: ignore[invalid-argument-type]
        (lambda: midi.Output(channel=True), "channel"),
        (lambda: midi.Output(bpm="120"), "bpm"),  # ty: ignore[invalid-argument-type]
        (lambda: discovery.Poller(encoding="bytes", port_limit=True), "port_limit"),
    ],
)
def test_invalid_settings_report_the_field_without_coercing_values(
    create: Callable[[], ResourceModel], field: str
) -> None:
    with pytest.raises(ValidationError) as error:
        create()
    assert error.value.errors()[0]["loc"][0] == field
