"""SysEx preview, paced sending and duration-based recording commands."""

from . import archive, messages, sysex
from ._cli import SysexReceive, TransferPreview, TransferSend, emit


def run_transfer(config: TransferPreview) -> int:
    values: list[messages.SysEx] = []
    wire_bytes = 0
    reader = sysex.Reader(config.source, payload_limit=config.payload_limit)
    try:
        for m in reader:
            wire_bytes += len(m.data) + 2
            if len(values) == config.message_limit or wire_bytes > config.byte_limit:
                raise ValueError("transfer message or byte limit exceeded")
            values.append(m)
    finally:
        reader.close()
    if not values:
        raise ValueError("transfer requires at least one SysEx message")
    if not isinstance(config, TransferSend):
        count = min(len(values), config.detail_limit)
        emit(
            {
                "kind": "transfer_preview",
                "dry_run": True,
                "message_count": len(values),
                "wire_bytes": wire_bytes,
                "gap": config.gap,
                "examples": [
                    {
                        "index": i,
                        "wire_bytes": len(m.data) + 2,
                        "payload_preview": m.data[: config.preview_bytes].hex(),
                    }
                    for i, m in enumerate(values[:count])
                ],
                "omitted_examples": len(values) - count,
            },
            config.output,
        )
        return 0

    from . import midi
    from ._monitor import select_port

    failure: str | None = None
    interrupted = False
    confirmed = 0
    uncertain = None
    completed = False
    try:
        client = midi.Output(
            protocol=midi.Protocol.Midi1,
            backend=None
            if config.backend is None
            else getattr(midi.Backend, config.backend),
            run_in_subprocess=config.run_in_subprocess,
            subprocess_worker=config.subprocess_worker,
            subprocess_executable=config.subprocess_executable,
        )
        port = select_port(client.ports(), config.port, direction="output")
        with client.connect(port) as output:
            report = sysex.send(output, values, gap=config.gap)
            confirmed = report.sent_events
            completed = not report.cancelled
    except KeyboardInterrupt as error:
        confirmed = getattr(error, "completed_events", confirmed)
        interrupted = True
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
        confirmed = getattr(error, "completed_events", confirmed)
        if hasattr(error, "completed_events") and isinstance(
            error, (midi.SendError, midi.WorkerError, midi.WorkerTimeoutError)
        ):
            uncertain = confirmed
        failure = str(error)[:1024]
    emit(
        {
            "kind": "transfer_summary",
            "dry_run": False,
            "port": config.port,
            "gap": config.gap,
            "failure": failure,
            "interrupted": interrupted,
            "status": "completed"
            if completed
            else "failed"
            if failure
            else "cancelled",
            "total_messages": len(values),
            "total_bytes": wire_bytes,
            "confirmed_messages": confirmed,
            "confirmed_bytes": sum(len(m.data) + 2 for m in values[:confirmed]),
            "remaining_messages": len(values) - confirmed,
            "uncertain_message": uncertain,
        },
        config.output,
    )
    return 2 if failure else 0 if completed and not interrupted else 1


def run_receive(config: SysexReceive) -> int:
    from . import midi
    from ._monitor import select_port

    failure: str | None = None
    interrupted = False
    exported = False
    values: list[messages.SysEx] = []
    ignored = 0
    wire_bytes = 0
    try:
        if config.destination.exists():
            raise FileExistsError("capture destination already exists")
        client = midi.Input(
            protocol=midi.Protocol.Midi1,
            backend=None
            if config.backend is None
            else getattr(midi.Backend, config.backend),
            run_in_subprocess=config.run_in_subprocess,
            subprocess_worker=config.subprocess_worker,
            subprocess_executable=config.subprocess_executable,
            receive_byte_limit=config.receive_byte_limit,
            sysex_limit=config.payload_limit,
        )
        port = select_port(client.ports(), config.port)
        with client.connect(port) as receiver:
            records = receiver.record(config.duration)
        for r in records:
            if isinstance(r, (archive.Gap, archive.Discontinuity)):
                raise ValueError(f"incomplete recording: {r}")
            if isinstance(r, messages.SysEx):
                wire_bytes += len(r.data) + 2
                if wire_bytes > config.byte_limit:
                    raise ValueError("SysEx export byte limit exceeded")
                values.append(r)
            elif isinstance(r, messages.Message):
                ignored += 1
        if not values:
            raise ValueError("recording contained no SysEx messages")
        with config.destination.open("xb") as destination:
            sysex.write(destination, values, payload_limit=config.payload_limit)
        exported = True
    except KeyboardInterrupt:
        interrupted = True
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
    emit(
        {
            "kind": "sysex_receive_summary",
            "port": config.port,
            "destination": str(config.destination),
            "duration": config.duration,
            "exported": exported,
            "failure": failure,
            "interrupted": interrupted,
            "status": "completed"
            if exported
            else "cancelled"
            if interrupted
            else "failed",
            "received_messages": len(values),
            "wire_bytes": wire_bytes,
            "ignored_events": ignored,
        },
        config.output,
    )
    return 0 if exported else 1 if interrupted else 2
