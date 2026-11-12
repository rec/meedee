"""Passive CLI presentation over the existing typed input and Rust interpreters."""

from collections import deque

from . import events, inspection, messages, midi, ump
from ._cli import Monitor, channel_state_data, emit, inspection_data


class Session:
    def __init__(self, config: Monitor, stream: str, clock: events.Clock) -> None:
        self.config = config
        self.clock = clock
        self.monitor = inspection.Monitor(
            stream=stream,
            clock=clock,
            encoding=config.encoding,
            types=config.types,
            channel=config.channel,
            group=config.group,
            detail_limit=config.detail_limit,
        )
        self.history: deque[dict[str, object]] = deque(maxlen=config.history_limit)

    def present(self, entry: inspection.MonitorEntry) -> None:
        row: dict[str, object]
        if (diagnostic := entry.diagnostic) is not None:
            row = {
                "kind": "diagnostic",
                "stream": diagnostic.stream,
                "packet": diagnostic.packet,
                "diagnostic": diagnostic.kind,
                "lost": diagnostic.lost,
                "reason": diagnostic.reason[: self.config.preview_bytes * 2],
            }
        elif (item := entry.message) is not None:
            message = item
            inner = message
            channel = getattr(inner, "channel", None)
            group = getattr(message, "group", None)
            if isinstance(inner, messages.SysEx):
                payload = inner.data
                preview = (
                    f"SysEx({payload[: self.config.preview_bytes].hex()}; "
                    f"{len(payload)} bytes)"
                )
            else:
                preview = repr(message)[: self.config.preview_bytes * 2]
            row = {
                "kind": "event",
                "stream": item.stream,
                "clock": self.clock.identity
                if item.timestamp is None
                else item.timestamp.clock.identity,
                "ticks": None if item.timestamp is None else item.timestamp.ticks,
                "type": type(message).__name__,
                "channel": channel,
                "group": group,
                "preview": preview,
                "observations": [
                    {
                        "type": type(o.value).__name__,
                        "preview": repr(o.value)[: self.config.preview_bytes * 2],
                    }
                    for o in entry.observations
                ],
                "note_state": [channel_state_data(c) for c in entry.note_state]
                if channel is not None
                else [],
            }
        else:
            row = {
                "kind": "loss_counter",
                "transport_drop_total": entry.transport_drop_total,
                "reason": "asynchronous counter; may overlap sequence-gap diagnostics",
            }
        self.history.append(row)
        emit(row, self.config.output)


def select_port(
    ports: list[midi.Port], name: str, *, direction: str = "input"
) -> midi.Port:
    matches = [p for p in ports if p.name == name]
    if len(matches) != 1:
        raise ValueError(
            f"expected exactly one {direction} named {name!r}; found {len(matches)}"
        )
    return matches[0]


def run_monitor(config: Monitor) -> int:
    known = {
        n
        for m in (messages, ump)
        for n, v in vars(m).items()
        if isinstance(v, type)
        and issubclass(v, messages.Message)
        and v is not messages.Message
    }
    if any(t not in known for t in config.types):
        raise ValueError("--types must name concrete MIDI message classes")
    encoding = midi.Encoding.Bytes if config.encoding == "bytes" else midi.Encoding.Ump
    backend = None if config.backend is None else getattr(midi.Backend, config.backend)
    try:
        client = midi.Input(
            encoding=encoding,
            backend=backend,
            run_in_subprocess=config.run_in_subprocess,
            subprocess_worker=config.subprocess_worker,
            subprocess_executable=config.subprocess_executable,
            receive_byte_limit=config.receive_byte_limit,
            event_limit=config.event_limit,
            sysex_limit=config.sysex_limit,
        )
        port = select_port(client.ports(), config.port)
        return watch(client.connect(port), config)
    except (
        midi.MidiError,
        midi.StateError,
        midi.CallbackThreadError,
        midi.NativePanicError,
        midi.ResourceError,
        midi.WorkerError,
        midi.WorkerTimeoutError,
    ) as error:
        emit(
            {"kind": "failure", "reason": str(error)[: config.preview_bytes * 2]},
            config.output,
        )
        return 2


def watch(connection: midi.Input, config: Monitor) -> int:
    session = Session(config, connection.stream, connection.clock)
    failure: str | None = None
    try:
        with connection:
            inspection.run(
                session.monitor,
                connection,
                callback=session.present,
                duration=config.duration,
                metrics=config.metrics,
            )
    except (
        midi.MidiError,
        midi.StateError,
        midi.CallbackThreadError,
        midi.NativePanicError,
        midi.ResourceError,
        midi.WorkerError,
        midi.WorkerTimeoutError,
        ExceptionGroup,
    ) as error:
        failure = str(error)[: config.preview_bytes * 2]
        emit({"kind": "failure", "reason": failure}, config.output)
    if (entry := session.monitor.counter(connection.dropped_packets)) is not None:
        session.present(entry)
    summary = session.monitor.snapshot()
    report = summary.inspection
    data = inspection_data(report)
    data.pop("complete")
    data.pop("problem")
    data.update(
        {
            "kind": "summary",
            "live": True,
            "interrupted": summary.interrupted,
            "failure": failure,
            "filtered": summary.filtered,
            "transport_drop_total": summary.transport_drops,
            "history": list(session.history),
        }
    )
    if (measurement := summary.delivery) is not None:
        intervals = measurement.intervals
        data["metrics"] = {
            "clock": "host_monotonic",
            "events": measurement.events,
            "elapsed_ns": measurement.elapsed_ns,
            "events_per_second": (
                measurement.events * 1_000_000_000 / measurement.elapsed_ns
                if measurement.elapsed_ns
                else None
            ),
            "delivery_intervals": {
                n: getattr(intervals, n)
                for n in (
                    "count",
                    "sum_ns",
                    "minimum_ns",
                    "maximum_ns",
                    "upper_bounds_ns",
                    "buckets",
                )
            },
        }
    emit(data, config.output)
    return (
        2
        if failure is not None
        else int(
            bool(
                summary.transport_drops
                or summary.diagnostics
                or report.timestamp_regressions
            )
        )
    )
