"""Management wire vectors and caller-driven CI state, with no OS MIDI."""

from pathlib import Path

import pytest

from meedee import ci, codecs, messages, ump


def identity() -> ci.Identity:
    return ci.Identity(
        manufacturer=b"\x7d\x00\x00", family=1, model=2, revision=b"\x01\x02\x03\x04"
    )


def header(source: int, destination: int, version: int = 2) -> ci.Header:
    return ci.Header(source_muid=source, destination_muid=destination, version=version)


def info(path: int | None = 3) -> ci.DiscoveryInfo:
    return ci.DiscoveryInfo(
        identity=identity(), capabilities=0, max_sysex=512, output_path=path
    )


def values() -> list[messages.Message]:
    return [
        ci.Discovery(header=header(1, ci.BROADCAST_MUID), info=info()),
        ci.DiscoveryReply(header=header(2, 1), info=info()),
        ci.EndpointInquiry(header=header(1, 2)),
        ci.EndpointReply(header=header(2, 1), product_instance_id="ABC"),
        ci.InvalidateMuid(header=header(1, ci.BROADCAST_MUID), target=2),
        ci.Ack(
            header=header(2, 1).replace(address=ci.Address.channel(channel=1)),
            notice=ci.Notice(
                original=114,
                status=0,
                status_data=0,
                details=b"\x00" * 5,
                message_text=b"OK!",
            ),
        ),
        ci.Nak(
            header=header(2, 1),
            notice=ci.Notice(
                original=114,
                status=1,
                status_data=0,
                details=b"\x00" * 5,
                message_text=b"",
            ),
        ),
        ci.Nak(header=header(2, 1, 1)),
        ci.Discovery(header=header(1, ci.BROADCAST_MUID, 1), info=info(None)),
    ]


def test_management_matches_shared_vectors_and_preserves_immutable_values() -> None:
    fixture = (
        Path(__file__).resolve().parents[1] / "crates/meedee/tests/fixtures/ci.txt"
    )
    vectors = [
        bytes.fromhex(s.split("|")[1])
        for s in fixture.read_text().splitlines()
        if not s.startswith("#")
    ]
    assert len(vectors) == len(values())
    for value, wire in zip(values(), vectors, strict=True):
        assert isinstance(value, messages.Message)
        assert codecs.encode(value) == wire
        decoded = ci.decode(messages.SysEx(data=wire[1:-1]))
        assert decoded == value and hash(decoded) == hash(value)
        assert decoded is not None and decoded.replace() == value
        with pytest.raises(AttributeError):
            decoded.header = header(1, 2)  # ty: ignore[invalid-assignment]
    assert ci.Address.channel(channel=16).channel_number == 16
    assert ci.Address.GROUP.channel_number is None
    assert ci.decode(messages.SysEx(data=b"\x7d\x01")) is None
    decoder = codecs.Decoder()
    assert isinstance(decoder.feed(vectors[0])[0], messages.SysEx)
    assert isinstance(ci.decode(decoder.feed(vectors[0])[0]), ci.Discovery)  # ty: ignore[invalid-argument-type]


@pytest.mark.parametrize("invalid", [True, 1.0, "1", -1, 0x0FFFFF00, ci.BROADCAST_MUID])
def test_invalid_local_muid_values_are_not_coerced(invalid: object) -> None:
    with pytest.raises((TypeError, ValueError)):
        ci.Header(source_muid=invalid, destination_muid=1)  # ty: ignore[invalid-argument-type]


def test_versions_malformed_frames_and_ump_sysex_keep_explicit_interpretation() -> None:
    wire = codecs.encode(values()[0])
    for cut in range(3, len(wire) - 2):
        with pytest.raises(ValueError):
            ci.decode(messages.SysEx(data=wire[1 : cut + 1]))
    future = bytearray(wire[1:-1])
    future[4] = 3
    future.extend(b"\x01\x7f")
    value = ci.decode(messages.SysEx(data=future))
    assert value is not None and value.extensions == b"\x01\x7f"
    assert codecs.encode(value)[1:-1] == future
    future[4] = 32
    with pytest.raises(ValueError):
        ci.decode(messages.SysEx(data=future))
    assembler = ump.Assembler()
    assembled = []
    for p in ump.packetize_sysex7(group=16, data=wire[1:-1]):
        assembled.extend(assembler.feed(p))
    assert len(assembled) == 1 and isinstance(assembled[0], ump.AssembledData)
    assert ci.decode(messages.SysEx(data=assembled[0].data)) == values()[0]
    with pytest.raises(ValueError):
        ci.Discovery(header=header(1, 2), info=info())
    with pytest.raises(ValueError):
        ci.EndpointReply(header=header(2, 1), product_instance_id="é")


def session(local: int, name: str, path: int = 0) -> ci.Session:
    return ci.Session(
        local_muid=local,
        identity=identity(),
        route=ci.Route(input=name, output=f"{name}-out"),
        output_path=path,
    )


