"""Duration-based SysEx export rejects incomplete recordings and overwrite."""

import json
from pathlib import Path
from types import SimpleNamespace

import pytest

from middi import archive, cli, messages, midi


def invoke(path: Path, *args: str) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "sysex-receive",
                str(path),
                "--port",
                "synth",
                "--duration",
                "100 ms",
                "--output",
                "json",
                *args,
            ]
        )
    assert isinstance(result.value.code, int)
    return result.value.code


def install(
    monkeypatch: pytest.MonkeyPatch, records: list[object], names: list[str]
) -> list[str]:
    actions: list[str] = []

    class Receiver:
        def __enter__(self) -> "Receiver":
            return self

        def __exit__(self, *args: object) -> None:
            actions.append("closed")

        def record(self, duration: float) -> list[object]:
            assert duration == 0.1
            actions.append("recorded")
            return records

    class Client:
        def __init__(self, **kwargs: object) -> None:
            assert (
                kwargs["encoding"] == midi.Encoding.Bytes
                and kwargs["run_in_subprocess"]
            )
            actions.append("created")

        def ports(self) -> list[SimpleNamespace]:
            return [SimpleNamespace(name=n) for n in names]

        def connect(self, port: SimpleNamespace) -> Receiver:
            actions.append("connected")
            return Receiver()

    monkeypatch.setattr(midi, "Input", Client)
    return actions


def test_recording_exports_only_sysex_after_closing_and_refuses_overwrite(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    actions = install(
        monkeypatch,
        [
            messages.SysEx(data=b"\x01"),
            messages.TimingClock(),
            messages.SysEx(data=b"\x02"),
        ],
        ["synth"],
    )
    path = tmp_path / "dump.syx"
    assert invoke(path) == 0
    assert actions == ["created", "connected", "recorded", "closed"]
    assert path.read_bytes() == b"\xf0\x01\xf7\xf0\x02\xf7"
    report = json.loads(capsys.readouterr().out)
    assert report["received_messages"] == 2 and report["ignored_events"] == 1
    assert invoke(path) == 2
    assert actions == ["created", "connected", "recorded", "closed"]


@pytest.mark.parametrize(
    "records",
    [
        [],
        [archive.Gap(stream="s", lost=1, reason="loss")],
        [archive.Discontinuity(stream="s", reason="failed")],
    ],
)
def test_empty_or_incomplete_recording_never_creates_file(
    records: list[object],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    install(monkeypatch, records, ["synth"])
    path = tmp_path / "failed.syx"
    assert invoke(path) == 2 and not path.exists()
    assert not json.loads(capsys.readouterr().out)["exported"]


def test_ambiguous_port_precedes_recording(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    actions = install(monkeypatch, [], ["synth", "synth"])
    path = tmp_path / "invalid.syx"
    assert invoke(path) == 2
    assert actions == ["created"] and not path.exists()
    assert "found 2" in json.loads(capsys.readouterr().out)["failure"]
