"""Caller-driven discovery polling. Import and construction start no clients."""

from __future__ import annotations

from typing import TYPE_CHECKING, Literal, Self

from . import topology

if TYPE_CHECKING:
    from . import midi


class Poller:
    def __init__(
        self,
        *,
        protocol: Literal["midi1", "ump"],
        backend: str | None = None,
        isolated: bool = True,
        port_limit: int = 256,
        detail_limit: int = 128,
        history_limit: int = 128,
    ) -> None:
        if protocol not in ("midi1", "ump"):
            raise ValueError("protocol must be midi1 or ump")
        if backend is not None and backend not in _BACKENDS:
            raise ValueError("unknown discovery backend")
        if type(isolated) is not bool:
            raise TypeError("isolated must be a boolean")
        self._tracker = topology.Tracker(
            port_limit=port_limit,
            detail_limit=detail_limit,
            history_limit=history_limit,
        )
        self._protocol = protocol
        self._backend = backend
        self._isolated = isolated
        self._port_limit = port_limit
        self._input: midi.Input | None = None
        self._output: midi.Output | None = None
        self._closed = False

    def poll(self) -> topology.Report:
        if self._closed:
            raise ValueError("discovery poller is closed")
        from . import midi

        try:
            protocol = (
                midi.Protocol.Midi1 if self._protocol == "midi1" else midi.Protocol.Ump
            )
            backend = (
                None if self._backend is None else getattr(midi.Backend, self._backend)
            )
            if self._input is None:
                self._input = midi.Input(
                    protocol=protocol, backend=backend, isolated=self._isolated
                )
            if self._output is None:
                self._output = midi.Output(
                    protocol=protocol, backend=backend, isolated=self._isolated
                )
            endpoints: list[topology.Endpoint] = []
            clients: list[
                tuple[Literal["input", "output"], midi.Input | midi.Output]
            ] = [("input", self._input), ("output", self._output)]
            for direction, client in clients:
                ports = client.ports()
                if len(endpoints) + len(ports) > self._port_limit:
                    raise ValueError(
                        "topology port limit exceeded; complete scan rejected"
                    )
                endpoints.extend(
                    topology.Endpoint(
                        backend=str(p.backend).rpartition(".")[2],
                        protocol=self._protocol,
                        direction=direction,
                        backend_id=p.backend_id,
                        name=p.name,
                    )
                    for p in ports
                )
            return self._tracker.observe(endpoints)
        except (
            midi.MidiError,
            midi.StateError,
            midi.CallbackThreadError,
            midi.NativePanicError,
            midi.ResourceError,
            midi.WorkerError,
            midi.WorkerTimeoutError,
            OSError,
            ValueError,
        ) as error:
            self._tracker.stop(str(error))
            self.close()
            return self._tracker.snapshot()

    def snapshot(self) -> topology.Report:
        return self._tracker.snapshot()

    @property
    def closed(self) -> bool:
        return self._closed

    def close(self) -> None:
        """Release owned client references through their existing retirement path."""
        self._closed = True
        self._input = None
        self._output = None

    def __enter__(self) -> Self:
        if self._closed:
            raise ValueError("discovery poller is closed")
        return self

    def __exit__(self, *args: object) -> None:
        self.close()


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
