"""Public deterministic transports and selected toolkit conformance workflows."""

from typing import cast

import pytest

from meedee import (
    ci,
    codecs,
    events,
    messages,
    midi,
    routing,
    testing,
    transfer,
    transfer_output,
    ump,
)


def clock() -> events.Clock:
    return events.Clock(identity="simulation", origin="manual")


def test_fragments_reuse_running_status_and_preserve_signed_timestamp_spans() -> None:
    i = testing.Input(stream="s", protocol="midi1")
    assert i.inject(b"\x90\x3c", clock=clock(), timestamp_ns=-10)
    assert i.receive(timeout=100) is None
    assert i.inject(b"\x64\x3d\x65", clock=clock(), timestamp_ns=20)
    first = i.receive()
    second = i.receive()
    assert isinstance(first, events.Event) and isinstance(second, events.Event)
    assert first.message == messages.NoteOn(channel=1, note=60, velocity=100)
    assert second.message == messages.NoteOn(channel=1, note=61, velocity=101)
    assert first.timestamp is not None and first.timestamp.ticks == 20
    assert (
        first.span is not None
        and first.span.first_packet == 0
        and first.span.last_packet == 1
    )
    assert (
        first.span.first_timestamp is not None
        and first.span.first_timestamp.ticks == -10
    )
    assert i.receive() is None and i.snapshot().queued_bytes == 0


def test_raw_overflow_keeps_older_packets_and_reports_loss_even_after_silence() -> None:
    i = testing.Input(stream="s", protocol="midi1", packet_limit=1)
    i.inject(b"\x90\x3c", clock=clock())
    assert not i.inject(b"\x64", clock=clock())
    saved = i.snapshot()
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.lost == 1
    assert i.dropped_packets == 1 and saved.next_sequence == 2
    assert i.inject(b"\x90\x3d\x64", clock=clock())
    event = i.receive()
    assert isinstance(event, events.Event) and event.span is not None
    assert event.span.first_packet == 2 and i.receive() is None
    assert saved.queued_packets == 1 and i.snapshot().queued_packets == 0
    with pytest.raises(AttributeError):
        saved.dropped_packets = 0  # ty: ignore[invalid-assignment]


def test_decoded_overflow_and_explicit_loss_reset_interpretation() -> None:
    i = testing.Input(stream="s", protocol="midi1", event_limit=1)
    i.inject(b"\x90\x3c\x64\x3d\x65", clock=clock())
    assert isinstance(i.receive(), events.Event)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.kind == "loss"
    assert i.snapshot().dropped_events == 1 and i.dropped_packets == 0
    i.lose(2)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.lost == 2
    assert i.receive() is None


def test_ump_values_keep_native_width_and_protocol_mismatches_are_rejected() -> None:
    message = ump.Midi2ControlChange(group=4, channel=2, controller=7, value=0xFEDCBA98)
    o = testing.Output(protocol="ump")
    o.send_message(message)
    packet = o.take_packet()
    assert isinstance(packet, midi.UmpPacket)
    i = testing.Input(stream="s", protocol="ump")
    i.inject(packet, clock=clock())
    result = i.receive()
    assert isinstance(result, events.Event) and result.message == message
    with pytest.raises(ValueError):
        i.inject(b"\xf8", clock=clock())
    with pytest.raises(midi.CapabilityError):
        o.send_message(messages.TimingClock())


def test_worker_failure_is_visible_and_close_retires_input_without_devices() -> None:
    i = testing.Input(stream="s", protocol="midi1")
    i.inject(b"\xf8", clock=clock())
    i.fail("simulated disconnect")
    assert isinstance(i.failure, midi.WorkerError)
    assert isinstance(i.receive(), events.Event)
    with pytest.raises(midi.WorkerError, match="disconnect"):
        i.receive()
    with pytest.raises(midi.WorkerError):
        i.inject(b"\xf8", clock=clock())
    i.close()
    assert i.closed and i.snapshot().queued_bytes == 0
    with pytest.raises(midi.StateError):
        i.receive()


@pytest.mark.parametrize("accepted", [False, True])
def test_send_failure_records_explicit_sink_effect_without_claiming_a_returned_handoff(
    accepted: bool,
) -> None:
    o = testing.Output(protocol="midi1")
    o.fail_next_send(accepted=accepted, reason="uncertain")
    with pytest.raises(midi.SendError, match="uncertain"):
        o.send_message(messages.TimingClock())
    report = o.snapshot()
    assert report.attempts == 1 and report.accepted == int(accepted)
    o.close()
    assert o.take_packet() == (b"\xf8" if accepted else None)
    assert o.snapshot().accepted == int(accepted)
    with pytest.raises(midi.StateError):
        o.send(b"\xf8")


def test_output_capacity_never_discards_accepted_evidence() -> None:
    o = testing.Output(protocol="midi1", packet_limit=1, byte_limit=3)
    o.send(b"\x90\x3c\x64")
    with pytest.raises(midi.ResourceError):
        o.send(b"\x80\x3c\x00")
    assert o.snapshot().attempts == 2 and o.snapshot().accepted == 1
    assert o.take_packet() == b"\x90\x3c\x64"
    o.send(b"\x80\x3c\x00")
    assert o.snapshot().accepted == 2


