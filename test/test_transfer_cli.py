"""Transfer commands validate before discovery and preserve partial progress."""

import json
from pathlib import Path
from types import SimpleNamespace

import pytest

pytest.importorskip("tyro", reason="install meedee[cli]")
pytest.importorskip("pydantic", reason="install meedee[cli]")

from meedee import cli, codecs, messages, midi


class Connection:
    def __init__(self, fail: bool = False) -> None:
        self.sent: list[bytes] = []
        self.closed = False
        self.fail = fail

    def __enter__(self) -> "Connection":
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True

    def send_message(self, message: messages.SysEx) -> None:
        self.sent.append(codecs.encode(message))
        if self.fail and len(self.sent) == 2:
            raise midi.SendError("uncertain send")


def install(
    monkeypatch: pytest.MonkeyPatch, connection: Connection, names: list[str]
) -> list[str]:
    actions: list[str] = []

    class Client:
        def __init__(self, **kwargs: object) -> None:
            assert (
                kwargs["protocol"] == midi.Protocol.Midi1 and kwargs["isolated"] is True
            )
            actions.append("created")

        def ports(self) -> list[SimpleNamespace]:
            actions.append("discovered")
            return [SimpleNamespace(name=n) for n in names]

        def connect(self, port: SimpleNamespace) -> Connection:
            actions.append("connected")
            return connection

    monkeypatch.setattr(midi, "TypedOutput", Client)
    return actions


def invoke(args: list[str]) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(args)
    assert isinstance(result.value.code, int)
    return result.value.code


def test_preview_bounds_examples_and_never_constructs_transport(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "dump.syx"
    source.write_bytes(bytes.fromhex("f0 7d 01 f7 f0 7d 02 f7"))
    connection = Connection()
    actions = install(monkeypatch, connection, ["synth"])
    assert (
        invoke(
            [
                "transfer-preview",
                str(source),
                "--gap-ms",
                "20",
                "--detail-limit",
                "1",
                "--preview-bytes",
                "1",
                "--output",
                "json",
            ]
        )
        == 0
    )
    data = json.loads(capsys.readouterr().out)
    assert data["dry_run"] and data["message_count"] == 2 and data["wire_bytes"] == 8
    assert data["gap_ns"] == 20_000_000 and data["omitted_examples"] == 1
    assert data["examples"] == [{"index": 0, "wire_bytes": 4, "payload_preview": "7d"}]
    assert actions == []


@pytest.mark.parametrize("contents", ["f0 01 f7 f0 02", "f0 01 f7 f8", ""])
def test_send_rejects_entire_bad_file_before_discovery(
    contents: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "dump.syx"
    source.write_bytes(bytes.fromhex(contents))
    actions = install(monkeypatch, Connection(), ["synth"])
    assert (
        invoke(
            [
                "transfer-send",
                str(source),
                "--gap-ms",
                "0",
                "--port",
                "synth",
                "--output",
                "json",
            ]
        )
        == 2
    )
    assert actions == [] and capsys.readouterr().err


@pytest.mark.parametrize("fail", [False, True])
def test_send_preserves_exact_bytes_and_reports_uncertain_progress(
    fail: bool,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "dump.syx"
    source.write_bytes(bytes.fromhex("f0 7d 01 f7 f0 7d 02 f7 f0 7d 03 f7"))
    connection = Connection(fail)
    actions = install(monkeypatch, connection, ["synth"])
    assert invoke(
        [
            "transfer-send",
            str(source),
            "--gap-ms",
            "0",
            "--port",
            "synth",
            "--output",
            "json",
        ]
    ) == (2 if fail else 0)
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == ("failed" if fail else "completed")
    assert data["confirmed_messages"] == (1 if fail else 3)
    assert data["confirmed_bytes"] == (4 if fail else 12)
    assert data["uncertain_message"] == (1 if fail else None)
    assert connection.closed and actions == ["created", "discovered", "connected"]
    assert connection.sent == [
        bytes.fromhex(f"f0 7d 0{i} f7") for i in range(1, 3 if fail else 4)
    ]


def test_ambiguous_output_never_opens_or_sends(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "dump.syx"
    source.write_bytes(bytes.fromhex("f0 7d f7"))
    connection = Connection()
    actions = install(monkeypatch, connection, ["synth", "synth"])
    assert (
        invoke(
            [
                "transfer-send",
                str(source),
                "--gap-ms",
                "0",
                "--port",
                "synth",
                "--output",
                "json",
            ]
        )
        == 2
    )
    data = json.loads(capsys.readouterr().out)
    assert data["confirmed_messages"] == 0 and data["uncertain_message"] is None
    assert actions == ["created", "discovered"] and connection.sent == []


def test_interrupt_during_close_keeps_completed_handoffs_visible(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    class InterruptedClose(Connection):
        def __exit__(self, *args: object) -> None:
            raise KeyboardInterrupt

    source = tmp_path / "dump.syx"
    source.write_bytes(bytes.fromhex("f0 7d f7"))
    install(monkeypatch, InterruptedClose(), ["synth"])
    assert (
        invoke(
            [
                "transfer-send",
                str(source),
                "--gap-ms",
                "0",
                "--port",
                "synth",
                "--output",
                "json",
            ]
        )
        == 1
    )
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "completed" and data["interrupted"]
    assert data["confirmed_messages"] == 1 and data["uncertain_message"] is None
