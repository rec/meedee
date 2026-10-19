"""Patch-bay host behavior with controlled transports; no endpoint is opened."""

import json
from types import SimpleNamespace
from typing import Self

import pytest

pytest.importorskip("tyro", reason="install meedee[cli]")
pytest.importorskip("pydantic", reason="install meedee[cli]")

from meedee import cli, events, messages, midi
from meedee._cli import Patch
from meedee._patch import run_patch


def test_patch_cli_parses_static_routes_before_invoking_host(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[Patch] = []

    def accept(config: Patch) -> int:
        calls.append(config)
        return 0

    monkeypatch.setattr("meedee._patch.run_patch", accept)
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "patch",
                "keys",
                "--protocol",
                "midi1",
                "--destinations",
                "a",
                "b",
                "--remap-channel",
                "16",
            ]
        )
    assert result.value.code == 0 and len(calls) == 1
    assert calls[0].destinations == ["a", "b"] and calls[0].remap_channel == 16


class Input:
    stream = "s"

    def __init__(self) -> None:
        self.dropped_packets = 0
        self.failure: Exception | None = None
        self.closed = False
        self.items: list[events.Event | events.StreamDiagnostic] = [
            events.Event(
                stream="s", message=messages.NoteOn(channel=1, note=60, velocity=1)
            )
        ]

    def ports(self) -> list[SimpleNamespace]:
        return [SimpleNamespace(name="keys")]

    def connect(self, port: SimpleNamespace) -> Self:
        return self

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True

    def receive(self, *, timeout: float) -> events.Event | events.StreamDiagnostic:
        if self.items:
            return self.items.pop(0)
        raise KeyboardInterrupt


class Output:
    def __init__(self) -> None:
        self.closed = False
        self.sent: list[messages.Message] = []
        self.error: BaseException | None = None
        self.lose_input: Input | None = None

    def ports(self) -> list[SimpleNamespace]:
        return [SimpleNamespace(name=n) for n in ["a", "b", "c"]]

    def connect(self, port: SimpleNamespace) -> Self:
        return self

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: object) -> None:
        self.closed = True

    def send_message(self, message: messages.Message) -> None:
        if self.error is not None:
            raise self.error
        self.sent.append(message)
        if self.lose_input is not None:
            self.lose_input.dropped_packets += 1


def install(
    monkeypatch: pytest.MonkeyPatch, source: Input, outputs: list[Output]
) -> None:
    remaining = iter(outputs)

    def output_client(**kwargs: object) -> Output:
        return next(remaining)

    def input_client(**kwargs: object) -> Input:
        return source

    monkeypatch.setattr(midi, "TypedOutput", output_client)
    monkeypatch.setattr(midi, "TypedInput", input_client)


def config(*, event_limit: int = 128) -> Patch:
    return Patch(
        port="keys",
        destinations=["a", "b", "c"],
        protocol="midi1",
        output="json",
        event_limit=event_limit,
        remap_channel=16,
    )


def test_patch_fans_out_remaps_and_closes_every_connection(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source, outputs = Input(), [Output() for _ in range(3)]
    install(monkeypatch, source, outputs)
    assert run_patch(config()) == 0
    data = json.loads(capsys.readouterr().out)
    assert data["confirmed"] == 3 and data["interrupted"]
    assert source.closed and all(o.closed for o in outputs)
    assert all(
        o.sent == [messages.NoteOn(channel=16, note=60, velocity=1)] for o in outputs
    )


def test_send_failure_preserves_prefix_and_never_sends_later_copies(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source, outputs = Input(), [Output() for _ in range(3)]
    outputs[1].error = midi.SendError("uncertain send")
    install(monkeypatch, source, outputs)
    assert run_patch(config()) == 2
    data = json.loads(capsys.readouterr().out)
    assert data["confirmed"] == 1 and data["pending"] == 2
    assert data["uncertain_delivery"] == 1 and not outputs[2].sent
    assert data["uncertain_route"] == data["uncertain_destination"] == "b"
    assert source.closed and all(o.closed for o in outputs)


@pytest.mark.parametrize(
    "kind", ["loss", "overload", "terminal", "interrupted_send", "diagnostic"]
)
def test_stop_policies_close_without_cleanup_or_retry(
    kind: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source, outputs = Input(), [Output() for _ in range(3)]
    limit = 2 if kind == "overload" else 128
    if kind == "loss":
        outputs[0].lose_input = source
    elif kind == "diagnostic":
        source.items = [
            events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1).loss(
                lost=1, reason="lost input"
            )
        ]
    elif kind == "terminal":
        source.failure = midi.WorkerError("worker failed")
    elif kind == "interrupted_send":
        outputs[0].error = KeyboardInterrupt()
    install(monkeypatch, source, outputs)
    assert run_patch(config(event_limit=limit)) == (
        2 if kind in ["terminal", "interrupted_send"] else 1
    )
    data = json.loads(capsys.readouterr().out)
    assert data["stopped"] and not outputs[1].sent and not outputs[2].sent
    assert data["confirmed"] == (1 if kind == "loss" else 0)
    assert source.closed and all(o.closed for o in outputs)
