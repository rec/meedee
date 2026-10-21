"""Optional offline tuning sources, diagnostics and exclusive bulk SYX export."""

import json
import sys
from pathlib import Path

import pytest

if sys.version_info < (3, 13):
    pytest.skip("uFor workflow requires Python 3.13+", allow_module_level=True)
pytest.importorskip("ufor", reason="install meedee[ufor]")
pytest.importorskip("tyro", reason="install meedee[cli]")

from ufor.tuning import Computed, FrequencyTable, RatioTable, Tuning

from meedee import cli, midi, syx, transfer, tuning


def invoke(args: list[str]) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(args)
    assert isinstance(result.value.code, int)
    return result.value.code


@pytest.fixture(autouse=True)
def forbid_devices(monkeypatch: pytest.MonkeyPatch) -> None:
    def forbidden(**kwargs: object) -> None:
        raise AssertionError("offline workflow constructed a MIDI client")

    for n in ("Input", "Output", "TypedInput", "TypedOutput"):
        monkeypatch.setattr(midi, n, forbidden)


def test_json_and_scala_preview_keep_exact_values_and_report_omissions(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "tuning.json"
    source.write_text(Tuning(source=RatioTable(values=["1", "3/2"])).model_dump_json())
    before = source.read_bytes()
    assert (
        invoke(
            [
                "ufor-preview",
                str(source),
                "--source-format",
                "tuning-json",
                "--first-note",
                "69",
                "--last-note",
                "71",
                "--output",
                "json",
            ]
        )
        == 1
    )
    report = json.loads(capsys.readouterr().out)
    assert report["dry_run"] and report["converted_keys"] == 2
    assert [k["frequency"] for k in report["entries"]] == ["440", "660", None]
    assert report["entries"][2]["status"] == "unmapped"
    assert source.read_bytes() == before
    scala = tmp_path / "scale.scl"
    scala.write_text("Three steps\n3\n9/8\n3/2\n2/1\n")
    assert (
        invoke(
            [
                "ufor-preview",
                str(scala),
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
                "--output",
                "json",
            ]
        )
        == 0
    )
    report = json.loads(capsys.readouterr().out)
    assert [k["frequency"] for k in report["entries"]] == ["440", "495", "660", "880"]
    assert report["entries"][2]["error_cents"] != 0


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
    plan = transfer.read_file(destination, gap_ns=17)
    assert plan.message_count == 1 and plan.gap_ns == 17
    decoded = tuning.decode(plan.message(0))
    assert decoded is not None and decoded.checksum == tuning.ChecksumStatus.Matches
    assert isinstance(
        decoded.message, tuning.BulkDump if bank is None else tuning.KeyBasedDump
    )
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
    reader = syx.Reader(destination)
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
    assert "out_of_range" in capsys.readouterr().err


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
