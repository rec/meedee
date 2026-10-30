"""Management-only MIDI-CI browser presentation and explicit port lifecycle."""

from contextlib import ExitStack
from secrets import randbelow

from . import ci
from ._cli import CiBrowse, emit


def report_data(report: ci.BrowseReport) -> dict[str, object]:
    return {
        n: getattr(report, n)
        for n in (
            "status",
            "local_muid",
            "confirmed_actions",
            "uncertain_token",
            "pending_tokens",
            "omitted_diagnostics",
            "problems",
            "reason",
        )
    } | {
        "peers": [
            {
                "muid": p.muid,
                "version": p.version,
                "manufacturer": p.info.identity.manufacturer.hex(),
                "family": p.info.identity.family,
                "model": p.info.identity.model,
                "revision": p.info.identity.revision.hex(),
                "declared_capabilities": p.info.capabilities,
                "declared_max_sysex": p.info.max_sysex,
                "output_path": p.info.output_path,
                "function_block": p.function_block,
                "product_instance_id": p.product_instance_id,
            }
            for p in report.peers
        ],
        "diagnostics": [
            {n: getattr(d, n) for n in ("kind", "peer_muid", "token", "reason")}
            for d in report.diagnostics
        ],
    }


def run_browse(config: CiBrowse) -> int:
    from . import ci_live, midi
    from ._monitor import select_port

    identity = ci.Identity(
        manufacturer=bytes.fromhex(config.manufacturer),
        family=config.family,
        model=config.model,
        revision=bytes.fromhex(config.revision),
    )
    local_muid = randbelow(ci.MAX_MUID + 1)
    browser: ci.Browser | None = None
    failure: str | None = None
    interrupted = False
    try:
        protocol = (
            midi.Protocol.Midi1 if config.protocol == "midi1" else midi.Protocol.Ump
        )
        backend = (
            None if config.backend is None else getattr(midi.Backend, config.backend)
        )
        with ExitStack() as stack:
            output_client = midi.Output(
                protocol=protocol, backend=backend, isolated=config.isolated
            )
            input_client = midi.TypedInput(
                protocol=protocol,
                backend=backend,
                isolated=config.isolated,
                event_limit=config.event_limit,
                receive_byte_limit=config.byte_limit,
                sysex_limit=config.max_sysex,
            )
            # Resolve both names before opening either endpoint.
            output_port = select_port(
                output_client.ports(), config.output_port, direction="output"
            )
            input_port = select_port(input_client.ports(), config.input_port)
            output = stack.enter_context(output_client.connect(output_port))
            receiver = stack.enter_context(input_client.connect(input_port))
            browser = ci.Browser(
                local_muid=local_muid,
                identity=identity,
                route=ci.Route(
                    input=receiver.stream, output=config.output_port, group=config.group
                ),
                output_path=config.output_path,
                function_block=config.function_block,
                max_sysex=config.max_sysex,
                peer_limit=config.peer_limit,
                exchange_limit=config.exchange_limit,
                reply_byte_limit=config.byte_limit,
                discovery_ns=config.discovery_ms * 1_000_000,
                endpoint_ns=config.endpoint_ms * 1_000_000,
                timeout_ns=config.timeout_ms * 1_000_000,
                detail_limit=config.detail_limit,
            )
            ci_live.run(
                browser,
                receiver,
                output,
                message_limit=config.max_sysex,
                byte_limit=config.byte_limit,
            )
            if receiver.dropped_packets:
                failure = "input packet loss reported at scan shutdown"
            if (error := receiver.failure) is not None:
                raise error
    except KeyboardInterrupt:
        interrupted = True
        if browser is not None:
            browser.stop("interrupted during setup or shutdown")
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
        if browser is not None:
            browser.stop(failure)
    report = None if browser is None else browser.snapshot()
    data = (
        {"status": "stopped", "local_muid": local_muid, "peers": []}
        if report is None
        else report_data(report)
    )
    data.update(
        {
            "kind": "ci_browse_summary",
            "input_port": config.input_port,
            "output_port": config.output_port,
            "protocol": config.protocol,
            "group": config.group,
            "failure": failure,
            "interrupted": interrupted,
        }
    )
    emit(data, config.output)
    if failure is not None or report is None or report.uncertain_token is not None:
        return 2
    return int(interrupted or report.status != "completed" or report.problems > 0)
