"""Opt-in MIDI 1.0 loopback; never select a pre-existing endpoint."""

from pathlib import Path
from statistics import median, quantiles
from time import perf_counter_ns, process_time_ns, sleep
from uuid import uuid4

import pytest
from measure import emit

from meedee import archive, events, messages, midi


@pytest.mark.parametrize("isolated", [False, True], ids=["in_process", "isolated"])
def test_roundtrip(isolated: bool, tmp_path: Path) -> None:
    name = f"meedee-qualification-{uuid4().hex}"
    start = perf_counter_ns()
    with midi.TypedInput(isolated=isolated).create_virtual(name) as receiver:
        sender = midi.TypedOutput(isolated=isolated)
        matches = [p for p in sender.ports() if p.name == name]
        assert len(matches) == 1, "test-owned endpoint was not discovered uniquely"
        with sender.connect(matches[0]) as output:
            opened_ns = perf_counter_ns() - start
            cpu = process_time_ns()
            idle = perf_counter_ns()
            sleep(0.5)
            emit(
                "native_idle",
                isolated=isolated,
                open_wall_ns=opened_ns,
                wall_ns=perf_counter_ns() - idle,
                parent_cpu_ns=process_time_ns() - cpu,
                child_cpu_measured=False,
            )
            samples = []
            receiver.start_recording(tmp_path / "loopback.meedee")
            for n in range(256):
                value = messages.ControlChange(channel=1, controller=7, value=n % 128)
                start = perf_counter_ns()
                output.send_message(value)
                received = receiver.receive(timeout=2)
                assert isinstance(received, events.Event), received
                assert received.message == value
                samples.append(perf_counter_ns() - start)
            report = receiver.stop_recording()
            assert report.complete and report.gap_records == 0, report.failure
            reader = archive.Reader(tmp_path / "loopback.meedee")
            try:
                assert sum(isinstance(e, events.Event) for e in reader) == 256
                assert reader.complete
            finally:
                reader.close()
            emit(
                "native_roundtrip",
                isolated=isolated,
                count=256,
                median_ns=median(samples),
                p95_ns=quantiles(samples, n=100)[94],
                max_ns=max(samples),
                samples_ns=samples,
                timing=(
                    "host send entry to typed receive return; "
                    "includes recording admission"
                ),
            )
