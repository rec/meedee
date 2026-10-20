"""Passive CLI capture never opens an output or overwrites existing files."""

import json
from pathlib import Path
from types import SimpleNamespace

import pytest

pytest.importorskip("tyro", reason="install meedee[cli]")
pytest.importorskip("pydantic", reason="install meedee[cli]")

from meedee import cli, events, midi, testing, transfer_input


def invoke(path: Path, *args: str) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "sysex-receive",
                str(path),
                "--port",
                "synth",
                "--expected-messages",
                "2",
                "--timeout-ms",
                "100",
                "--output",
                "json",
                *args,
            ]
        )
    assert isinstance(result.value.code, int)
    return result.value.code


def install(
    monkeypatch: pytest.MonkeyPatch,
    receiver: testing.Input,
    names: list[str],
) -> list[str]:
    actions: list[str] = []

    class Client:
        def __init__(self, **kwargs: object) -> None:
            assert kwargs["protocol"] == midi.Protocol.Midi1 and kwargs["isolated"]
            actions.append("created")

        def ports(self) -> list[SimpleNamespace]:
            return [SimpleNamespace(name=n) for n in names]

        def connect(self, port: SimpleNamespace) -> testing.Input:
            actions.append("connected")
            return receiver

    monkeypatch.setattr(midi, "TypedInput", Client)
    monkeypatch.setattr(transfer_input, "monotonic_ns", lambda: 0)
    return actions


def test_passive_capture_closes_input_then_exports_exact_messages(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    receiver.inject(b"\xf0\x01\xf8\xf7\xf0\x02\xf7", clock=events.Clock(identity="c"))
    actions = install(monkeypatch, receiver, ["synth"])
    path = tmp_path / "dump.syx"
    assert invoke(path) == 0 and receiver.closed
    assert path.read_bytes() == b"\xf0\x01\xf7\xf0\x02\xf7"
    report = json.loads(capsys.readouterr().out)
    assert report["exported"] and report["status"] == "completed"
    assert report["ignored_events"] == 1 and actions == ["created", "connected"]
    assert invoke(path) == 2
    assert actions == ["created", "connected"]
    assert path.read_bytes() == b"\xf0\x01\xf7\xf0\x02\xf7"


@pytest.mark.parametrize("fault", ["timeout", "loss", "worker", "malformed"])
def test_receive_failures_never_create_output(
    fault: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    install(monkeypatch, receiver, ["synth"])
    if fault == "loss":
        receiver.lose(1)
    elif fault == "worker":
        receiver.fail("worker failed")
    elif fault == "malformed":
        receiver.inject(b"\xf0\x01\x90\x40\x01", clock=events.Clock(identity="c"))
    else:
        ticks = iter([0])
        monkeypatch.setattr(
            transfer_input, "monotonic_ns", lambda: next(ticks, 100_000_000)
        )
    path = tmp_path / "failed.syx"
    assert invoke(path) == (1 if fault == "timeout" else 2)
    report = json.loads(capsys.readouterr().out)
    assert not report["exported"] and not path.exists() and receiver.closed


def test_validation_and_unique_name_selection_precede_receiving(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    receiver = testing.Input(stream="s", protocol="midi1")
    actions = install(monkeypatch, receiver, ["synth", "synth"])
    path = tmp_path / "invalid.syx"
    assert invoke(path, "--byte-limit", "2") == 2
    assert not actions and capsys.readouterr().err
    assert invoke(path) == 2
    assert actions == ["created"] and not path.exists()
    assert "found 2" in json.loads(capsys.readouterr().out)["failure"]
    receiver.close()
