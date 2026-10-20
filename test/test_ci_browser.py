"""Bounded CI scans and deterministic byte/UMP hosts without native devices."""

import json
from collections import deque
from types import SimpleNamespace
from typing import Self, cast

import pytest

from meedee import ci, ci_live, codecs, events, messages, midi, ump


def identity() -> ci.Identity:
    return ci.Identity(
        manufacturer=b"\x7d\x00\x00", family=1, model=2, revision=b"\x00" * 4
    )


def browser(group: int | None = None, detail_limit: int = 128) -> ci.Browser:
    return ci.Browser(
        local_muid=1,
        identity=identity(),
        route=ci.Route(input="in", output="out", group=group),
        detail_limit=detail_limit,
    )


def reply(source: int = 2) -> ci.DiscoveryReply:
    return ci.DiscoveryReply(
        header=ci.Header(source_muid=source, destination_muid=1),
        info=ci.DiscoveryInfo(
            identity=identity(), capabilities=0, max_sysex=1024, output_path=0
        ),
    )


def test_coordinator_preserves_frozen_peers_and_bounds_diagnostic_samples() -> None:
    b = browser(detail_limit=1)
    a = b.start(0)[0]
    b.report_send(token=a.token, sent=True, now_ns=0)
    b.feed(reply(), input="in", now_ns=1)
    saved = b.snapshot()
    a = b.poll(3_000_000_000)[0]
    assert isinstance(a.message, ci.EndpointInquiry)
    b.report_send(token=a.token, sent=True, now_ns=3_000_000_000)
    b.feed(
        ci.EndpointReply(
            header=ci.Header(source_muid=2, destination_muid=1),
            product_instance_id="test-peer",
        ),
        input="in",
        now_ns=3_000_000_001,
    )
    report = b.snapshot()
    assert report.status == "completed" and report.confirmed_actions == 2
    assert report.peers[0].product_instance_id == "test-peer"
    assert saved.peers[0].product_instance_id is None
    assert len(report.diagnostics) == 1 and report.omitted_diagnostics > 0
    saved.peers.clear()
    assert len(b.snapshot().peers) == 1
    with pytest.raises(AttributeError):
        report.status = "ready"  # ty: ignore[invalid-assignment]
    with pytest.raises(ValueError):
        b.start(4_000_000_000)


def test_collision_retires_the_scan_and_hands_off_required_cleanup_once() -> None:
    b = browser()
    stale = b.start(0)[0]
    cleanup = b.feed(reply(1), input="in", now_ns=1)
    assert b.snapshot().status == "stopping" and len(cleanup) == 1
    assert isinstance(cleanup[0].message, ci.InvalidateMuid)
    with pytest.raises(ValueError, match="retired"):
        b.report_send(token=stale.token, sent=True, now_ns=2)
    b.report_send(token=cleanup[0].token, sent=True, now_ns=2)
    assert b.snapshot().status == "stopped" and b.snapshot().pending_tokens == []


def test_timeout_and_known_loss_preserve_observed_peer_declarations() -> None:
    b = browser()
    a = b.start(0)[0]
    b.report_send(token=a.token, sent=True, now_ns=0)
    b.feed(reply(), input="in", now_ns=1)
    with pytest.raises(ValueError, match="regressed"):
        b.poll(0)
    b.stop("input loss")
    b.stop("later reason")
    assert b.snapshot().reason == "input loss" and len(b.snapshot().peers) == 1


class Receiver:
    stream = "in"

    def __init__(self, clock: list[int]) -> None:
        self.clock = clock
        self.items: deque[events.Event | events.StreamDiagnostic] = deque()
        self.dropped_packets = 0
        self.failure: Exception | None = None
        self.closed = False

    def receive(
        self, *, timeout: float
    ) -> events.Event | events.StreamDiagnostic | None:
        if self.items:
            self.clock[0] += 1
            return self.items.popleft()
        self.clock[0] += round(timeout * 1_000_000_000)
        return None

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True


