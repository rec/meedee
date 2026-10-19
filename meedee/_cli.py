"""Pydantic command definitions and presentation over the shared native APIs."""

import json
import sys
from pathlib import Path
from typing import Literal, Self

import tyro
from pydantic import BaseModel, Field, model_validator

from . import inspection


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


def run(args: list[str] | None = None) -> int:
    try:
        command = tyro.cli(
            Inspect | Validate | Compare | Monitor, args=args, prog="meedee"
        )
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
    return data


def emit(data: dict[str, object], output: Literal["text", "json"]) -> None:
    if output == "json":
        print(json.dumps(data, sort_keys=True, allow_nan=False))
    else:
        for k, v in data.items():
            print(f"{k}: {json.dumps(v, sort_keys=True, allow_nan=False)}")
