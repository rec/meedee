"""Opt-in WinMM unsupported-path checks; no hardware port is selected."""

import sys

import pytest

from meedee import midi
from meedee.events import ReceivedPacket


@pytest.mark.parametrize("run_in_subprocess", [True, False])
def test_windows_failed_virtual_open_invalidates_the_client(
    run_in_subprocess: bool,
) -> None:
    if sys.platform != "win32":
        pytest.skip("WinMM check requires Windows")
    source = midi.Input(raw=True, run_in_subprocess=run_in_subprocess)
    target = midi.Output(run_in_subprocess=run_in_subprocess)
    with pytest.raises((midi.MidiError, ValueError)):
        source.create_virtual("unsupported", receive)
    with pytest.raises((midi.MidiError, ValueError)):
        target.create_virtual("unsupported")
    for c in (source, target):
        with pytest.raises(midi.StateError):
            c.ports()


def receive(packet: ReceivedPacket) -> None:
    raise AssertionError("unsupported virtual input dispatched a callback")
