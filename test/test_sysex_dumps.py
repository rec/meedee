"""Opaque transfers share the Rust implementation and source-based wire vectors."""

from collections import deque
from collections.abc import Callable
from io import BytesIO
from pathlib import Path
from typing import cast

import pytest

from meedee import events, messages, midi, sysex, testing


class Input:
    stream: str = "s"
    dropped_packets: int = 0
    failure: Exception | None = None

    def __init__(self, clock: testing.ManualClock) -> None:
        self.clock = clock
        self.items: deque[messages.Message | events.StreamDiagnostic] = deque()
        self.scheduled: list[messages.Message | events.StreamDiagnostic] = []
        self.on_receive: Callable[[], None] | None = None

    def receive(
        self, *, timeout: float
    ) -> messages.Message | events.StreamDiagnostic | None:
        if timeout > 0:
            if self.on_receive is not None:
                self.on_receive()
            self.items.extend(self.scheduled)
            self.scheduled = []
        if self.items:
            return self.items.popleft()
        self.clock.advance(round(timeout * 1_000_000_000))
        return None


class Output:
    def __init__(
        self, receiver: Input, requests: list[bytes], replies: list[bytes]
    ) -> None:
        self.receiver = receiver
        self.requests = requests
        self.replies = replies
        self.sent: list[bytes] = []

    def send(self, message: messages.SysEx) -> None:
        assert message.data == self.requests[len(self.sent)]
        self.sent.append(message.data)
        self.receiver.items.append(
            messages.SysEx(self.replies[len(self.sent) - 1]).replace(stream="s")
        )


def cases() -> list[list[str]]:
    path = Path(__file__).parents[1] / "crates/meedee/tests/fixtures/sysex-dumps.txt"
    return [
        r.split("|")
        for r in path.read_text().splitlines()
        if r and not r.startswith("#")
    ]


