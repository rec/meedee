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
        raise ValueError("Install middi[cli,ufor] for uFor tuning commands") from error
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
    result: list[tuning.NoteTuning] = []
    for note in range(config.first_note, config.last_note + 1):
        try:
            result.append(tuning.convert(source, note))
        except KeyError as error:
            if not config.preserve_unmapped:
                raise ValueError(str(error)) from error
            result.append(tuning.NoteTuning(note=note, pitch=tuning.NoChange()))
        except ArithmeticError as error:
            raise ValueError(f"cannot convert note {note}: {error}") from error
    converted = sum(isinstance(k.pitch, tuning.Pitch) for k in result)
    data: dict[str, object] = {
        "selected_keys": len(result),
        "converted_keys": converted,
    }
    dump = tuning.bulk_dump(
        result,
        device_id=config.device_id,
        program=config.program,
        name=config.name,
        bank=config.bank,
        preserve_unmapped=config.preserve_unmapped,
    )
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
