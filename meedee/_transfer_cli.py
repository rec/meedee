"""Explicit SysEx preview/send commands; no transport imports for preview."""

from . import transfer
from ._cli import TransferPreview, TransferSend, emit


def report_data(report: transfer.Report) -> dict[str, object]:
    return {
        n: getattr(report, n)
        for n in (
            "status",
            "total_messages",
            "total_bytes",
            "confirmed_messages",
            "confirmed_bytes",
            "remaining_messages",
            "in_flight",
            "uncertain_message",
            "next_ready_ns",
            "reason",
        )
    }


def run_transfer(config: TransferPreview) -> int:
    plan = transfer.read_file(
        config.source,
        gap_ns=config.gap_ms * 1_000_000,
        message_limit=config.message_limit,
        byte_limit=config.byte_limit,
        payload_limit=config.payload_limit,
    )
    if not isinstance(config, TransferSend):
        count = min(plan.message_count, config.detail_limit)
        emit(
            {
                "kind": "transfer_preview",
                "dry_run": True,
                "message_count": plan.message_count,
                "wire_bytes": plan.wire_bytes,
                "gap_ns": plan.gap_ns,
                "examples": [
                    {
                        "index": i,
                        "wire_bytes": len(plan.message(i).data) + 2,
                        "payload_preview": plan.message(i)
                        .data[: config.preview_bytes]
                        .hex(),
                    }
                    for i in range(count)
                ],
                "omitted_examples": plan.message_count - count,
            },
            config.output,
        )
        return 0

    from . import midi, transfer_output
    from ._monitor import select_port

    session = transfer.Session(plan)
    failure: str | None = None
    interrupted = False
    try:
        backend = (
            None if config.backend is None else getattr(midi.Backend, config.backend)
        )
        client = midi.TypedOutput(
            protocol=midi.Protocol.Midi1, backend=backend, isolated=config.isolated
        )
        port = select_port(client.ports(), config.port, direction="output")
        with client.connect(port) as output:
            transfer_output.run(session, output)
    except KeyboardInterrupt:
        interrupted = True
        session.cancel("interrupted during setup or close")
    except (
        midi.MidiError,
        midi.StateError,
        midi.CallbackThreadError,
        midi.NativePanicError,
        midi.ResourceError,
        midi.WorkerError,
        midi.WorkerTimeoutError,
        OSError,
        ValueError,
        ExceptionGroup,
    ) as error:
        failure = str(error)[:1024]
        session.cancel(failure)
    report = session.snapshot()
    data = report_data(report)
    data.update(
        {
            "kind": "transfer_summary",
            "dry_run": False,
            "port": config.port,
            "gap_ns": plan.gap_ns,
            "failure": failure,
            "interrupted": interrupted or report.reason == "interrupted",
        }
    )
    emit(data, config.output)
    if (
        failure is not None
        or report.status == "failed"
        or report.uncertain_message is not None
    ):
        return 2
    return 0 if report.status == "completed" and not interrupted else 1