def test_empty_receive_and_cancellation_never_advance_manual_time() -> None:
    time = testing.ManualClock(now_ns=100)
    with testing.Input(stream="s", protocol="midi1") as i:
        assert i.receive(timeout=3600) is None and time.now_ns == 100
        cancel = events.Cancellation()
        cancel.cancel()
        with pytest.raises(InterruptedError):
            i.receive(cancel=cancel)
        time.advance(10)
        assert time.now_ns == 110
    assert i.closed
    last = testing.ManualClock(now_ns=2**64 - 1)
    with pytest.raises(ValueError):
        last.advance(1)
    assert last.now_ns == 2**64 - 1


def test_routing_conformance_uses_shared_injected_decode_and_recorded_output() -> None:
    i = testing.Input(stream="s", protocol="midi1")
    o = testing.Output(protocol="midi1")
    router = routing.Router(
        routes=[
            routing.Route(
                name="r",
                source="s",
                destination="out",
                protocol="midi1",
                remap_channel=2,
            )
        ]
    )
    i.inject(b"\x90\x3c\x64", clock=clock(), timestamp_ns=10)
    event = i.receive()
    assert isinstance(event, events.Event)
    router.feed(event)
    delivery = router.peek()
    assert delivery is not None
    o.send_message(delivery.event.message)
    router.confirm(delivery.id)
    assert o.take_packet() == b"\x91\x3c\x64" and router.snapshot().confirmed == 1
    i.lose(1)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic)
    router.feed(loss)
    assert router.snapshot().stopped


def test_transfer_conformance_retains_known_handoffs_and_uncertain_sink_effects(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    time = testing.ManualClock()
    monkeypatch.setattr(transfer_output, "monotonic_ns", lambda: time.now_ns)
    monkeypatch.setattr(
        transfer_output,
        "sleep",
        lambda seconds: time.advance(round(seconds * 1_000_000_000)),
    )
    session = transfer.Session(
        transfer.Plan(
            [messages.SysEx(data=b"\x7d\x01"), messages.SysEx(data=b"\x7d\x02")],
            gap_ns=10,
        )
    )
    o = testing.Output(protocol="midi1")
    first = session.prepare(time.now_ns)
    assert first is not None
    o.send_message(first.message)
    session.confirm(first.index, time.now_ns)
    o.fail_next_send(accepted=True)
    report = transfer_output.run(session, cast(midi.TypedOutputConnection, o))
    assert report.confirmed_messages == 1 and report.uncertain_message == 1
    assert time.now_ns == 10 and o.snapshot().accepted == 2
    assert o.take_packet() == bytes.fromhex("f0 7d 01 f7")
    assert o.take_packet() == bytes.fromhex("f0 7d 02 f7")


@pytest.mark.parametrize("group", [None, 3])
def test_ci_browser_conformance_uses_caller_driven_time_and_in_memory_endpoints(
    group: int | None,
) -> None:
    protocol = "midi1" if group is None else "ump"
    i = testing.Input(stream="s", protocol=protocol)
    o = testing.Output(protocol=protocol)
    time = testing.ManualClock()
    identity = ci.Identity(
        manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
    )
    b = ci.Browser(
        local_muid=1,
        identity=identity,
        route=ci.Route(input="s", output="out", group=group),
    )

    def packets(message: messages.Message) -> list[bytes | midi.UmpPacket]:
        wire = codecs.encode(message)
        if group is None:
            return [wire]
        return [
            midi.UmpPacket(words=ump.encode(p))
            for p in ump.packetize_sysex7(group=group, data=wire[1:-1])
        ]

    def send(action: ci.Action) -> None:
        for p in packets(action.message):
            o.send(p)
        assert b.report_send(token=action.token, sent=True, now_ns=time.now_ns) == []

    def receive(message: messages.Message) -> list[ci.Action]:
        for p in packets(message):
            i.inject(p, clock=clock(), timestamp_ns=time.now_ns)
        assembler = ump.EventAssembler()
        while (item := i.receive()) is not None:
            assert isinstance(item, events.Event)
            if group is None:
                return b.feed(item.message, input="s", now_ns=time.now_ns)
            for observation in assembler.feed(item):
                value = observation.value
                assert isinstance(value, ump.AssembledData)
                return b.feed(
                    messages.SysEx(data=value.data),
                    input="s",
                    group=group,
                    now_ns=time.now_ns,
                )
        raise AssertionError("complete management reply was not decoded")

    send(b.start(time.now_ns)[0])
    header = ci.Header(source_muid=2, destination_muid=1)
    assert (
        receive(
            ci.DiscoveryReply(
                header=header,
                info=ci.DiscoveryInfo(
                    identity=identity, capabilities=0, max_sysex=1024, output_path=0
                ),
            )
        )
        == []
    )
    time.advance(3_000_000_000)
    send(b.poll(time.now_ns)[0])
    assert (
        receive(ci.EndpointReply(header=header, product_instance_id="simulation")) == []
    )
    assert b.snapshot().status == "completed"
    assert b.snapshot().peers[0].product_instance_id == "simulation"
    assert o.snapshot().accepted >= 2
