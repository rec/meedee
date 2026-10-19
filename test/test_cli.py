"""Exercise CLI parsing and file operations without launching a MIDI runtime."""

import json
from pathlib import Path

import pytest

pytest.importorskip("tyro", reason="install meedee[cli] for CLI tests")
pytest.importorskip("pydantic", reason="install meedee[cli] for CLI tests")

from meedee import archive, cli, events, messages


def test_route_preview_is_offline_and_labels_simulated_deliveries(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "capture.meedee"
    writer = archive.Writer(path)
    writer.write(archive.Stream(id="s", label=""))
    writer.write(
        events.Event(
            stream="s", message=messages.NoteOn(channel=1, note=60, velocity=100)
        )
    )
    writer.finish()
    before = path.read_bytes()
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "route-preview",
                str(path),
                "--stream",
                "s",
                "--protocol",
                "midi1",
                "--destinations",
                "out",
                "--remap-channel",
                "16",
                "--output",
                "json",
            ]
        )
    assert result.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["dry_run"] and data["complete"] and data["planned"] == 1
    assert "confirmed" not in data and "channel=16" in data["examples"][0]["preview"]
    assert path.read_bytes() == before
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "route-preview",
                str(path),
                "--stream",
                "missing",
                "--protocol",
                "midi1",
                "--destinations",
                "out",
            ]
        )
    assert result.value.code == 2 and capsys.readouterr().err


def test_inspect_validate_compare_exit_status_and_json(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    a = tmp_path / "a.syx"
    b = tmp_path / "b.syx"
    a.write_bytes(b"\xf0\x7d\xf7")
    b.write_bytes(b"\xf0\x01\xf7")
    with pytest.raises(SystemExit) as result:
        cli.main(["inspect", str(a), "--format", "syx", "--output", "json"])
    assert result.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["records"] == 1 and data["message_counts"] == {"SysEx": 1}
    assert data["note_state"]["channels"] == []
    with pytest.raises(SystemExit) as result:
        cli.main(["validate", str(a), "--format", "syx"])
    assert result.value.code == 0
    assert "complete: true" in capsys.readouterr().out
    with pytest.raises(SystemExit) as result:
        cli.main(["compare", str(a), str(b), "--format", "syx", "--output", "json"])
    assert result.value.code == 1
    assert json.loads(capsys.readouterr().out)["differences"] == 1
    a.write_bytes(b"\xf0\x7d")
    with pytest.raises(SystemExit) as result:
        cli.main(["validate", str(a), "--format", "syx", "--output", "json"])
    assert result.value.code == 2
    assert not json.loads(capsys.readouterr().out)["complete"]


def test_cli_reports_bad_arguments_and_missing_files(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    with pytest.raises(SystemExit) as result:
        cli.main(["inspect", str(tmp_path / "missing"), "--format", "syx"])
    assert result.value.code == 2
    assert capsys.readouterr().err
    with pytest.raises(SystemExit) as result:
        cli.main(["inspect", "file", "--format", "syx", "--detail-limit", "0"])
    assert result.value.code == 2
    assert capsys.readouterr().err
    with pytest.raises(SystemExit) as result:
        cli.main(["--help"])
    assert result.value.code == 0
    assert "compare" in capsys.readouterr().out


def test_cli_identity_maps_and_quality_verdict(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    for s in ["a", "b"]:
        writer = archive.Writer(tmp_path / s)
        clock = events.Clock(identity=f"clock-{s}")
        writer.write(clock)
        writer.write(archive.Stream(id=s, label="", clock_identity=clock.identity))
        writer.write(events.Event(stream=s, message=messages.TimingClock()))
        writer.write(archive.Gap(stream=s, lost=1, reason="loss"))
        writer.finish()
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "compare",
                str(tmp_path / "a"),
                str(tmp_path / "b"),
                "--format",
                "archive",
                "--source-map",
                "a",
                "b",
                "--clock-map",
                "clock-a",
                "clock-b",
            ]
        )
    assert result.value.code == 0
    capsys.readouterr()
    with pytest.raises(SystemExit) as result:
        cli.main(
            ["validate", str(tmp_path / "a"), "--format", "archive", "--output", "json"]
        )
    assert result.value.code == 1
    assert json.loads(capsys.readouterr().out)["quality_problems"]["lost_events"] == 1
