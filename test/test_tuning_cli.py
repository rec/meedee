"""Explicit MTS CLI queries use fake ports and preserve raw matched replies."""

import json
from pathlib import Path
from types import SimpleNamespace

import pytest

pytest.importorskip("tyro", reason="install meedee[cli]")
pytest.importorskip("pydantic", reason="install meedee[cli]")

from meedee import cli, codecs, events, messages, midi, testing, tuning, tuning_live


def invoke(args: list[str]) -> int:
    with pytest.raises(SystemExit) as result:
        cli.main(args)
    assert isinstance(result.value.code, int)
    return result.value.code


def arguments(path: Path) -> list[str]:
    return [
        "tuning-query",
        str(path),
        "--input-port",
        "in",
        "--output-port",
        "out",
        "--device-id",
        "1",
        "--program",
        "2",
        "--timeout-ms",
        "100",
        "--output",
        "json",
    ]


def install(
    monkeypatch: pytest.MonkeyPatch,
    path: Path,
    raw: bytes,
    fault: str = "",
) -> tuple[testing.Input, list[bytes], list[str]]:
    receiver = testing.Input(stream="s", protocol="midi1")
    sent: list[bytes] = []
    actions: list[str] = []

    class Output:
        def __enter__(self) -> "Output":
            return self

        def __exit__(self, *args: object) -> None:
            actions.append("output closed")
            assert receiver.closed
            if fault == "close":
                raise midi.WorkerError("output close failed")
            if fault == "race":
                path.write_bytes(b"another writer")

        def send(self, message: messages.Message) -> None:
            sent.append(codecs.encode(message))
            if fault == "send":
                raise midi.SendError("uncertain request")
            if fault == "input_loss":
                receiver.lose(1)
            elif fault != "timeout":
                receiver.inject(raw, clock=events.Clock(identity="c"))

    class InputClient:
        def __init__(self, **kwargs: object) -> None:
            assert kwargs["protocol"] == midi.Protocol.Midi1 and kwargs["isolated"]

        def ports(self) -> list[SimpleNamespace]:
            return [SimpleNamespace(name="in")]

        def connect(self, port: SimpleNamespace) -> testing.Input:
            actions.append("input connected")
            return receiver

    class OutputClient:
        def __init__(self, **kwargs: object) -> None:
            assert kwargs["protocol"] == midi.Protocol.Midi1 and kwargs["isolated"]

        def ports(self) -> list[SimpleNamespace]:
            return [SimpleNamespace(name="out")]

        def connect(self, port: SimpleNamespace) -> Output:
            actions.append("output connected")
            return Output()

    monkeypatch.setattr(midi, "TypedInput", InputClient)
    monkeypatch.setattr(midi, "Output", OutputClient)
    ticks = iter([0, 0])
    monkeypatch.setattr(
        tuning_live,
        "monotonic_ns",
        lambda: next(ticks, 100_000_000 if fault == "timeout" else 0),
    )
    return receiver, sent, actions


def raw_reply(bank: int | None = None) -> bytes:
    if bank is not None:
        return codecs.encode(
            tuning.ScaleOctaveDump(
                device_id=1,
                bank=bank,
                program=2,
                name="scale",
                offsets=tuning.ScaleOffsets(
                    resolution=7, values=[0 for _ in range(12)]
                ),
            )
        )
    return codecs.encode(
        tuning.BulkDump(
            device_id=1,
            program=2,
            name="dump",
            table=tuning.TuningTable(values=[tuning.NoChange() for _ in range(128)]),
        )
    )


def test_preview_and_argument_rejection_open_no_ports(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def forbidden(**kwargs: object) -> None:
        raise AssertionError("preview must not construct transports")

    monkeypatch.setattr(midi, "TypedInput", forbidden)
    monkeypatch.setattr(midi, "Output", forbidden)
    args = [
        "tuning-preview",
        "--device-id",
        "1",
        "--program",
        "2",
        "--timeout-ms",
        "100",
        "--output",
        "json",
    ]
    assert invoke(args) == 0
    data = json.loads(capsys.readouterr().out)
    assert data["dry_run"] and data["wire_hex"] == "f07e01080002f7"
    assert invoke(args + ["--device-id", "127"]) == 2
    assert capsys.readouterr().err


@pytest.mark.parametrize("kind", ["ordinary", "banked", "legacy"])
def test_query_sends_once_closes_both_ports_and_exports_original_bytes(
    kind: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    path = tmp_path / "dump.syx"
    raw = bytearray(raw_reply(3 if kind == "banked" else None))
    if kind == "legacy":
        raw[-2] ^= 1
    receiver, sent, actions = install(monkeypatch, path, bytes(raw))
    args = arguments(path) + (["--bank", "3"] if kind == "banked" else [])
    assert invoke(args) == (1 if kind == "legacy" else 0)
    data = json.loads(capsys.readouterr().out)
    assert (
        data["exported"]
        and data["status"] == "completed"
        and not data["uncertain_send"]
    )
    assert path.read_bytes() == raw and receiver.closed
    assert sent == [
        bytes.fromhex("f07e0108030302f7" if kind == "banked" else "f07e01080002f7")
    ]
    assert actions[-1] == "output closed"
    assert ("LegacyMismatch" in data["checksum"]) == (kind == "legacy")
    assert invoke(args) == 2
    assert len(sent) == 1 and path.read_bytes() == raw


@pytest.mark.parametrize("fault", ["send", "input_loss", "close", "timeout", "race"])
def test_failures_do_not_export_or_retry_and_preserve_racing_files(
    fault: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    path = tmp_path / "failed.syx"
    receiver, sent, actions = install(monkeypatch, path, raw_reply(), fault)
    assert invoke(arguments(path)) == (1 if fault == "timeout" else 2)
    data = json.loads(capsys.readouterr().out)
    assert not data["exported"] and len(sent) == 1 and receiver.closed
    assert data["uncertain_send"] == (fault == "send")
    assert actions[-1] == "output closed"
    if fault == "race":
        assert path.read_bytes() == b"another writer"
    else:
        assert not path.exists()


def test_setup_interrupt_is_reported_without_a_send_or_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def interrupt(**kwargs: object) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr(midi, "Output", interrupt)
    path = tmp_path / "interrupted.syx"
    assert invoke(arguments(path)) == 1
    assert json.loads(capsys.readouterr().out)["interrupted"] and not path.exists()
