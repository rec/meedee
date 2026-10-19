"""Caller-driven discovery and CLI scheduling with controlled scan results."""

import json
from types import SimpleNamespace

import pytest

from meedee import discovery, midi


class Client:
    def __init__(self, scans: list[list[SimpleNamespace] | Exception]) -> None:
        self.scans = scans
        self.calls = 0

    def ports(self) -> list[SimpleNamespace]:
        self.calls += 1
        scan = self.scans.pop(0) if len(self.scans) > 1 else self.scans[0]
        if isinstance(scan, Exception):
            raise scan
        return scan


def port(name: str) -> SimpleNamespace:
    return SimpleNamespace(backend=midi.Backend.CoreMidi, backend_id=1, name=name)


def install(
    monkeypatch: pytest.MonkeyPatch, inputs: Client, outputs: Client
) -> list[str]:
    created: list[str] = []

    def input_client(**kwargs: object) -> Client:
        created.append("input")
        return inputs

    def output_client(**kwargs: object) -> Client:
        created.append("output")
        return outputs

    monkeypatch.setattr(midi, "Input", input_client)
    monkeypatch.setattr(midi, "Output", output_client)
    return created


def test_polling_is_lazy_reuses_clients_and_keeps_only_observable_descriptors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    inputs = Client([[port("keys")], [port("renamed")]])
    outputs = Client([[port("synth")]])
    created = install(monkeypatch, inputs, outputs)
    poller = discovery.Poller(protocol="midi1")
    assert created == [] and poller.snapshot().generation == 0
    first = poller.poll()
    assert first.port_count == 2 and len(created) == 2
    second = poller.poll()
    assert len(created) == 2 and second.generation == 2
    assert second.last is not None and second.last.added == second.last.removed == 1
    assert first.ports[0].endpoint.name == "keys"
    poller.close()
    assert poller.closed and poller.snapshot().generation == 2
    with pytest.raises(ValueError, match="closed"):
        poller.poll()


@pytest.mark.parametrize("failure", ["discovery", "overflow"])
def test_partial_failed_scan_cannot_replace_trusted_baseline(
    failure: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    inputs = Client([[port("keys")], [port("changed during failed scan")]])
    failed_output: list[SimpleNamespace] | Exception = (
        midi.PortInfoError("discovery failed")
        if failure == "discovery"
        else [port("synth"), port("extra")]
    )
    outputs = Client([[port("synth")], failed_output])
    install(monkeypatch, inputs, outputs)
    poller = discovery.Poller(protocol="midi1", port_limit=2)
    poller.poll()
    report = poller.poll()
    assert report.stopped and poller.closed and report.generation == 1
    assert report.port_count == 2 and report.ports[0].endpoint.name == "keys"
    assert report.last is not None and report.last.added == report.last.removed == 0
    assert report.history == []
    with pytest.raises(ValueError, match="closed"):
        poller.poll()
    assert inputs.calls == outputs.calls == 2


def test_initial_client_failure_has_no_baseline(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def failed_client(**kwargs: object) -> Client:
        raise midi.InitError("unavailable")

    monkeypatch.setattr(midi, "Input", failed_client)
    poller = discovery.Poller(protocol="midi1")
    report = poller.poll()
    assert report.stopped and report.generation == 0 and report.last is None
    assert report.reason == "unavailable" and poller.closed


def test_watch_cli_is_quiet_for_unchanged_scans_and_stops_at_duration(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    pytest.importorskip("tyro", reason="install meedee[cli]")
    pytest.importorskip("pydantic", reason="install meedee[cli]")
    from meedee import _topology_cli, cli

    inputs = Client([[port("keys")], [port("keys")], [port("renamed")]])
    outputs = Client([[]])
    install(monkeypatch, inputs, outputs)
    elapsed = 0.0

    def now() -> float:
        return elapsed

    def advance(delay: float) -> None:
        nonlocal elapsed
        elapsed += delay

    monkeypatch.setattr(_topology_cli, "monotonic", now)
    monkeypatch.setattr(_topology_cli, "sleep", advance)
    with pytest.raises(SystemExit) as result:
        cli.main(
            [
                "watch-ports",
                "--protocol",
                "midi1",
                "--duration",
                "2.5",
                "--output",
                "json",
            ]
        )
    assert result.value.code == 0
    rows = [json.loads(r) for r in capsys.readouterr().out.splitlines()]
    assert len(rows) == 3
    assert rows[0]["kind"] == "topology_baseline" and rows[0]["initialized"]
    assert (
        rows[1]["kind"] == "topology_change"
        and rows[1]["added"] == rows[1]["removed"] == 1
    )
    assert rows[2]["kind"] == "topology_summary" and rows[2]["generation"] == 3
    assert len(rows[2]["history"]) == 1
    assert inputs.calls == outputs.calls == 3


def test_ports_cli_reports_scan_failure_without_claiming_empty_discovery(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    pytest.importorskip("tyro", reason="install meedee[cli]")
    pytest.importorskip("pydantic", reason="install meedee[cli]")
    from meedee import cli

    install(monkeypatch, Client([midi.PortInfoError("scan failed")]), Client([[]]))
    with pytest.raises(SystemExit) as result:
        cli.main(["ports", "--protocol", "midi1", "--output", "json"])
    assert result.value.code == 2
    data = json.loads(capsys.readouterr().out)
    assert data["kind"] == "topology_failure" and not data["initialized"]
    assert data["reason"] == "scan failed"


def test_watch_interrupt_keeps_its_baseline_without_inventing_changes(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    pytest.importorskip("tyro", reason="install meedee[cli]")
    pytest.importorskip("pydantic", reason="install meedee[cli]")
    from meedee import _topology_cli, cli

    inputs, outputs = Client([[port("keys")]]), Client([[]])
    install(monkeypatch, inputs, outputs)

    def interrupt(delay: float) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr(_topology_cli, "sleep", interrupt)
    with pytest.raises(SystemExit) as result:
        cli.main(["watch-ports", "--protocol", "midi1", "--output", "json"])
    assert result.value.code == 0
    rows = [json.loads(r) for r in capsys.readouterr().out.splitlines()]
    assert len(rows) == 2 and rows[-1]["interrupted"]
    assert rows[-1]["initialized"] and rows[-1]["generation"] == 1
    assert rows[-1]["history"] == [] and inputs.calls == outputs.calls == 1
