"""Caller-driven discovery polling. Import and construction start no clients."""

from __future__ import annotations

from os import PathLike
from typing import TYPE_CHECKING, Literal, Self

from . import _core, topology

if TYPE_CHECKING:
    from . import midi


class Poller:
    def __init__(
        self,
        *,
        protocol: Literal["midi1", "ump"],
        backend: str | None = None,
        run_in_subprocess: bool = True,
        subprocess_worker: Literal["python", "rust"] = "python",
        subprocess_executable: str | PathLike[str] | None = None,
        port_limit: int = 256,
        detail_limit: int = 128,
        history_limit: int = 128,
    ) -> None:
        if protocol not in ("midi1", "ump"):
            raise ValueError("protocol must be midi1 or ump")
        if backend is not None and backend not in _BACKENDS:
            raise ValueError("unknown discovery backend")
        if type(run_in_subprocess) is not bool:
            raise TypeError("run_in_subprocess must be a boolean")
        self._state = _core.DiscoveryPollState(
            protocol, port_limit, detail_limit, history_limit
        )
        self._protocol = protocol
        self._backend = backend
        if subprocess_worker not in ("python", "rust"):
            raise ValueError("subprocess_worker must be python or rust")
        self._subprocess_worker = subprocess_worker
        self._subprocess_executable = subprocess_executable
        self._run_in_subprocess = run_in_subprocess
        self._input: midi.Input | None = None
        self._output: midi.Output | None = None

    def poll(self) -> topology.Report:
        try:
            return self._state.poll(self._ports)
        finally:
            if self.closed:
                self.close()

    def snapshot(self) -> topology.Report:
        return self._state.snapshot()

    @property
    def closed(self) -> bool:
        return self._state.closed

    def close(self) -> None:
        """Release owned client references through their existing retirement path."""
        self._state.close()
        self._input = None
        self._output = None

    def __enter__(self) -> Self:
        if self.closed:
            raise ValueError("discovery poller is closed")
        return self

    def __exit__(self, *args: object) -> None:
        self.close()

    def _ports(self, direction: str) -> list[midi.Port]:
        from . import midi

        protocol = (
            midi.Protocol.Midi1 if self._protocol == "midi1" else midi.Protocol.Ump
        )
        backend = (
            None if self._backend is None else getattr(midi.Backend, self._backend)
        )
        if direction == "input":
            if self._input is None:
                self._input = midi.Input(
                    protocol=protocol,
                    backend=backend,
                    run_in_subprocess=self._run_in_subprocess,
                    subprocess_worker=self._subprocess_worker,
                    subprocess_executable=self._subprocess_executable,
                )
            return self._input.ports()
        if self._output is None:
            self._output = midi.Output(
                protocol=protocol,
                backend=backend,
                run_in_subprocess=self._run_in_subprocess,
                subprocess_worker=self._subprocess_worker,
                subprocess_executable=self._subprocess_executable,
            )
        return self._output.ports()


_BACKENDS = (
    "CoreMidi",
    "CoreMidiUmp",
    "AlsaSeq",
    "AlsaSeqUmp",
    "AlsaRaw",
    "AlsaRawUmp",
    "WinMm",
    "WindowsMidiServices",
)
