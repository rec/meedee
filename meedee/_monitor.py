"""Passive CLI presentation over the existing typed input and Rust interpreters."""

from collections import deque
from time import monotonic

from . import controls, events, inspection, messages, midi, ump
from ._cli import Monitor, channel_state_data, emit, inspection_data


class Session:
    def __init__(self, config: Monitor, stream: str, clock: events.Clock) -> None:
        self.config = config
        self.clock = clock
        self.observer = inspection.Observer(
            stream=stream, clock=clock, detail_limit=config.detail_limit
        )
        self.history: deque[dict[str, object]] = deque(maxlen=config.history_limit)
        self.views = (
            [
                controls.EventInterpreter(group=config.group),
                controls.EventVelocityInterpreter(group=config.group),
            ]
            if config.protocol == "midi1" or config.group is not None
            else []
        )
        self.filtered = 0
        self.diagnostics = 0
        self.transport_drops = 0

    def feed(
        self, item: events.Event | events.StreamDiagnostic
    ) -> list[dict[str, object]]:
        self.observer.feed(item)
        if isinstance(item, events.StreamDiagnostic):
            for v in self.views:
                v.reset()
            self.diagnostics += 1
            row: dict[str, object] = {
                "kind": "diagnostic",
                "stream": item.stream,
                "packet": item.packet,
                "diagnostic": item.kind,
                "lost": item.lost,
                "reason": item.reason[: self.config.preview_bytes * 2],
            }
        else:
            observations = [o for v in self.views for o in v.feed(item)]
            message = item.message
            inner = message.message if isinstance(message, ump.Midi1Packet) else message
            channel = getattr(inner, "channel", None)
            group = getattr(message, "group", None)
            if (
                (
                    self.config.types
                    and type(message).__name__ not in self.config.types
                    and type(inner).__name__ not in self.config.types
                )
                or (self.config.channel is not None and channel != self.config.channel)
                or (self.config.group is not None and group != self.config.group)
            ):
                self.filtered += 1
                return []
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
                    for o in observations
                ],
                "note_state": [
                    channel_state_data(c)
                    for c in self.observer.channel_states(channel=channel, group=group)
                ]
                if channel is not None
                else [],
            }
        self.history.append(row)
        return [row]

    def counter(self, total: int) -> list[dict[str, object]]:
        if total <= self.transport_drops:
            return []
        self.transport_drops = total
        self.observer.loss_counter_changed()
        for v in self.views:
            v.reset()
        row: dict[str, object] = {
            "kind": "loss_counter",
            "transport_drop_total": total,
            "reason": "asynchronous counter; may overlap sequence-gap diagnostics",
        }
        self.history.append(row)
        return [row]


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
    protocol = midi.Protocol.Midi1 if config.protocol == "midi1" else midi.Protocol.Ump
    backend = None if config.backend is None else getattr(midi.Backend, config.backend)
    try:
        client = midi.TypedInput(
            protocol=protocol,
            backend=backend,
            isolated=config.isolated,
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


def watch(connection: midi.TypedInputConnection, config: Monitor) -> int:
    session = Session(config, connection.stream, connection.clock)
    start = monotonic()
    interrupted = False
    failure: str | None = None
    try:
        with connection:
            try:
                while config.duration is None or monotonic() - start < config.duration:
                    for r in session.counter(connection.dropped_packets):
                        emit(r, config.output)
                    if (error := connection.failure) is not None:
                        raise error
                    remaining = (
                        0.05
                        if config.duration is None
                        else min(
                            0.05, max(0.0, config.duration - (monotonic() - start))
                        )
                    )
                    item = connection.receive(timeout=remaining)
                    if item is not None:
                        for r in session.feed(item):
                            emit(r, config.output)
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
        ExceptionGroup,
    ) as error:
        failure = str(error)[: config.preview_bytes * 2]
        emit({"kind": "failure", "reason": failure}, config.output)
    for r in session.counter(connection.dropped_packets):
        emit(r, config.output)
    report = session.observer.snapshot()
    data = inspection_data(report)
    data.pop("complete")
    data.pop("problem")
    data.update(
        {
            "kind": "summary",
            "live": True,
            "interrupted": interrupted,
            "failure": failure,
            "filtered": session.filtered,
            "transport_drop_total": session.transport_drops,
            "history": list(session.history),
        }
    )
    emit(data, config.output)
    return (
        2
        if failure is not None
        else int(
            bool(
                session.transport_drops
                or session.diagnostics
                or report.timestamp_regressions
            )
        )
    )
