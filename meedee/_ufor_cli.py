"""Optional offline uFor source resolution and bulk-dump export."""

from __future__ import annotations

import sys

from . import syx
from ._cli import UforExport, UforPreview, emit


def run_ufor(config: UforPreview) -> int:
    if sys.version_info < (3, 13):
        raise ValueError(
            "uFor tuning commands require Python 3.13+ and meedee[cli,ufor]"
        )
    try:
        from ufor import scala
        from ufor.tuning import Tuning
    except ModuleNotFoundError as error:
        if error.name != "ufor":
            raise
        raise ValueError("Install meedee[cli,ufor] for uFor tuning commands") from error
    from . import ufor_tuning

    text = config.source.read_text()
    if config.source_format == "tuning-json":
        source = Tuning.model_validate_json(text)
    else:
        assert config.root_note is not None and config.root_frequency is not None
        source = Tuning(
            source=scala.parse_scala(text, name=config.source.stem),
            root_note=config.root_note,
            root_frequency=config.root_frequency,
        )
    result = ufor_tuning.preview(
        source, notes=list(range(config.first_note, config.last_note + 1))
    )
    converted = sum(
        k.status == ufor_tuning.PitchStatus.CONVERTED for k in result.entries
    )
    data: dict[str, object] = {
        "selected_keys": len(result.entries),
        "converted_keys": converted,
        "entries": [
            {
                "note": k.note,
                "status": k.status.value,
                "frequency": None if k.frequency is None else str(k.frequency),
                "semitone": None if k.pitch is None else k.pitch.semitone,
                "fraction": None if k.pitch is None else k.pitch.fraction,
                "encoded_frequency_hz": k.encoded_frequency_hz,
                "error_cents": k.error_cents,
                "reason": k.reason,
            }
            for k in result.entries
        ],
    }
    if not isinstance(config, UforExport):
        data.update({"kind": "ufor_preview", "dry_run": True})
        emit(data, config.output)
        return 0 if converted == len(result.entries) else 1
    plan = ufor_tuning.bulk_plan(
        result,
        device_id=config.device_id,
        program=config.program,
        name=config.name,
        bank=config.bank,
        preserve_unmapped=config.preserve_unmapped,
        gap_ns=0,
    )
    with config.destination.open("xb") as destination:
        syx.write(destination, [plan.message(0)])
    data.update(
        {
            "kind": "ufor_export",
            "exported": True,
            "destination": str(config.destination),
            "message_count": plan.message_count,
            "wire_bytes": plan.wire_bytes,
            "preserved_keys": 128 - converted,
        }
    )
    emit(data, config.output)
    return 0
