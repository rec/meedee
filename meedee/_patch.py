"""Static patch-bay host over typed connections and the shared Rust router."""

from contextlib import ExitStack
from time import monotonic

from . import midi, routing
from ._cli import Patch, emit
from ._monitor import select_port
from ._routing_cli import configured_routes, routing_data


def run_patch(config: Patch) -> int:
    protocol = midi.Protocol.Midi1 if config.protocol == "midi1" else midi.Protocol.Ump
    backend = None if config.backend is None else getattr(midi.Backend, config.backend)
    router: routing.Router | None = None
    connection: midi.TypedInputConnection | None = None
    interrupted = False
    failure: str | None = None
    data: dict[str, object]
    try:
        with ExitStack() as stack:
            outputs: dict[str, midi.OutputConnection] = {}
            for name in config.destinations:
                client = midi.Output(
                    protocol=protocol, backend=backend, isolated=config.isolated
                )
                port = select_port(client.ports(), name, direction="output")
                outputs[name] = stack.enter_context(client.connect(port))
            client_input = midi.TypedInput(
                protocol=protocol,
                backend=backend,
                isolated=config.isolated,
                event_limit=config.event_limit,
                receive_byte_limit=config.receive_byte_limit,
                sysex_limit=config.sysex_limit,
            )
            port = select_port(client_input.ports(), config.port)
            connection = stack.enter_context(client_input.connect(port))
            router = routing.Router(
                routes=configured_routes(config, connection.stream),
                event_limit=config.event_limit,
                byte_limit=config.byte_limit,
            )
            forward(connection, outputs, router, config.duration)
    except KeyboardInterrupt:
        interrupted = True
        if router is not None and router.snapshot().pending:
            router.stop("interrupted with pending deliveries")
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
        if router is not None:
            router.stop(failure)
    drops = 0 if connection is None else connection.dropped_packets
    if router is not None:
        if drops:
            router.stop("input packet loss reported by transport counter")
        report = router.snapshot()
        data = routing_data(report)
        status = (
            2
            if failure is not None or report.uncertain_delivery is not None
            else int(report.stopped)
        )
    else:
        data = {"stopped": True, "reason": failure or "interrupted during setup"}
        status = 0 if interrupted else 2
    data.update(
        {
            "kind": "routing_summary",
            "dry_run": False,
            "interrupted": interrupted,
            "failure": failure,
            "input_drop_total": drops,
        }
    )
    emit(data, config.output)
    return status


def forward(
    connection: midi.TypedInputConnection,
    outputs: dict[str, midi.OutputConnection],
    router: routing.Router,
    duration: float | None,
) -> None:
    start = monotonic()
    while duration is None or monotonic() - start < duration:
        if connection.dropped_packets:
            router.stop("input packet loss reported by transport counter")
            return
        if (error := connection.failure) is not None:
            raise error
        remaining = (
            0.05
            if duration is None
            else min(0.05, max(0.0, duration - (monotonic() - start)))
        )
        item = connection.receive(timeout=remaining)
        if item is None:
            continue
        try:
            router.feed(item)
        except ValueError:
            if router.snapshot().stopped:
                return
            raise
        while (delivery := router.peek()) is not None:
            if connection.dropped_packets:
                router.stop("input packet loss reported by transport counter")
                return
            if (error := connection.failure) is not None:
                raise error
            try:
                outputs[delivery.destination].send(delivery.event.message)
                router.confirm(delivery.id)
            except KeyboardInterrupt:
                if (pending := router.peek()) is not None and pending.id == delivery.id:
                    router.fail(
                        delivery.id, "send interrupted; delivery may have taken effect"
                    )
                raise
            except (
                midi.MidiError,
                midi.StateError,
                midi.CallbackThreadError,
                midi.NativePanicError,
                midi.ResourceError,
                midi.WorkerError,
                midi.WorkerTimeoutError,
            ) as error:
                router.fail(delivery.id, str(error))
                raise
        if router.snapshot().stopped:
            return
