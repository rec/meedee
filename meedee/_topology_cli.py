"""Presentation and caller-thread polling over complete topology observations."""

from time import monotonic, sleep

from . import discovery, topology
from ._cli import Ports, WatchPorts, emit


def run_discovery(config: Ports) -> int:
    interrupted = False
    watching = isinstance(config, WatchPorts)
    with discovery.Poller(
        protocol=config.protocol,
        backend=config.backend,
        isolated=config.isolated,
        port_limit=config.port_limit,
        detail_limit=config.detail_limit,
        history_limit=config.history_limit if isinstance(config, WatchPorts) else 1,
    ) as poller:
        start = monotonic()
        try:
            while True:
                report = poller.poll()
                if not isinstance(config, WatchPorts) or report.stopped:
                    break
                if report.generation == 1:
                    emit(
                        {"kind": "topology_baseline"} | report_data(report),
                        config.output,
                    )
                elif (last := report.last) is not None and (last.added or last.removed):
                    emit(
                        {"kind": "topology_change", "port_count": report.port_count}
                        | delta_data(last),
                        config.output,
                    )
                remaining = (
                    None
                    if config.duration is None
                    else config.duration - (monotonic() - start)
                )
                if remaining is not None and remaining <= 0:
                    break
                sleep(
                    config.interval
                    if remaining is None
                    else min(config.interval, remaining)
                )
                if (
                    config.duration is not None
                    and monotonic() - start >= config.duration
                ):
                    break
        except KeyboardInterrupt:
            interrupted = True
        report = poller.snapshot()
    kind = (
        "topology_failure"
        if report.stopped
        else "topology_summary"
        if watching
        else "ports"
    )
    emit(
        {"kind": kind, "interrupted": interrupted} | report_data(report), config.output
    )
    return 2 if report.stopped else 0


def endpoint_data(endpoint: topology.Endpoint) -> dict[str, object]:
    return {
        n: getattr(endpoint, n)
        for n in ("backend", "protocol", "direction", "backend_id", "name")
    }


def delta_data(delta: topology.Delta) -> dict[str, object]:
    return {
        n: getattr(delta, n)
        for n in ("generation", "added", "removed", "omitted_changes")
    } | {
        "changes": [
            {"kind": c.kind, "count": c.count, "endpoint": endpoint_data(c.endpoint)}
            for c in delta.changes
        ],
    }


def report_data(report: topology.Report) -> dict[str, object]:
    return {
        n: getattr(report, n)
        for n in ("generation", "port_count", "omitted_history", "stopped", "reason")
    } | {
        "initialized": report.generation != 0,
        "ports": [
            {"endpoint": endpoint_data(p.endpoint), "count": p.count}
            for p in report.ports
        ],
        "last": None if report.last is None else delta_data(report.last),
        "history": [delta_data(d) for d in report.history],
    }
