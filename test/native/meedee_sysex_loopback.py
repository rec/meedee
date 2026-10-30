"""Opt-in SysEx/MTS workflows between exclusively test-owned MIDI 1 endpoints."""

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from tempfile import TemporaryDirectory
from uuid import uuid4

from meedee import (
    codecs,
    events,
    messages,
    midi,
    transfer,
    transfer_input,
    transfer_output,
    tuning,
    tuning_live,
)


def connect(name: str, isolated: bool) -> midi.OutputConnection:
    client = midi.Output(isolated=isolated)
    matches = [p for p in client.ports() if p.name == name]
    assert len(matches) == 1, "test-owned endpoint was not discovered uniquely"
    return client.connect(matches[0])


def respond(
    receiver: midi.TypedInputConnection,
    output: midi.OutputConnection,
    request: tuning.BulkDumpRequest | tuning.BankedDumpRequest,
    reply: messages.SysEx,
) -> None:
    received = receiver.receive(timeout=3)
    assert isinstance(received, events.Event), received
    assert codecs.encode(received.message) == codecs.encode(request)
    # Keep nonmatching traffic observable without pretending it is a reply.
    output.send(messages.TimingClock())
    output.send(messages.SysEx(data=b"\x7d\x01"))
    output.send(reply)
    assert receiver.dropped_packets == 0 and receiver.failure is None


def exercise(isolated: bool, directory: Path) -> None:
    suffix = uuid4().hex
    names = [f"meedee-syx-{suffix}-{n}" for n in (1, 2)]
    with (
        midi.TypedInput(isolated=isolated).create_virtual(names[0]) as receiver,
        midi.TypedInput(isolated=isolated).create_virtual(names[1]) as peer,
        connect(names[1], isolated) as output,
        connect(names[0], isolated) as peer_output,
        ThreadPoolExecutor(max_workers=1) as worker,
    ):
        messages_to_capture = [
            messages.SysEx(data=b"\x7d\x01"),
            messages.SysEx(data=b"\x7d" + bytes(range(128)) * 32),
        ]
        plan = transfer.Plan(messages_to_capture, gap_ns=1_000_000)
        capture = transfer.Capture(
            stream=receiver.stream,
            expected_messages=2,
            timeout_ns=3_000_000_000,
        )
        delivery = worker.submit(
            transfer_output.run, transfer.Session(plan), peer_output
        )
        report = transfer_input.run(capture, receiver)
        assert report.status == "completed", report.reason
        assert delivery.result(timeout=4).status == "completed"
        capture_path = directory / f"capture-{isolated}.syx"
        capture.write(capture_path)
        expected = b"".join(codecs.encode(m) for m in messages_to_capture)
        assert capture_path.read_bytes() == expected
        assert report.wire_bytes == len(expected)

        table = tuning.TuningTable(values=[tuning.NoChange() for _ in range(128)])
        cases: list[
            tuple[tuning.BulkDumpRequest | tuning.BankedDumpRequest, messages.Message]
        ] = [
            (
                tuning.BulkDumpRequest(device_id=1, program=2),
                tuning.BulkDump(device_id=1, program=2, name="virtual", table=table),
            ),
            (
                tuning.BankedDumpRequest(device_id=1, bank=3, program=2),
                tuning.KeyBasedDump(
                    device_id=1, bank=3, program=2, name="virtual", table=table
                ),
            ),
            (
                tuning.BankedDumpRequest(device_id=1, bank=3, program=2),
                tuning.ScaleOctaveDump(
                    device_id=1,
                    bank=3,
                    program=2,
                    name="virtual",
                    offsets=tuning.ScaleOffsets(
                        resolution=7, values=[0 for _ in range(12)]
                    ),
                ),
            ),
        ]
        for index, (request, value) in enumerate(cases):
            wire = bytearray(codecs.encode(value))
            if index == 0:
                wire[-2] ^= 1  # Original dump checksum status must survive saving.
            reply = messages.SysEx(data=wire[1:-1])
            answer = worker.submit(respond, peer, peer_output, request, reply)
            query = tuning.Query(
                request=request, stream=receiver.stream, timeout_ns=3_000_000_000
            )
            result = tuning_live.run(query, receiver, output)
            answer.result(timeout=4)
            assert result.status == "completed", result.reason
            assert result.request_sent and not result.uncertain_send
            assert result.ignored_events == 2
            assert result.reply is not None and codecs.encode(result.reply) == wire
            assert result.decoded is not None
            assert result.decoded.checksum == (
                tuning.ChecksumStatus.LegacyMismatch
                if index == 0
                else tuning.ChecksumStatus.Matches
            )
            path = directory / f"query-{isolated}-{index}.syx"
            query.write(path)
            assert path.read_bytes() == wire

        silent_request = tuning.BulkDumpRequest(device_id=1, program=3)
        silent = tuning.Query(
            request=silent_request, stream=receiver.stream, timeout_ns=100_000_000
        )
        result = tuning_live.run(silent, receiver, output)
        assert (
            result.status == "timed_out"
            and result.request_sent
            and not result.uncertain_send
        )
        received = peer.receive(timeout=1)
        assert isinstance(received, events.Event)
        assert codecs.encode(received.message) == codecs.encode(silent_request)
        token = events.Cancellation()
        token.cancel()
        cancelled = tuning.Query(
            request=silent_request, stream=receiver.stream, timeout_ns=1_000_000_000
        )
        result = tuning_live.run(cancelled, receiver, output, cancel=token)
        assert result.status == "cancelled" and not result.request_sent
        assert peer.receive(timeout=0) is None
        assert receiver.dropped_packets == peer.dropped_packets == 0
        assert receiver.failure is peer.failure is None
    assert receiver.closed and peer.closed and output.closed and peer_output.closed
    print(
        f"native SysEx capture and three MTS query formats passed: isolated={isolated}",
        flush=True,
    )


def main() -> None:
    with TemporaryDirectory(prefix="meedee-sysex-loopback-") as directory:
        for isolated in (True, False):
            exercise(isolated, Path(directory))


if __name__ == "__main__":
    main()