class Output:
    def __init__(
        self,
        receiver: Receiver,
        group: int | None,
        *,
        fail: bool = False,
        answer: bool = True,
    ) -> None:
        self.receiver = receiver
        self.group = group
        self.fail = fail
        self.answer = answer
        self.sent: list[messages.Message] = []
        self.assembler = ump.Assembler()
        self.closed = False

    def send_message(self, message: messages.Message) -> None:
        self.sent.append(message)
        if self.fail and len(self.sent) == 2:
            raise midi.SendError("second packet may have been sent")
        if not self.answer:
            return
        if isinstance(message, ump.SysEx7Packet):
            values = self.assembler.feed(message)
            if not values:
                return
            data = values[0]
            assert isinstance(data, ump.AssembledData)
            command = ci.decode(messages.SysEx(data=data.data))
        else:
            command = message
        assert isinstance(command, (ci.Discovery, ci.EndpointInquiry))
        header = ci.Header(source_muid=2, destination_muid=command.header.source_muid)
        response: messages.Message
        if isinstance(command, ci.Discovery):
            response = ci.DiscoveryReply(
                header=header,
                info=ci.DiscoveryInfo(
                    identity=identity(), capabilities=0, max_sysex=1024, output_path=0
                ),
            )
        else:
            response = ci.EndpointReply(header=header, product_instance_id="test-peer")
        if self.group is None:
            packets: list[messages.Message] = [
                messages.SysEx(data=codecs.encode(response)[1:-1])
            ]
        else:
            packets = list(
                ump.packetize_sysex7(
                    group=self.group, data=codecs.encode(response)[1:-1]
                )
            )
        self.receiver.items.extend(
            events.Event(stream="in", message=p) for p in packets
        )

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True


def run(
    b: ci.Browser,
    receiver: Receiver,
    output: Output,
    cancel: events.Cancellation | None = None,
) -> ci.BrowseReport:
    return ci_live.run(
        b,
        cast(midi.TypedInputConnection, receiver),
        cast(midi.TypedOutputConnection, output),
        cancel=cancel,
    )


@pytest.mark.parametrize("group", [None, 3])
def test_live_scan_preserves_both_protocols_and_obtains_endpoint_ids(
    group: int | None, monkeypatch: pytest.MonkeyPatch
) -> None:
    clock = [0]
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: clock[0])
    receiver = Receiver(clock)
    output = Output(receiver, group)
    report = run(browser(group), receiver, output)
    assert report.status == "completed" and report.problems == 0
    assert (
        report.confirmed_actions == 2
        and report.peers[0].product_instance_id == "test-peer"
    )
    assert not receiver.closed and not output.closed
    if group is not None:
        assert len(output.sent) > 2
        assert all(
            isinstance(p, ump.SysEx7Packet) and p.group == group for p in output.sent
        )


def test_partial_ump_send_stops_with_uncertain_action_and_never_retries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: 0)
    receiver = Receiver([0])
    output = Output(receiver, 1, fail=True)
    b = browser(1)
    with pytest.raises(midi.SendError):
        run(b, receiver, output)
    report = b.snapshot()
    assert report.status == "stopped" and report.confirmed_actions == 0
    assert report.uncertain_token == 0 and len(output.sent) == 2


def test_input_loss_and_pre_cancel_send_nothing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: 0)
    receiver = Receiver([0])
    output = Output(receiver, None)
    receiver.dropped_packets = 1
    assert run(browser(), receiver, output).status == "stopped"
    assert output.sent == []
    receiver.dropped_packets = 0
    token = events.Cancellation()
    token.cancel()
    assert run(browser(), receiver, output, token).status == "stopped"
    assert output.sent == []


def test_no_peers_is_a_complete_scan_without_an_offline_claim(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = [0]
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: clock[0])
    receiver = Receiver(clock)
    output = Output(receiver, None, answer=False)
    report = run(browser(), receiver, output)
    assert report.status == "completed" and report.peers == [] and report.problems == 0


