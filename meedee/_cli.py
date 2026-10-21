"""Pydantic command definitions and presentation over the shared native APIs."""

import json
import sys
from pathlib import Path
from typing import Literal, Self

import tyro
from pydantic import BaseModel, Field, model_validator

from . import inspection, notes


class FileOptions(BaseModel, frozen=True):
    format: Literal["archive", "smf", "syx", "clip"]
    output: Literal["text", "json"] = "text"
    payload_limit: int = Field(default=1_048_576, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    preserve_unknown: bool = False


class Inspect(FileOptions, frozen=True):
    """Summarize file records, sources and timing diagnostics."""

    source: tyro.conf.Positional[Path]


class Validate(FileOptions, frozen=True):
    """Print a compact completeness and capture-quality verdict."""

    source: tyro.conf.Positional[Path]


class Compare(FileOptions, frozen=True):
    """Compare same-format records without automatic alignment."""

    left: tyro.conf.Positional[Path]
    right: tyro.conf.Positional[Path]
    source_map: dict[str, str] = Field(default_factory=dict)
    clock_map: dict[str, str] = Field(default_factory=dict)
    ignore_timestamps: bool = False
    tick_tolerance: int = Field(default=0, ge=0)

    @model_validator(mode="after")
    def check_timing_policy(self) -> Self:
        if self.ignore_timestamps and self.tick_tolerance:
            raise ValueError("tick tolerance cannot accompany ignored timestamps")
        return self


class Monitor(BaseModel, frozen=True):
    """Passively monitor one input selected by its exact, unique port name."""

    port: tyro.conf.Positional[str] = Field(min_length=1)
    protocol: Literal["midi1", "ump"]
    backend: (
        Literal[
            "CoreMidi",
            "CoreMidiUmp",
            "AlsaSeq",
            "AlsaSeqUmp",
            "AlsaRaw",
            "AlsaRawUmp",
            "WinMm",
            "WindowsMidiServices",
        ]
        | None
    ) = None
    output: Literal["text", "json"] = "text"
    isolated: bool = True
    metrics: bool = False
    duration: float | None = Field(default=None, gt=0, allow_inf_nan=False)
    types: list[str] = Field(default_factory=list)
    channel: int | None = Field(default=None, ge=1, le=16)
    group: int | None = Field(default=None, ge=1, le=16)
    history_limit: int = Field(default=128, gt=0)
    preview_bytes: int = Field(default=64, gt=0)
    event_limit: int = Field(default=128, gt=0)
    receive_byte_limit: int = Field(default=8_388_608, gt=0)
    sysex_limit: int = Field(default=1_048_576, gt=0)
    detail_limit: int = Field(default=128, gt=0)

    @model_validator(mode="after")
    def check_group(self) -> Self:
        if self.protocol == "midi1" and self.group is not None:
            raise ValueError("a group filter requires UMP protocol")
        return self


class RouteOptions(BaseModel, frozen=True):
    protocol: Literal["midi1", "ump"]
    destinations: list[str] = Field(min_length=1, max_length=128)
    channel: int | None = Field(default=None, ge=1, le=16)
    group: int | None = Field(default=None, ge=1, le=16)
    remap_channel: int | None = Field(default=None, ge=1, le=16)
    remap_group: int | None = Field(default=None, ge=1, le=16)
    include_nonchannel: bool = False
    event_limit: int = Field(default=128, gt=0)
    byte_limit: int = Field(default=8_388_608, gt=0)
    output: Literal["text", "json"] = "text"

    @model_validator(mode="after")
    def check_routes(self) -> Self:
        if any(not d for d in self.destinations) or len(set(self.destinations)) != len(
            self.destinations
        ):
            raise ValueError("destinations must be nonempty and distinct")
        if self.protocol == "midi1" and (
            self.group is not None or self.remap_group is not None
        ):
            raise ValueError("group selection/remapping requires UMP")
        return self


class RoutePreview(RouteOptions, frozen=True):
    """Preview one archive source routed to destination labels; opens no ports."""

    source: tyro.conf.Positional[Path]
    stream: str = Field(min_length=1)
    payload_limit: int = Field(default=1_048_576, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    preview_bytes: int = Field(default=64, gt=0)


class Patch(RouteOptions, frozen=True):
    """Forward one exact input to exact output names using fixed routes."""

    port: tyro.conf.Positional[str] = Field(min_length=1)
    backend: (
        Literal[
            "CoreMidi",
            "CoreMidiUmp",
            "AlsaSeq",
            "AlsaSeqUmp",
            "AlsaRaw",
            "AlsaRawUmp",
            "WinMm",
            "WindowsMidiServices",
        ]
        | None
    ) = None
    isolated: bool = True
    duration: float | None = Field(default=None, gt=0, allow_inf_nan=False)
    receive_byte_limit: int = Field(default=8_388_608, gt=0)
    sysex_limit: int = Field(default=1_048_576, gt=0)


class Ports(BaseModel, frozen=True):
    """List a complete input/output discovery snapshot, without opening endpoints."""

    protocol: Literal["midi1", "ump"]
    backend: (
        Literal[
            "CoreMidi",
            "CoreMidiUmp",
            "AlsaSeq",
            "AlsaSeqUmp",
            "AlsaRaw",
            "AlsaRawUmp",
            "WinMm",
            "WindowsMidiServices",
        ]
        | None
    ) = None
    isolated: bool = True
    port_limit: int = Field(default=256, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    output: Literal["text", "json"] = "text"


class WatchPorts(Ports, frozen=True):
    """Poll discovery; print the baseline and changes, with bounded history."""

    interval: float = Field(default=1.0, gt=0, allow_inf_nan=False)
    duration: float | None = Field(default=None, gt=0, allow_inf_nan=False)
    history_limit: int = Field(default=128, gt=0)


class TransferPreview(BaseModel, frozen=True):
    """Validate and preview a complete bounded MIDI 1 SysEx file; opens no ports."""

    source: tyro.conf.Positional[Path]
    gap_ms: int = Field(ge=0, le=18_446_744_073_709)
    message_limit: int = Field(default=256, gt=0)
    byte_limit: int = Field(default=8_388_608, gt=0)
    payload_limit: int = Field(default=1_048_576, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    preview_bytes: int = Field(default=64, gt=0)
    output: Literal["text", "json"] = "text"


class TransferSend(TransferPreview, frozen=True):
    """Send a validated SysEx file to one exact, unique MIDI 1 output name."""

    port: str = Field(min_length=1)
    backend: Literal["CoreMidi", "AlsaSeq", "AlsaRaw", "WinMm"] | None = None
    isolated: bool = True


class SysexReceive(BaseModel, frozen=True):
    """Passively capture an expected MIDI 1 SysEx count, then create a new file."""

    destination: tyro.conf.Positional[Path]
    port: str = Field(min_length=1)
    expected_messages: int = Field(gt=0)
    timeout_ms: int = Field(gt=0, le=18_446_744_073_709)
    byte_limit: int = Field(default=8_388_608, gt=0)
    payload_limit: int = Field(default=1_048_576, gt=0)
    receive_byte_limit: int = Field(default=8_388_608, gt=0)
    backend: Literal["CoreMidi", "AlsaSeq", "AlsaRaw", "WinMm"] | None = None
    isolated: bool = True
    output: Literal["text", "json"] = "text"

    @model_validator(mode="after")
    def check_capture(self) -> Self:
        from . import transfer

        transfer.Capture(
            stream="validation",
            expected_messages=self.expected_messages,
            timeout_ns=self.timeout_ms * 1_000_000,
            byte_limit=self.byte_limit,
            payload_limit=self.payload_limit,
        )
        return self


class TuningPreview(BaseModel, frozen=True):
    """Preview one unicast MIDI 1 tuning-dump request without opening ports."""

    device_id: int = Field(ge=0, le=126)
    program: int = Field(ge=0, le=127)
    bank: int | None = Field(default=None, ge=0, le=127)
    timeout_ms: int = Field(gt=0, le=18_446_744_073_709)
    output: Literal["text", "json"] = "text"


class TuningQuery(TuningPreview, frozen=True):
    """Request one tuning dump, then save the matched raw reply to a new file."""

    destination: tyro.conf.Positional[Path]
    input_port: str = Field(min_length=1)
    output_port: str = Field(min_length=1)
    backend: Literal["CoreMidi", "AlsaSeq", "AlsaRaw", "WinMm"] | None = None
    isolated: bool = True
    receive_byte_limit: int = Field(default=8_388_608, gt=0)
    payload_limit: int = Field(default=1_048_576, gt=0)


class UforPreview(BaseModel, frozen=True):
    """Resolve uFor Tuning JSON or Scala pitches offline; requires meedee[ufor]."""

    source: tyro.conf.Positional[Path]
    source_format: Literal["tuning-json", "scala"]
    root_note: int | None = Field(default=None, ge=0, le=127)
    root_frequency: str | None = Field(default=None, min_length=1)
    first_note: int = Field(default=0, ge=0, le=127)
    last_note: int = Field(default=127, ge=0, le=127)
    output: Literal["text", "json"] = "text"

    @model_validator(mode="after")
    def check_source(self) -> Self:
        if self.first_note > self.last_note:
            raise ValueError("first note must not exceed last note")
        if self.source_format == "scala":
            if self.root_note is None or self.root_frequency is None:
                raise ValueError("Scala requires explicit root note and root frequency")
        elif self.root_note is not None or self.root_frequency is not None:
            raise ValueError("Tuning JSON supplies its own reference settings")
        return self


class UforExport(UforPreview, frozen=True):
    """Create a new bulk-dump SYX file offline; sending is a separate command."""

    destination: tyro.conf.Positional[Path]
    device_id: int = Field(ge=0, le=126)
    program: int = Field(ge=0, le=127)
    name: str = Field(max_length=16, pattern=r"^[\x00-\x7f]*$")
    bank: int | None = Field(default=None, ge=0, le=127)
    preserve_unmapped: bool = False


class CiBrowse(BaseModel, frozen=True):
    """Run one CI management discovery scan, then inquire about format-2 endpoints."""

    input_port: str = Field(min_length=1)
    output_port: str = Field(min_length=1)
    protocol: Literal["midi1", "ump"]
    manufacturer: str
    family: int = Field(ge=0, le=16383)
    model: int = Field(ge=0, le=16383)
    revision: str
    group: int | None = Field(default=None, ge=1, le=16)
    output_path: int = Field(default=0, ge=0, le=127)
    function_block: int | None = Field(default=None, ge=0, le=31)
    backend: (
        Literal[
            "CoreMidi",
            "CoreMidiUmp",
            "AlsaSeq",
            "AlsaSeqUmp",
            "AlsaRaw",
            "AlsaRawUmp",
            "WinMm",
            "WindowsMidiServices",
        ]
        | None
    ) = None
    isolated: bool = True
    discovery_ms: int = Field(default=3000, ge=3000, le=18_446_744_073_709)
    endpoint_ms: int = Field(default=1000, gt=0, le=18_446_744_073_709)
    timeout_ms: int = Field(default=30000, gt=0, le=18_446_744_073_709)
    peer_limit: int = Field(default=16, gt=0)
    exchange_limit: int = Field(default=16, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    event_limit: int = Field(default=128, gt=0)
    max_sysex: int = Field(default=1_048_576, ge=128, le=268435455)
    byte_limit: int = Field(default=8_388_608, gt=0)
    output: Literal["text", "json"] = "text"

    @model_validator(mode="after")
    def check_scan(self) -> Self:
        from . import ci

        ci.Identity(
            manufacturer=bytes.fromhex(self.manufacturer),
            family=self.family,
            model=self.model,
            revision=bytes.fromhex(self.revision),
        )
        if (self.protocol == "ump") != (self.group is not None):
            raise ValueError("UMP requires an explicit group; MIDI 1 has no group")
        if self.protocol == "midi1" and self.function_block is not None:
            raise ValueError("MIDI 1 has no UMP Function Block")
        if self.timeout_ms <= self.discovery_ms:
            raise ValueError("overall timeout must exceed discovery duration")
        return self


def run(args: list[str] | None = None) -> int:
    try:
        command = tyro.cli(
            Inspect
            | Validate
            | Compare
            | Monitor
            | RoutePreview
            | Patch
            | Ports
            | WatchPorts
            | TransferPreview
            | TransferSend
            | SysexReceive
            | TuningPreview
            | TuningQuery
            | UforPreview
            | UforExport
            | CiBrowse,
            args=args,
            prog="meedee",
        )
        if isinstance(command, UforPreview):
            from ._ufor_cli import run_ufor

            return run_ufor(command)
        if isinstance(command, TuningPreview):
            from ._tuning_cli import run_query

            return run_query(command)
        if isinstance(command, CiBrowse):
            from ._ci_cli import run_browse

            return run_browse(command)
        if isinstance(command, SysexReceive):
            from ._transfer_cli import run_receive

            return run_receive(command)
        if isinstance(command, TransferPreview):
            from ._transfer_cli import run_transfer

            return run_transfer(command)
        if isinstance(command, Ports):
            from ._topology_cli import run_discovery

            return run_discovery(command)
        if isinstance(command, RoutePreview):
            from ._routing_cli import run_preview

            return run_preview(command)
        if isinstance(command, Patch):
            from ._patch import run_patch

            return run_patch(command)
        if isinstance(command, Monitor):
            from ._monitor import run_monitor

            return run_monitor(command)
        if isinstance(command, Compare):
            report = inspection.compare_files(
                command.left,
                command.right,
                format=command.format,
                source_map=command.source_map,
                clock_map=command.clock_map,
                ignore_timestamps=command.ignore_timestamps,
                tick_tolerance=command.tick_tolerance,
                payload_limit=command.payload_limit,
                detail_limit=command.detail_limit,
                preserve_unknown=command.preserve_unknown,
            )
            data: dict[str, object] = {
                n: getattr(report, n)
                for n in (
                    "equal",
                    "left_complete",
                    "right_complete",
                    "left_problem",
                    "right_problem",
                    "left_records",
                    "right_records",
                    "differences",
                )
            }
            data["examples"] = [
                {"record": d.record, "reason": d.reason} for d in report.examples
            ]
            status = (
                2
                if not report.left_complete or not report.right_complete
                else int(not report.equal)
            )
        else:
            report = inspection.inspect_file(
                command.source,
                format=command.format,
                payload_limit=command.payload_limit,
                detail_limit=command.detail_limit,
                preserve_unknown=command.preserve_unknown,
            )
            quality = {
                n: getattr(report, n)
                for n in (
                    "gap_records",
                    "lost_events",
                    "discontinuities",
                    "timestamp_regressions",
                    "opaque_records",
                )
            }
            data = {
                "complete": report.complete,
                "problem": report.problem,
                "quality_problems": quality,
            }
            if isinstance(command, Inspect):
                data.update(inspection_data(report))
            status = 2 if not report.complete else int(any(quality.values()))
        emit(data, command.output)
        return status
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 2


def inspection_data(report: inspection.Report) -> dict[str, object]:
    data: dict[str, object] = {
        n: getattr(report, n)
        for n in (
            "complete",
            "problem",
            "records",
            "message_counts",
            "source_counts",
            "track_records",
            "gap_records",
            "lost_events",
            "discontinuities",
            "untimed_events",
            "opaque_records",
            "timestamp_regressions",
            "omitted_segments",
            "omitted_diagnostics",
        )
    }
    data["clocks"] = [
        {
            "identity": c.identity,
            "tick_numerator": c.tick_numerator,
            "tick_denominator": c.tick_denominator,
            "origin": c.origin,
            "resolution_ticks": c.resolution_ticks,
            "backend": None if c.backend is None else str(c.backend),
            "mode": None if c.mode is None else str(c.mode),
        }
        for c in report.clocks
    ]
    data["streams"] = [
        {"id": s.id, "label": s.label, "clock_identity": s.clock_identity}
        for s in report.streams
    ]
    header = report.smf_header
    data["smf_header"] = (
        None
        if header is None
        else {
            "format": header.format,
            "tracks": header.tracks,
            "division": header.division,
            "extra": header.extra.hex(),
        }
    )
    data["timing_segments"] = [
        {
            n: getattr(s, n)
            for n in (
                "source",
                "clock_identity",
                "number",
                "events",
                "untimed",
                "first",
                "last",
                "minimum",
                "maximum",
                "regressions",
            )
        }
        for s in report.timing_segments
    ]
    data["timing_diagnostics"] = [
        {"source": d.source, "record": d.record, "kind": d.kind}
        for d in report.timing_diagnostics
    ]
    data["note_state"] = note_state_data(report.note_state)
    return data


def note_state_data(report: notes.Report) -> dict[str, object]:
    return {
        "boundaries": report.boundaries,
        "omitted_events": report.omitted_events,
        "channels": [channel_state_data(c) for c in report.channels],
    }


def channel_state_data(channel: notes.ChannelState) -> dict[str, object]:
    return {
        n: getattr(channel, n)
        for n in (
            "source",
            "group",
            "channel",
            "protocol",
            "sustain",
            "sostenuto",
            "uncertain",
            "unmatched_releases",
        )
    } | {
        "notes": [
            {"note": n.note, "held": n.held, "released": n.released}
            for n in channel.notes
        ]
    }


def emit(data: dict[str, object], output: Literal["text", "json"]) -> None:
    if output == "json":
        print(json.dumps(data, sort_keys=True, allow_nan=False))
    else:
        for k, v in data.items():
            print(f"{k}: {json.dumps(v, sort_keys=True, allow_nan=False)}")