def test_session_requires_explicit_discovery_reply_and_loss_handling() -> None:
    a, b = session(1, "a", 3), session(2, "b")
    request = a.discover(now_ns=0)
    assert request.route.output == "a-out"
    a.report_send(token=request.token, sent=True, now_ns=1)
    updates = b.feed(request.message, input="b", now_ns=2)
    actions = [u for u in updates if isinstance(u, ci.Action)]
    assert len(actions) == 1 and isinstance(actions[0].message, ci.DiscoveryReply)
    reply = actions[0].message
    assert isinstance(reply, ci.DiscoveryReply) and reply.info.output_path == 3
    b.report_send(token=actions[0].token, sent=True, now_ns=3)
    a.feed(reply, input="a", now_ns=4)
    assert a.peers[0].muid == 2 and a.peers[0].info.capabilities == 0
    query = a.inquire_endpoint(peer_muid=2, now_ns=5, timeout_ns=100)
    a.report_send(token=query.token, sent=True, now_ns=6)
    updates = b.feed(query.message, input="b", now_ns=7)
    assert any(
        isinstance(u, ci.Observation) and u.kind == "endpoint_request" for u in updates
    )
    response = b.respond(
        ci.EndpointReply(header=header(2, 1), product_instance_id="serial"),
        now_ns=8,
        timeout_ns=100,
    )
    b.report_send(token=response.token, sent=True, now_ns=9)
    a.feed(response.message, input="a", now_ns=10)
    assert a.peers[0].product_instance_id == "serial"
    a.reset(reason="known packet loss")
    assert not a.active and a.peers == [] and a.pending_count == 0
    with pytest.raises(ValueError, match="restart"):
        a.discover(now_ns=11)
    with pytest.raises(ValueError, match="randomized"):
        a.restart(local_muid=1, now_ns=11)
    a.restart(local_muid=3, now_ns=12)
    assert a.active


def test_deadlines_rejected_categories_and_responder_collisions_are_bounded() -> None:
    a = ci.Session(
        local_muid=1,
        identity=identity(),
        route=ci.Route(input="a", output="out"),
        exchange_limit=1,
        peer_limit=1,
    )
    request = a.discover(now_ns=0)
    with pytest.raises(ValueError):
        a.discover(now_ns=1)
    result = a.poll(now_ns=3_000_000_000)
    assert any(
        isinstance(u, ci.Observation) and u.kind == "send_confirmation_timeout"
        for u in result
    )
    with pytest.raises(ValueError):
        a.report_send(token=request.token, sent=True, now_ns=3_000_000_000)
    collision = ci.Discovery(header=header(1, ci.BROADCAST_MUID), info=info())
    updates = a.feed(collision, input="a", now_ns=3_000_000_001)
    assert not a.active
    assert any(
        isinstance(u, ci.Action) and isinstance(u.message, ci.InvalidateMuid)
        for u in updates
    )
    a.restart(local_muid=3, now_ns=3_000_000_002)
    unknown = bytearray(codecs.encode(ci.EndpointInquiry(header=header(2, 3)))[1:-1])
    unknown[3] = 0x34
    updates = a.feed(messages.SysEx(data=unknown), input="a", now_ns=3_000_000_003)
    assert any(
        isinstance(u, ci.Action) and isinstance(u.message, ci.Nak) for u in updates
    )
    with pytest.raises(ValueError, match="route"):
        a.feed(messages.SysEx(data=unknown), input="foreign", now_ns=3_000_000_004)


def test_early_replies_wait_for_confirmation_and_stay_invalidated() -> None:
    a = session(1, "a", 3)
    discovery = a.discover(now_ns=0)
    reply = ci.DiscoveryReply(header=header(2, 1), info=info())
    updates = a.feed(reply, input="a", now_ns=1)
    assert not a.peers
    assert any(
        isinstance(u, ci.Observation) and u.kind == "reply_pending_send_result"
        for u in updates
    )
    a.report_send(token=discovery.token, sent=True, now_ns=2)
    assert a.peers[0].muid == 2
    query = a.inquire_endpoint(peer_muid=2, now_ns=3, timeout_ns=100)
    a.feed(
        ci.EndpointReply(header=header(2, 1), product_instance_id="serial"),
        input="a",
        now_ns=4,
    )
    assert a.peers[0].product_instance_id is None
    a.report_send(token=query.token, sent=False, now_ns=5)
    assert a.peers[0].product_instance_id is None
    a.feed(
        ci.InvalidateMuid(header=header(2, ci.BROADCAST_MUID), target=2),
        input="a",
        now_ns=6,
    )
    a.feed(reply, input="a", now_ns=7)
    assert not a.peers and a.pending_count == 0
    limited = ci.Session(
        local_muid=1,
        identity=identity(),
        route=ci.Route(input="s", output="out"),
        output_path=3,
        reply_byte_limit=1,
    )
    limited.discover(now_ns=0)
    updates = limited.feed(reply, input="s", now_ns=1)
    assert any(
        isinstance(u, ci.Observation) and u.kind == "resource_limit" for u in updates
    )
