"""Optional offline tuning sources, diagnostics and exclusive bulk SYX export."""

import json
from pathlib import Path

import pytest

pytest.importorskip("ufor", reason="install middi[ufor]")
pytest.importorskip("tyro", reason="install middi[cli]")

from ufor.tuning import Computed, FrequencyTable, RatioTable, Tuning

from middi import cli, file_io, midi, tuning


def invoke(args: list[str]) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(args)
    assert isinstance(result.value.code, int)
    return result.value.code


@pytest.fixture(autouse=True)
def forbid_devices(monkeypatch: pytest.MonkeyPatch) -> None:
    def forbidden(**kwargs: object) -> None:
        raise AssertionError("offline workflow constructed a MIDI client")

    for n in ("Input", "Output"):
        monkeypatch.setattr(midi, n, forbidden)


def test_scala_export_converts_each_selected_note(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "scale.scl"
    source.write_text("Three steps\n3\n9/8\n3/2\n2/1\n")
    destination = tmp_path / "scale.syx"
    assert (
        invoke(
            [
                "ufor-export",
                str(source),
                str(destination),
                "--source-format",
                "scala",
                "--root-note",
                "69",
                "--root-frequency",
                "440",
                "--first-note",
                "69",
                "--last-note",
                "72",
                "--device-id",
                "1",
                "--program",
                "0",
                "--name",
                "scale",
                "--preserve-unmapped",
                "--output",
                "json",
            ]
        )
        == 0
    )
    report = json.loads(capsys.readouterr().out)
    assert report["converted_keys"] == 4
    plan = list(file_io.Reader(destination))
    decoded = tuning.decode(plan[0])
    assert decoded is not None and isinstance(decoded.message, tuning.BulkDump)
    assert decoded.message.table.values[71] == tuning.Pitch(semitone=76, fraction=320)


@pytest.mark.parametrize("bank", [None, 3])
def test_complete_bulk_export_is_transfer_ready_and_refuses_overwrite(
    bank: int | None,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "tuning.json"
    source.write_text(Tuning(source=Computed()).model_dump_json())
    destination = tmp_path / "dump.syx"
    args = [
        "ufor-export",
        str(source),
        str(destination),
        "--source-format",
        "tuning-json",
        "--device-id",
        "1",
        "--program",
        "2",
        "--name",
        "test",
        "--output",
        "json",
    ] + ([] if bank is None else ["--bank", str(bank)])
    assert invoke(args) == 0
    report = json.loads(capsys.readouterr().out)
    assert (
        report["exported"]
        and report["converted_keys"] == 128
        and report["preserved_keys"] == 0
    )
    plan = list(file_io.Reader(destination))
    assert len(plan) == 1
    decoded = tuning.decode(plan[0])
    assert decoded is not None and decoded.checksum == tuning.ChecksumStatus.Matches
    assert isinstance(decoded.message, tuning.BulkDump) and decoded.message.bank == bank
    before = destination.read_bytes()
    assert invoke(args) == 2 and capsys.readouterr().err
    assert destination.read_bytes() == before


def test_partial_export_requires_preservation_and_never_hides_invalid_pitches(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "partial.json"
    source.write_text(Tuning(source=RatioTable(values=["1", "3/2"])).model_dump_json())
    destination = tmp_path / "partial.syx"
    args = [
        "ufor-export",
        str(source),
        str(destination),
        "--source-format",
        "tuning-json",
        "--first-note",
        "69",
        "--last-note",
        "71",
        "--device-id",
        "1",
        "--program",
        "2",
        "--name",
        "partial",
        "--output",
        "json",
    ]
    assert invoke(args) == 2 and not destination.exists()
    assert capsys.readouterr().err
    assert invoke(args + ["--preserve-unmapped"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["converted_keys"] == 2 and report["preserved_keys"] == 126
    reader = file_io.Reader(destination)
    decoded = tuning.decode(next(reader))
    reader.close()
    assert decoded is not None and isinstance(decoded.message, tuning.BulkDump)
    assert (
        sum(isinstance(v, tuning.NoChange) for v in decoded.message.table.values) == 126
    )
    source.write_text(
        Tuning(source=FrequencyTable(values=["1/10"], first_note=69)).model_dump_json()
    )
    failed = tmp_path / "invalid.syx"
    args[2] = str(failed)
    assert invoke(args + ["--preserve-unmapped"]) == 2 and not failed.exists()
    assert "MTS frequency range" in capsys.readouterr().err


def test_bad_source_does_not_create_a_destination(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "invalid.json"
    source.write_text("{")
    destination = tmp_path / "missing.syx"
    assert (
        invoke(
            [
                "ufor-export",
                str(source),
                str(destination),
                "--source-format",
                "tuning-json",
                "--device-id",
                "1",
                "--program",
                "2",
                "--name",
                "test",
            ]
        )
        == 2
    )
    assert not destination.exists() and capsys.readouterr().err
