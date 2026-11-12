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
    sysex,
    tuning,
)


def connect(name: str, run_in_subprocess: bool) -> midi.Output:
    client = midi.Output(run_in_subprocess=run_in_subprocess)
    matches = [p for p in client.ports() if p.name == name]
    assert len(matches) == 1, "test-owned endpoint was not discovered uniquely"
    return client.connect(matches[0])


def respond(
    receiver: midi.Input,
    output: midi.Output,
    request: tuning.BulkDumpRequest | tuning.BankedDumpRequest,
    reply: messages.SysEx,
) -> None:
    received = receiver.receive(timeout=3)
    assert isinstance(received, messages.Message), received
    assert codecs.encode(received) == codecs.encode(request)
    # Keep nonmatching traffic observable without pretending it is a reply.
    output.send(messages.TimingClock())
    output.send(messages.SysEx(data=b"\x7d\x01"))
    output.send(reply)
    assert receiver.dropped_packets == 0 and receiver.failure is None


def exercise(run_in_subprocess: bool, directory: Path) -> None:
    suffix = uuid4().hex
    names = [f"meedee-syx-{suffix}-{n}" for n in (1, 2)]
    with (
        midi.Input(run_in_subprocess=run_in_subprocess).create_virtual(
            names[0]
        ) as receiver,
        midi.Input(run_in_subprocess=run_in_subprocess).create_virtual(
            names[1]
        ) as peer,
        connect(names[1], run_in_subprocess) as output,
        connect(names[0], run_in_subprocess) as peer_output,
        ThreadPoolExecutor(max_workers=1) as worker,
    ):
        messages_to_capture = [
            messages.SysEx(data=b"\x7d\x01"),
            messages.SysEx(data=b"\x7d" + bytes(range(128)) * 32),
        ]
        with (
            midi.Input(run_in_subprocess=run_in_subprocess).create_virtual(
                names[0] + "-dump"
            ) as dump_input,
            connect(names[0] + "-dump", run_in_subprocess) as dump_output,
        ):
            delivery = worker.submit(
                dump_output.send, events.with_gap(iter(messages_to_capture), 0.001)
            )
            recording = dump_input.record(1)
        assert delivery.result(timeout=4).sent_events == 2
        captured = [m for m in recording if isinstance(m, messages.SysEx)]
        capture_path = directory / f"capture-{run_in_subprocess}.syx"
        sysex.write(capture_path, captured)
        expected = b"".join(codecs.encode(m) for m in messages_to_capture)
        assert capture_path.read_bytes() == expected

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
                request=request, stream=receiver.stream, timeout="3000000000 ns"
            )
            result = tuning.query(query, receiver, output)
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
            path = directory / f"query-{run_in_subprocess}-{index}.syx"
            query.write(path)
            assert path.read_bytes() == wire

        silent_request = tuning.BulkDumpRequest(device_id=1, program=3)
        silent = tuning.Query(
            request=silent_request, stream=receiver.stream, timeout="100000000 ns"
        )
        result = tuning.query(silent, receiver, output)
        assert (
            result.status == "timed_out"
            and result.request_sent
            and not result.uncertain_send
        )
        received = peer.receive(timeout=1)
        assert isinstance(received, messages.Message)
        assert codecs.encode(received) == codecs.encode(silent_request)
        token = events.Cancellation()
        token.cancel()
        cancelled = tuning.Query(
            request=silent_request, stream=receiver.stream, timeout="1000000000 ns"
        )
        result = tuning.query(cancelled, receiver, output, cancel=token)
        assert result.status == "cancelled" and not result.request_sent
        assert peer.receive(timeout=0) is None
        assert receiver.dropped_packets == peer.dropped_packets == 0
        assert receiver.failure is peer.failure is None
    assert receiver.closed and peer.closed and output.closed and peer_output.closed
    print(
        "native SysEx capture and three MTS query formats passed: "
        f"run_in_subprocess={run_in_subprocess}",
        flush=True,
    )


def main() -> None:
    with TemporaryDirectory(prefix="meedee-sysex-loopback-") as directory:
        for run_in_subprocess in (True, False):
            exercise(run_in_subprocess, Path(directory))


if __name__ == "__main__":
    main()