def test_cli_requires_identity_and_explicit_ump_group_before_transport(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    pytest.importorskip("tyro")
    pytest.importorskip("pydantic")
    from meedee import cli

    def forbidden(**kwargs: object) -> None:
        pytest.fail("transport constructed for invalid CLI configuration")

    monkeypatch.setattr(midi, "TypedOutput", forbidden)
    for extra in [
        [],
        [
            "--manufacturer",
            "7d0000",
            "--family",
            "1",
            "--model",
            "2",
            "--revision",
            "00000000",
        ],
    ]:
        with pytest.raises(SystemExit) as result:
            cli.main(
                [
                    "ci-browse",
                    "--input-port",
                    "in",
                    "--output-port",
                    "out",
                    "--protocol",
                    "ump",
                    *extra,
                ]
            )
        assert result.value.code == 2


def test_cli_uses_fresh_muid_and_closes_both_connections(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    pytest.importorskip("tyro")
    pytest.importorskip("pydantic")
    from meedee import _ci_cli, cli

    clock = [0]
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: clock[0])
    monkeypatch.setattr(_ci_cli, "randbelow", lambda bound: 123)
    receiver = Receiver(clock)
    output = Output(receiver, None)

    class Client:
        def __init__(self, connection: Receiver | Output, name: str) -> None:
            self.connection = connection
            self.name = name

        def ports(self) -> list[SimpleNamespace]:
            return [SimpleNamespace(name=self.name)]

        def connect(self, port: SimpleNamespace) -> Receiver | Output:
            return self.connection

    monkeypatch.setattr(midi, "TypedInput", lambda **kwargs: Client(receiver, "in"))
    monkeypatch.setattr(midi, "TypedOutput", lambda **kwargs: Client(output, "out"))
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "ci-browse",
                "--input-port",
                "in",
                "--output-port",
                "out",
                "--protocol",
                "midi1",
                "--manufacturer",
                "7d0000",
                "--family",
                "1",
                "--model",
                "2",
                "--revision",
                "00000000",
                "--output",
                "json",
            ]
        )
    assert result.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["local_muid"] == 123 and data["status"] == "completed"
    assert data["peers"][0]["product_instance_id"] == "test-peer"
    assert receiver.closed and output.closed


def test_ump_assembly_diagnostics_stop_the_scan(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = [0]
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: clock[0])
    receiver = Receiver(clock)
    receiver.items.append(
        events.Event(
            stream="in", message=ump.SysEx7Packet(group=1, format=2, data=b"\x01")
        )
    )
    output = Output(receiver, 1, answer=False)
    report = run(browser(1), receiver, output)
    assert report.status == "stopped" and report.reason is not None
    assert "assembly diagnostic" in report.reason
    assert report.confirmed_actions == 1


def test_stream_diagnostic_stops_without_interpreting_later_input(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = [0]
    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: clock[0])
    receiver = Receiver(clock)
    decoder = events.PacketDecoder(stream="in", protocol=midi.Protocol.Midi1)
    receiver.items.append(decoder.loss(lost=1, reason="known framing loss"))
    output = Output(receiver, None)
    report = run(browser(), receiver, output)
    assert report.status == "stopped" and report.peers == []
    assert report.reason == "known framing loss" and report.confirmed_actions == 1


def test_cancel_before_first_packet_does_not_claim_uncertain_delivery(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class Cancellation:
        def __init__(self) -> None:
            self.checks = 0

        @property
        def cancelled(self) -> bool:
            self.checks += 1
            return self.checks >= 2

    monkeypatch.setattr(ci_live, "monotonic_ns", lambda: 0)
    receiver = Receiver([0])
    output = Output(receiver, 1)
    report = run(
        browser(1), receiver, output, cast(events.Cancellation, Cancellation())
    )
    assert report.status == "stopped" and report.uncertain_token is None
    assert report.confirmed_actions == 0 and output.sent == []
