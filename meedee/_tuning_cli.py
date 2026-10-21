"""Offline request preview and explicit MTS dump-query port lifecycle."""

from contextlib import ExitStack

from . import codecs, tuning
from ._cli import TuningPreview, TuningQuery, emit


def report_data(report: tuning.QueryReport) -> dict[str, object]:
    decoded = report.decoded
    reply = report.reply
    return {
        n: getattr(report, n)
        for n in (
            "status",
            "request_sent",
            "uncertain_send",
            "ignored_events",
            "deadline_ns",
            "reason",
        )
    } | {
        "reply_bytes": 0 if reply is None else len(reply.data) + 2,
        "reply_type": None if decoded is None else type(decoded.message).__name__,
        "checksum": None if decoded is None else str(decoded.checksum),
    }


def run_query(config: TuningPreview) -> int:
    request: tuning.BulkDumpRequest | tuning.BankedDumpRequest
    if config.bank is None:
        request = tuning.BulkDumpRequest(
            device_id=config.device_id, program=config.program
        )
    else:
        request = tuning.BankedDumpRequest(
            device_id=config.device_id, bank=config.bank, program=config.program
        )
    if not isinstance(config, TuningQuery):
        emit(
            {
                "kind": "tuning_preview",
                "dry_run": True,
                "request_type": type(request).__name__,
                "wire_hex": codecs.encode(request).hex(),
                "timeout_ns": config.timeout_ms * 1_000_000,
            },
            config.output,
        )
        return 0

    from . import midi, tuning_live
    from ._monitor import select_port

    query: tuning.Query | None = None
    failure: str | None = None
    interrupted = False
    exported = False
    try:
        if config.destination.exists():
            raise FileExistsError("query destination already exists")
        backend = (
            None if config.backend is None else getattr(midi.Backend, config.backend)
        )
        with ExitStack() as stack:
            output_client = midi.TypedOutput(
                protocol=midi.Protocol.Midi1, backend=backend, isolated=config.isolated
            )
            input_client = midi.TypedInput(
                protocol=midi.Protocol.Midi1,
                backend=backend,
                isolated=config.isolated,
                receive_byte_limit=config.receive_byte_limit,
                sysex_limit=config.payload_limit,
            )
            output_port = select_port(
                output_client.ports(), config.output_port, direction="output"
            )
            input_port = select_port(input_client.ports(), config.input_port)
            output = stack.enter_context(output_client.connect(output_port))
            receiver = stack.enter_context(input_client.connect(input_port))
            query = tuning.Query(
                request=request,
                stream=receiver.stream,
                timeout_ns=config.timeout_ms * 1_000_000,
            )
            tuning_live.run(query, receiver, output)
        if query.snapshot().status == "completed":
            query.write(config.destination)
            exported = True
    except KeyboardInterrupt:
        interrupted = True
        if query is not None:
            query.cancel("interrupted during setup, close or export")
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
        if query is not None:
            query.fail(failure)
    data: dict[str, object] = {
        "kind": "tuning_query_summary",
        "dry_run": False,
        "input_port": config.input_port,
        "output_port": config.output_port,
        "destination": str(config.destination),
        "exported": exported,
        "failure": failure,
        "interrupted": interrupted,
    }
    if query is not None:
        report = query.snapshot()
        data.update(report_data(report))
        interrupted = interrupted or report.reason == "interrupted"
        data["interrupted"] = interrupted
    emit(data, config.output)
    if failure is not None:
        return 2
    if query is None:
        return 1 if interrupted else 2
    report = query.snapshot()
    if exported:
        decoded = report.decoded
        assert decoded is not None
        return 1 if decoded.checksum == tuning.ChecksumStatus.LegacyMismatch else 0
    return 1 if interrupted or report.status in ("cancelled", "timed_out") else 2
