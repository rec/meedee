"""Offline routing presentation. Importing this module starts no transport."""

from . import messages, routing
from ._cli import RouteOptions, RoutePreview, emit


def configured_routes(config: RouteOptions, stream: str) -> list[routing.Route]:
    return [
        routing.Route(
            name=d,
            source=stream,
            destination=d,
            protocol=config.protocol,
            channel=config.channel,
            group=config.group,
            remap_channel=config.remap_channel,
            remap_group=config.remap_group,
            include_nonchannel=config.include_nonchannel,
        )
        for d in config.destinations
    ]


def routing_data(report: routing.Report, *, preview: bool = False) -> dict[str, object]:
    confirmed = "planned" if preview else "confirmed"
    return {
        n: getattr(report, n)
        for n in (
            "received",
            "filtered",
            "admitted",
            "capacity_rejections",
            "pending",
            "queued_bytes",
            "stopped",
            "reason",
            "uncertain_delivery",
            "uncertain_route",
            "uncertain_destination",
        )
    } | {
        confirmed: report.confirmed,
        "routes": {
            n: {
                "admitted": s["admitted"],
                confirmed: s["confirmed"],
                "capacity_rejections": s["capacity_rejections"],
            }
            for n, s in report.routes.items()
        },
    }


def run_preview(config: RoutePreview) -> int:
    report = routing.preview_file(
        config.source,
        routes=configured_routes(config, config.stream),
        event_limit=config.event_limit,
        byte_limit=config.byte_limit,
        payload_limit=config.payload_limit,
        detail_limit=config.detail_limit,
    )
    data = routing_data(report.routing, preview=True)
    data.update(
        {
            "dry_run": True,
            "complete": report.complete,
            "problem": report.problem,
            "records": report.records,
            "omitted_examples": report.omitted_examples,
            "examples": [
                delivery_data(d, config.preview_bytes) for d in report.examples
            ],
        }
    )
    emit(data, config.output)
    return 2 if not report.complete else int(report.routing.stopped)


def delivery_data(delivery: routing.Delivery, preview_bytes: int) -> dict[str, object]:
    event = delivery.event
    message = event.message
    preview = (
        f"SysEx({message.data[:preview_bytes].hex()}; {len(message.data)} bytes)"
        if isinstance(message, messages.SysEx)
        else repr(message)[: preview_bytes * 2]
    )
    return {
        "route": delivery.route,
        "destination": delivery.destination,
        "stream": event.stream,
        "ticks": None if event.timestamp is None else event.timestamp.ticks,
        "clock": None if event.timestamp is None else event.timestamp.clock.identity,
        "preview": preview,
    }
