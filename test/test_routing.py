"""Pure routing and archive preview keep provenance and delivery uncertainty."""

from io import BytesIO
from typing import Literal

import pytest

from meedee import archive, events, messages, midi, routing, testing, ump


def route(
    *, protocol: Literal["midi1", "ump"] = "midi1", include_nonchannel: bool = False
) -> routing.Route:
    return routing.Route(
        name="r",
        source="s",
        destination="out",
        protocol=protocol,
        remap_channel=16,
        include_nonchannel=include_nonchannel,
    )


def event(message: messages.Message) -> messages.Message:
    return (message).replace(
        stream="s",
        timestamp=events.Timestamp(ticks=-2, clock=events.Clock(identity="c")),
    )


def test_midi1_remap_retains_source_span_and_immutable_snapshot() -> None:
    router = routing.Router(routes=[route()])
    original = event(messages.ProgramChange(channel=1, program=128))
    original = original.replace(
        span=events.SourceSpan(
            first_packet=1,
            last_packet=3,
            first_timestamp=original.timestamp,
            last_timestamp=original.timestamp,
        )
    )
    router.feed(original)
    delivery = router.peek()
    assert delivery is not None
    assert delivery.message == messages.ProgramChange(channel=16, program=128)
    assert delivery.message.stream == original.stream
    assert delivery.message.timestamp == original.timestamp
    assert delivery.message.span == original.span
    assert original == messages.ProgramChange(channel=1, program=128)
    before = router.snapshot()
    with pytest.raises(ValueError, match="queue head"):
        router.confirm(delivery.id + 1)
    router.confirm(delivery.id)
    assert before.confirmed == 0 and router.snapshot().confirmed == 1
    before.routes.clear()
    detail = before.routes["r"]
    assert isinstance(detail, routing.RouteReport)
    assert detail.admitted == 1 and detail.confirmed == 0
    assert detail.capacity_rejections == 0
    assert router.snapshot().routes["r"].confirmed == 1
    with pytest.raises(AttributeError):
        detail.confirmed = 2  # ty: ignore[invalid-assignment]
    with pytest.raises(AttributeError):
        delivery.destination = "elsewhere"  # ty: ignore[invalid-assignment]


def test_nonchannel_opt_in_and_group_selection_are_explicit() -> None:
    router = routing.Router(routes=[route()])
    router.feed(event(messages.SystemReset()))
    assert router.peek() is None and router.snapshot().filtered == 1
    opted = routing.Router(routes=[route(include_nonchannel=True)])
    reset = event(messages.SystemReset())
    opted.feed(reset)
    delivery = opted.peek()
    assert delivery is not None and delivery.message == reset
    selected = routing.Router(
        routes=[
            routing.Route(
                name="ump",
                source="s",
                destination="out",
                protocol="ump",
                group=2,
                remap_group=16,
                channel=1,
            )
        ]
    )
    selected.feed(
        event(
            ump.Midi1Packet(
                group=1, message=messages.NoteOn(channel=1, note=60, velocity=1)
            )
        )
    )
    assert selected.peek() is None
    selected.feed(
        event(
            ump.Midi1Packet(
                group=2, message=messages.NoteOn(channel=1, note=60, velocity=0)
            )
        )
    )
    delivery = selected.peek()
    assert delivery is not None
    assert delivery.message == ump.Midi1Packet(
        group=16, message=messages.NoteOn(channel=1, note=60, velocity=0)
    )


def test_route_report_retains_counts_after_capacity_failure() -> None:
    router = routing.Router(routes=[route()], event_limit=1)
    router.feed(event(messages.NoteOn(60, channel=1)))
    before = router.snapshot().routes["r"]
    with pytest.raises(ValueError):
        router.feed(event(messages.NoteOn(61, channel=1)))
    after = router.snapshot().routes["r"]
    assert before.admitted == after.admitted == 1
    assert before.confirmed == after.confirmed == 0
    assert before.capacity_rejections == 0 and after.capacity_rejections == 1


