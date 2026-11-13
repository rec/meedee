"""Optional offline uFor source resolution and bulk-dump export."""

from __future__ import annotations

from . import codecs, file_io
from ._cli import UforExport, emit


def run_ufor(config: UforExport) -> int:
    try:
        from ufor import scala
        from ufor.tuning import Tuning
    except ModuleNotFoundError as error:
        if error.name != "ufor":
            raise
        raise ValueError("Install middy[cli,ufor] for uFor tuning commands") from error
    from . import tuning

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
    notes = list(range(config.first_note, config.last_note + 1))
    try:
        dump = tuning.bulk_dump(
            source,
            notes=notes,
            device_id=config.device_id,
            program=config.program,
            name=config.name,
            bank=config.bank,
        )
    except ArithmeticError as error:
        raise ValueError(f"cannot convert tuning: {error}") from error
    converted = sum(isinstance(p, tuning.Pitch) for p in dump.table.values)
    data: dict[str, object] = {
        "selected_keys": len(notes),
        "converted_keys": converted,
    }
    wire = codecs.encode(dump)
    with file_io.Writer(config.destination) as destination:
        destination.write(dump)
    data.update(
        {
            "kind": "ufor_export",
            "exported": True,
            "destination": str(config.destination),
            "message_count": 1,
            "wire_bytes": len(wire),
            "preserved_keys": 128 - converted,
        }
    )
    emit(data, config.output)
    return 0