@pytest.mark.parametrize("model,scope", sorted({(c[0], c[1]) for c in cases()}))
def test_documented_requests_retain_opaque_replies(
    model: str, scope: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    vectors = [c for c in cases() if c[:2] == [model, scope]]
    requests = [bytes.fromhex(c[2]) for c in vectors]
    replies = [
        bytes.fromhex(c[3]) + bytes(int(c[4])) + bytes.fromhex(c[5]) for c in vectors
    ]
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    output = Output(receiver, requests, replies)
    # A stale matching response must not satisfy the request.
    receiver.items.append(messages.SysEx(replies[0]).replace(stream="s"))
    report = sysex.dump(
        cast(midi.Input, receiver), cast(midi.Output, output), model=model, scope=scope
    )
    assert report.status == vectors[-1][6]
    assert report.complete == (report.status == "complete")
    assert report.requests_sent == len(requests) and not report.uncertain_send
    assert [m.data for m in report.messages] == replies
    assert output.sent == requests and report.ignored_events == 1
    assert report.wire_bytes == sum(len(r) + 2 for r in replies)
    saved = BytesIO()
    sysex.write(saved, report.messages)
    assert saved.getvalue() == b"".join(b"\xf0" + r + b"\xf7" for r in replies)


@pytest.mark.parametrize(
    "model,prefix",
    [
        ("novation/peak", "002029"),
        ("elektron/digitakt", "00203c"),
        ("moog/subsequent-37", "04"),
    ],
)
def test_manual_capture_is_unverified_and_preserves_repeated_messages(
    model: str, prefix: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    reply = messages.SysEx(bytes.fromhex(prefix) + b"\x12\x34").replace(stream="s")
    receiver.scheduled = [
        messages.NoteOn(60, channel=1).replace(stream="s"),
        messages.SysEx(b"\x7d").replace(stream="s"),
        reply,
        reply,
    ]
    report = sysex.dump(
        cast(midi.Input, receiver), model=model, silence="10 ms", timeout="100 ms"
    )
    assert report.status == "unverified" and not report.complete
    assert report.requests_sent == 0 and not report.uncertain_send
    assert report.messages == [reply, reply] and report.ignored_events == 2
    assert clock.now_ns == 10_000_000


@pytest.mark.parametrize("corrupt", [False, True])
def test_opaque_reply_contents_survive_checksum_validation(
    corrupt: bool,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    reply = bytes.fromhex("4300570018000000") + bytes([0, 1, 127, 64]) * 6
    checksum = (-sum(reply[3:])) & 127
    reply += bytes([checksum ^ int(corrupt)])
    output = Output(receiver, [bytes.fromhex("432057000000")], [reply])
    report = sysex.dump(
        cast(midi.Input, receiver),
        cast(midi.Output, output),
        model="yamaha/vl70-m",
        scope="system",
    )
    assert report.status == ("failed" if corrupt else "complete")
    assert report.messages[0].data == reply and report.complete != corrupt
    if corrupt:
        assert "checksum" in (report.reason or "")


def test_loss_after_received_prefix_prevents_verified_completion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    receiver.on_receive = lambda: setattr(receiver, "dropped_packets", 1)
    reply = bytes.fromhex("012f0f") + bytes(50)
    output = Output(receiver, [bytes.fromhex("012f0e")], [reply])
    report = sysex.dump(
        cast(midi.Input, receiver),
        cast(midi.Output, output),
        model="sequential/prophet-rev2",
        scope="system",
    )
    assert report.status == "failed" and not report.complete
    assert report.messages[0].data == reply


def test_limits_and_cancellation_preserve_only_admitted_messages(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    reply = messages.SysEx(b"\x04\x01").replace(stream="s")
    receiver.scheduled = [reply, reply]
    report = sysex.dump(
        cast(midi.Input, receiver), model="moog/subsequent-37", message_limit=1
    )
    assert report.status == "limit_reached" and report.messages == [reply]
    cancel = events.Cancellation()
    cancel.cancel()
    output = Output(receiver, [], [])
    report = sysex.dump(
        cast(midi.Input, receiver),
        cast(midi.Output, output),
        model="yamaha/vl70-m",
        cancel=cancel,
    )
    assert (
        report.status == "cancelled" and not report.uncertain_send and not output.sent
    )


def test_empty_capture_times_out_and_uncertain_send_never_retries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    report = sysex.dump(
        cast(midi.Input, receiver), model="moog/subsequent-37", timeout="10 ms"
    )
    assert report.status == "timed_out" and not report.messages
    output = testing.Output(protocol="midi1")
    output.fail_next_send(accepted=True)
    report = sysex.dump(
        cast(midi.Input, receiver),
        cast(midi.Output, output),
        model="yamaha/vl70-m",
        scope="system",
    )
    assert (
        report.status == "failed"
        and report.uncertain_send
        and report.requests_sent == 0
    )
    assert output.snapshot().accepted == 1
    output.close()


def test_unsupported_requests_fail_before_sending() -> None:
    receiver = Input(testing.ManualClock())
    output = Output(receiver, [], [])
    with pytest.raises(ValueError, match="manual"):
        sysex.dump(
            cast(midi.Input, receiver),
            cast(midi.Output, output),
            model="moog/subsequent-37",
        )
    with pytest.raises(ValueError, match="conflict"):
        sysex.dump(
            cast(midi.Input, receiver),
            cast(midi.Output, output),
            model="sequential/prophet-rev2",
        )
    with pytest.raises(ValueError, match="conflict"):
        sysex.dump(
            cast(midi.Input, receiver),
            cast(midi.Output, output),
            model="korg/minilogue-xd",
        )
    assert not output.sent


@pytest.mark.parametrize(
    "model,count,last",
    [
        ("yamaha/vl70-m", 71, "43205740003f"),
        ("roland/vt-4", 35, "4110000000511161030000000000100c"),
        ("behringer/deepmind-12", 1025, "002032200001077f"),
    ],
)
def test_all_scope_requests_each_documented_block_once(
    model: str, count: int, last: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    sent: list[bytes] = []

    class Instrument:
        def send(self, message: messages.SysEx) -> None:
            data = message.data
            sent.append(data)
            if model == "yamaha/vl70-m":
                address = data[3:6]
                size = {0: 24, 0x30: 0x56B, 0x40: 0xA3}[address[0]]
                reply = (
                    bytes([0x43, 0, 0x57, size >> 7, size & 127])
                    + address
                    + bytes(size)
                )
                reply += bytes([(-sum(reply[3:])) & 127])
            elif model == "roland/vt-4":
                size = sum(b << (7 * (3 - i)) for i, b in enumerate(data[11:15]))
                reply = data[:6] + b"\x12" + data[7:11] + bytes(size)
                reply += bytes([(-sum(reply[7:])) & 127])
            else:
                if data[5] == 5:
                    reply = data[:5] + b"\x06\x06" + bytes(56)
                else:
                    assert data[5] == 1 and data[6] < 8 and data[7] < 128
                    reply = data[:5] + b"\x02\x06" + data[6:8] + bytes(278)
            receiver.items.append(messages.SysEx(reply).replace(stream="s"))

    report = sysex.dump(
        cast(midi.Input, receiver), cast(midi.Output, Instrument()), model=model
    )
    assert report.complete and report.status == "complete"
    assert len(set(sent)) == count == report.requests_sent == len(report.messages)
    assert sent[-1] == bytes.fromhex(last)


def test_unrelated_and_foreign_events_do_not_extend_silence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    reply = messages.SysEx(b"\x04\x01").replace(stream="s")
    receiver.scheduled = [
        reply,
        reply.replace(stream="other"),
        messages.SysEx(b"\x7d").replace(stream="s"),
    ]
    receiver.on_receive = lambda: clock.advance(5_000_000)
    report = sysex.dump(
        cast(midi.Input, receiver),
        model="moog/subsequent-37",
        silence="10 ms",
        timeout="100 ms",
    )
    assert report.status == "unverified" and report.messages == [reply]
    assert report.ignored_events == 2 and clock.now_ns == 15_000_000


def test_truncated_frame_diagnostic_retains_only_complete_prefix(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    decoder.feed(
        events.ReceivedPacket(
            clock=events.Clock(
                identity="c",
                backend=midi.Backend.CoreMidi,
                mode=midi.TimestampMode.Absolute,
            ),
            packet=b"\xf0\x04\x01",
            sequence=0,
            timestamp_ns=0,
        )
    )
    reply = messages.SysEx(b"\x04\x02").replace(stream="s")
    receiver.scheduled = [reply, *decoder.finish()]
    report = sysex.dump(cast(midi.Input, receiver), model="moog/subsequent-37")
    assert report.status == "failed" and not report.complete
    assert report.messages == [reply] and report.reason


@pytest.mark.parametrize("limits", [{"byte_limit": 3}, {"payload_limit": 1}])
def test_byte_limits_apply_before_retention(
    limits: dict[str, int], monkeypatch: pytest.MonkeyPatch
) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    receiver.scheduled = [messages.SysEx(b"\x04\x01").replace(stream="s")]
    report = sysex.dump(
        cast(midi.Input, receiver), model="moog/subsequent-37", **limits
    )
    assert (
        report.status == "limit_reached"
        and not report.messages
        and report.wire_bytes == 0
    )


def test_documented_negative_reply_is_retained(monkeypatch: pytest.MonkeyPatch) -> None:
    clock = testing.ManualClock()
    monkeypatch.setattr(sysex, "monotonic_ns", lambda: clock.now_ns)
    receiver = Input(clock)
    reply = bytes.fromhex("423000015124")
    output = Output(receiver, [bytes.fromhex("42300001510e")], [reply])
    report = sysex.dump(
        cast(midi.Input, receiver),
        cast(midi.Output, output),
        model="korg/minilogue-xd",
        scope="system",
    )
    assert report.status == "failed" and "rejected" in (report.reason or "")
    assert report.messages[0].data == reply