def test_failure_preserves_known_prefix_and_uncertain_delivery() -> None:
    router = routing.Router(routes=[route()])
    router.feed(event(messages.NoteOn(channel=1, note=60, velocity=1)))
    router.feed(event(messages.NoteOff(channel=1, note=60, velocity=0)))
    first = router.peek()
    assert first is not None
    router.confirm(first.id)
    second = router.peek()
    assert second is not None
    router.fail(second.id, "send may have taken effect")
    router.stop("later close failure")
    report = router.snapshot()
    assert report.stopped and report.confirmed == 1 and report.pending == 1
    assert report.uncertain_delivery == second.id
    assert report.reason == "send may have taken effect"
    assert router.peek() is None
    with pytest.raises(ValueError, match="stopped"):
        router.feed(event(messages.TimingClock()))


def test_file_only_packets_and_commands_are_not_implicitly_sent() -> None:
    router = routing.Router(routes=[route(protocol="ump", include_nonchannel=True)])
    with pytest.raises(ValueError, match="file-only"):
        router.feed(event(ump.DeltaClockstamp(ticks=1)))
    router = routing.Router(routes=[route()])
    with pytest.raises(ValueError, match="expanded"):
        router.feed(
            event(messages.RpnSet(channel=1, parameter=0, value=2, resolution=7))
        )


def capture(*, gap: bool = False) -> bytes:
    data = BytesIO()
    writer = archive.Writer(data)
    writer.write(events.Clock(identity="c"))
    writer.write(archive.Stream(id="s", label="", clock_identity="c"))
    for n in range(3):
        writer.write(event(messages.NoteOn(channel=1, note=60 + n, velocity=1)))
        if n == 0 and gap:
            writer.write(archive.Gap(stream="s", lost=1, reason="loss"))
    writer.finish()
    return data.getvalue()


def test_preview_bounds_examples_and_never_calls_prefix_complete() -> None:
    report = routing.preview_file(BytesIO(capture()), routes=[route()], detail_limit=1)
    assert report.complete and report.problem is None
    assert report.routing.confirmed == 3 and len(report.examples) == 1
    assert report.omitted_examples == 2
    report = routing.preview_file(BytesIO(capture()), routes=[route()], byte_limit=3)
    assert len(report.examples) == 1 and report.omitted_examples == 2
    report = routing.preview_file(BytesIO(capture(gap=True)), routes=[route()])
    assert report.complete and report.routing.stopped and report.routing.confirmed == 1
    report = routing.preview_file(BytesIO(capture()[:-1]), routes=[route()])
    assert not report.complete and report.problem
    with pytest.raises(ValueError, match="absent"):
        routing.preview_file(
            BytesIO(capture()),
            routes=[
                routing.Route(
                    name="r", source="missing", destination="out", protocol="midi1"
                )
            ],
        )


def test_route_configuration_rejects_ambiguous_identities() -> None:
    with pytest.raises(ValueError, match="unique"):
        routing.Router(routes=[route(), route()])
    with pytest.raises(ValueError, match="protocol"):
        routing.Route(name="r", source="s", destination="o", protocol="midi1", group=1)


def test_live_driver_preserves_uncertain_send_and_leaves_connections_open() -> None:
    source = testing.Input(stream="s", protocol="midi1")
    output = testing.Output(protocol="midi1")
    source.inject(b"\x90\x3c\x5a", clock=events.Clock(identity="c"))
    output.fail_next_send(accepted=True)
    router = routing.Router(routes=[route()])
    with pytest.raises(midi.SendError, match="simulated send failure"):
        routing.run(router, source, {"out": output})  # ty: ignore[invalid-argument-type]
    report = router.snapshot()
    assert report.stopped and report.confirmed == 0
    assert report.uncertain_delivery == 0 and report.pending == 1
    assert output.take_packet() == b"\x9f\x3c\x5a"
    assert not source.closed and not output.closed
