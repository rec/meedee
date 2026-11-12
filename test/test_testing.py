"""Public deterministic transports and selected toolkit conformance workflows."""

import pytest

from middi import (
    ci,
    codecs,
    events,
    messages,
    midi,
    routing,
    testing,
    ump,
)


def clock() -> events.Clock:
    return events.Clock(identity="simulation", origin="manual")


def test_fragments_reuse_running_status_and_preserve_signed_timestamp_spans() -> None:
    i = testing.Input(stream="s", encoding="bytes")
    assert i.inject(b"\x90\x3c", clock=clock(), timestamp_ns=-10)
    assert i.receive(timeout=100) is None
    assert i.inject(b"\x64\x3d\x65", clock=clock(), timestamp_ns=20)
    first = i.receive()
    second = i.receive()
    assert isinstance(first, messages.Message) and isinstance(second, messages.Message)
    assert first == messages.NoteOn(channel=1, note=60, velocity=100)
    assert second == messages.NoteOn(channel=1, note=61, velocity=101)
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
    i = testing.Input(stream="s", encoding="bytes", packet_limit=1)
    i.inject(b"\x90\x3c", clock=clock())
    assert not i.inject(b"\x64", clock=clock())
    saved = i.snapshot()
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.lost == 1
    assert i.dropped_packets == 1 and saved.next_sequence == 2
    assert i.inject(b"\x90\x3d\x64", clock=clock())
    event = i.receive()
    assert isinstance(event, messages.Message) and event.span is not None
    assert event.span.first_packet == 2 and i.receive() is None
    assert saved.queued_packets == 1 and i.snapshot().queued_packets == 0
    with pytest.raises(AttributeError):
        saved.dropped_packets = 0  # ty: ignore[invalid-assignment]


def test_decoded_overflow_and_explicit_loss_reset_interpretation() -> None:
    i = testing.Input(stream="s", encoding="bytes", event_limit=1)
    i.inject(b"\x90\x3c\x64\x3d\x65", clock=clock())
    assert isinstance(i.receive(), messages.Message)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.kind == "loss"
    assert i.snapshot().dropped_events == 1 and i.dropped_packets == 0
    i.lose(2)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic) and loss.lost == 2
    assert i.receive() is None


def test_ump_values_keep_native_width_and_protocol_mismatches_are_rejected() -> None:
    message = messages.ControlChange(
        version=2, group=4, channel=2, controller=7, value=0xFEDCBA98
    )
    o = testing.Output(encoding="ump")
    o.send(message)
    packet = o.take_packet()
    assert isinstance(packet, midi.UmpPacket)
    i = testing.Input(stream="s", encoding="ump")
    i.inject(packet, clock=clock())
    result = i.receive()
    assert isinstance(result, messages.Message) and result == message
    with pytest.raises(ValueError):
        i.inject(b"\xf8", clock=clock())
    with pytest.raises(ValueError, match="group"):
        o.send(messages.TimingClock())


def test_worker_failure_is_visible_and_close_retires_input_without_devices() -> None:
    i = testing.Input(stream="s", encoding="bytes")
    i.inject(b"\xf8", clock=clock())
    i.fail("simulated disconnect")
    assert isinstance(i.failure, midi.WorkerError)
    assert isinstance(i.receive(), messages.Message)
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
    o = testing.Output(encoding="bytes")
    o.fail_next_send(accepted=accepted, reason="uncertain")
    with pytest.raises(midi.SendError, match="uncertain"):
        o.send(messages.TimingClock())
    report = o.snapshot()
    assert report.attempts == 1 and report.accepted == int(accepted)
    o.close()
    assert o.take_packet() == (b"\xf8" if accepted else None)
    assert o.snapshot().accepted == int(accepted)
    with pytest.raises(midi.StateError):
        o.send(b"\xf8")


def test_output_capacity_never_discards_accepted_evidence() -> None:
    o = testing.Output(encoding="bytes", packet_limit=1, byte_limit=3)
    o.send(b"\x90\x3c\x64")
    with pytest.raises(midi.ResourceError):
        o.send(b"\x80\x3c\x00")
    assert o.snapshot().attempts == 2 and o.snapshot().accepted == 1
    assert o.take_packet() == b"\x90\x3c\x64"
    o.send(b"\x80\x3c\x00")
    assert o.snapshot().accepted == 2


def test_empty_receive_and_cancellation_never_advance_manual_time() -> None:
    time = testing.ManualClock(now_ns=100)
    with testing.Input(stream="s", encoding="bytes") as i:
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
    i = testing.Input(stream="s", encoding="bytes")
    o = testing.Output(encoding="bytes")
    router = routing.Router(
        routes=[
            routing.Route(
                name="r",
                source="s",
                destination="out",
                encoding="bytes",
                remap_channel=2,
            )
        ]
    )
    i.inject(b"\x90\x3c\x64", clock=clock(), timestamp_ns=10)
    event = i.receive()
    assert isinstance(event, messages.Message)
    router.feed(event)
    delivery = router.peek()
    assert delivery is not None
    o.send(delivery.message)
    router.confirm(delivery.id)
    assert o.take_packet() == b"\x91\x3c\x64" and router.snapshot().confirmed == 1
    i.lose(1)
    loss = i.receive()
    assert isinstance(loss, events.StreamDiagnostic)
    router.feed(loss)
    assert router.snapshot().stopped


@pytest.mark.parametrize("group", [None, 3])
def test_ci_browser_conformance_uses_caller_driven_time_and_in_memory_endpoints(
    group: int | None,
) -> None:
    encoding = "bytes" if group is None else "ump"
    i = testing.Input(stream="s", encoding=encoding)
    o = testing.Output(encoding=encoding)
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
            for p in ump.packetize_sysex7(messages.SysEx(wire[1:-1], group=group))
        ]

    def send(action: ci.Action) -> None:
        for p in packets(action.message):
            o.send(p)
        assert b.report_send(token=action.token, sent=True, now_ns=time.now_ns) == []

    def receive(message: messages.Message) -> list[ci.Action]:
        for p in packets(message):
            i.inject(p, clock=clock(), timestamp_ns=time.now_ns)
        assembler = ump.Assembler()
        while (item := i.receive()) is not None:
            assert isinstance(item, messages.Message)
            if group is None:
                return b.feed(item, input="s", now_ns=time.now_ns)
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


def test_output_accepts_messages_packets_and_compound_commands() -> None:
    output = testing.Output(encoding="bytes")
    note = messages.NoteOn(channel=1, note=60, velocity=100)
    output.send(note)
    output.send(b"\x90\x3c\x64")
    output.send(
        messages.ControlChange(resolution=14, channel=1, controller=7, value=128)
    )
    assert [output.take_packet() for _ in range(4)] == [
        b"\x90\x3c\x64",
        b"\x90\x3c\x64",
        b"\xb0\x07\x01",
        b"\xb0\x27\x00",
    ]
    assert output.snapshot().accepted == 4


def test_compound_output_failure_preserves_prefix_and_reports_progress() -> None:
    output = testing.Output(encoding="bytes", packet_limit=1)
    with pytest.raises(midi.CompoundSendError) as failure:
        output.send(
            messages.ControlChange(resolution=14, channel=1, controller=7, value=128)
        )
    assert failure.value.completed_messages == 1
    assert failure.value.total_messages == 2
    assert isinstance(failure.value.__cause__, midi.ResourceError)
    assert output.take_packet() == b"\xb0\x07\x01"
    assert output.take_packet() is None
