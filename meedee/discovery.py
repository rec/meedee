"""Caller-driven discovery polling. Import and construction start no clients."""

from __future__ import annotations

from os import PathLike
from typing import TYPE_CHECKING, Literal, Self

from pydantic import Field, PrivateAttr, field_validator

from . import _core, topology
from ._resource import ResourceModel

if TYPE_CHECKING:
    from . import midi


class Poller(ResourceModel, frozen=True):
    protocol: Literal["midi1", "ump"]
    backend: (
        Literal[
            "CoreMidi",
            "CoreMidiUmp",
            "AlsaSeq",
            "AlsaSeqUmp",
            "AlsaRaw",
            "AlsaRawUmp",
            "WinMm",
            "WindowsMidiServices",
        ]
        | None
    ) = None
    run_in_subprocess: bool = True
    subprocess_worker: Literal["python", "rust"] = "python"
    subprocess_executable: str | PathLike[str] | None = None
    port_limit: int = Field(default=256, gt=0)
    detail_limit: int = Field(default=128, gt=0)
    history_limit: int = Field(default=128, gt=0)

    _state: _core.DiscoveryPollState = PrivateAttr()
    _input: midi.Input | None = PrivateAttr(default=None)
    _output: midi.Output | None = PrivateAttr(default=None)

    @field_validator("port_limit", "detail_limit", "history_limit", mode="before")
    @classmethod
    def _limits(cls, value: object) -> object:
        if type(value) is not int:
            raise ValueError("expected a built-in integer")
        return value

    def model_post_init(self, context: object) -> None:
        self._state = _core.DiscoveryPollState(
            self.protocol, self.port_limit, self.detail_limit, self.history_limit
        )

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
            midi.Protocol.Midi1 if self.protocol == "midi1" else midi.Protocol.Ump
        )
        backend = None if self.backend is None else getattr(midi.Backend, self.backend)
        if direction == "input":
            if self._input is None:
                self._input = midi.Input(
                    protocol=protocol,
                    backend=backend,
                    run_in_subprocess=self.run_in_subprocess,
                    subprocess_worker=self.subprocess_worker,
                    subprocess_executable=self.subprocess_executable,
                )
            return self._input.ports()
        if self._output is None:
            self._output = midi.Output(
                protocol=protocol,
                backend=backend,
                run_in_subprocess=self.run_in_subprocess,
                subprocess_worker=self.subprocess_worker,
                subprocess_executable=self.subprocess_executable,
            )
        return self._output.ports()
