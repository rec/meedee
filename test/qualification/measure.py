"""Opt-in qualification, deliberately outside pytest's default file discovery."""

import json
import os
import platform
import signal
import subprocess
import sys
from collections.abc import Callable, Iterator
from importlib.metadata import version
from pathlib import Path
from statistics import median
from time import perf_counter_ns, process_time_ns, sleep

import pytest

from meedee import archive, clip, codecs, events, messages, tuning, ump


def emit(name: str, **values: object) -> None:
    print(json.dumps({"measurement": name, **values}, sort_keys=True), flush=True)


@pytest.fixture(autouse=True)
def process_resources(request: pytest.FixtureRequest) -> Iterator[None]:
    cpu = process_time_ns()
    wall = perf_counter_ns()
    yield
    peak_rss = None
    if sys.platform != "win32":
        import resource

        peak_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if sys.platform != "darwin":
            peak_rss *= 1024
    emit(
        "process_resources",
        workload=request.node.name,
        process_peak_rss_bytes=peak_rss,
        parent_cpu_ns=process_time_ns() - cpu,
        wall_ns=perf_counter_ns() - wall,
        peak_scope="entire pytest process lifetime; excludes child processes",
    )


def measure(name: str, operations: int, action: Callable[[], None]) -> None:
    """Seven batch samples; rates include validation and Python allocation costs."""
    samples = []
    cpu = process_time_ns()
    for _ in range(7):
        start = perf_counter_ns()
        action()
        samples.append(perf_counter_ns() - start)
    emit(
        name,
        operations_per_batch=operations,
        batch_wall_ns=samples,
        batch_median_ns=median(samples),
        batch_max_ns=max(samples),
        cpu_ns=process_time_ns() - cpu,
        operations_per_second=operations * 7e9 / sum(samples),
    )


def test_environment_and_import() -> None:
    # A fresh interpreter avoids reporting a cached import as startup time.
    script = Path(__file__).with_name("startup.py")
    result = subprocess.run(
        [sys.executable, str(script)], capture_output=True, text=True, timeout=30
    )
    assert result.returncode == 0, result.stderr
    emit(
        "environment",
        os=platform.platform(),
        machine=platform.machine(),
        libc=platform.libc_ver(),
        python=sys.version,
        meedee=version("meedee"),
        startup=json.loads(result.stdout),
    )


def test_byte_codecs() -> None:
    wire = b"\xb0\x07\x7f" * 2048

    def controls() -> None:
        values = codecs.Decoder().feed(wire)
        assert len(values) == 2048
        assert all(isinstance(v, messages.ControlChange) for v in values)

    measure("python_cc_decode", 2048, controls)
    command = messages.NrpnSet(channel=1, parameter=1234, value=12345, resolution=14)

    def nrpn() -> None:
        for _ in range(2048):
            parts = codecs.expand(command)
            assert len(parts) == 6
            assert sum(len(codecs.encode(p)) for p in parts) == 18

    measure("python_nrpn_expand_encode", 2048, nrpn)
    for size in (1024, 65536, 1048576):
        sysex = messages.SysEx(data=b"\x7d" * size)
        encoded = codecs.encode(sysex)

        def dump(encoded: bytes = encoded, sysex: messages.SysEx = sysex) -> None:
            decoder = codecs.Decoder()
            restored = []
            for n in range(0, len(encoded), 1024):
                restored.extend(decoder.feed(memoryview(encoded)[n : n + 1024]))
            assert restored == [sysex] and decoder.finish() == []

        measure(f"python_sysex_{size}_bytes", size, dump)
    table = tuning.TuningTable(
        values=[tuning.Pitch(semitone=n, fraction=0) for n in range(128)]
    )
    bulk = tuning.BulkDump(device_id=0, program=0, name="qualification", table=table)
    payload = codecs.encode(bulk)

    def mts() -> None:
        for _ in range(128):
            raw = codecs.Decoder().feed(payload)
            assert len(raw) == 1 and isinstance(raw[0], messages.SysEx)
            assert tuning.decode(raw[0]) is not None

    measure("python_mts_bulk_decode", 128, mts)


def test_clip_and_archive(tmp_path: Path) -> None:
    path = tmp_path / "score.midi2"
    writer = clip.Writer(path)
    for p in (
        ump.DeltaClockstamp(ticks=0),
        ump.DeltaClockstampTicksPerQuarter(ticks=480),
        ump.DeltaClockstamp(ticks=0),
        ump.start_of_clip(),
    ):
        writer.write(p)
    note = ump.Midi2NoteOn(
        group=1,
        channel=1,
        note=60,
        velocity=65535,
        attribute_type=0,
        attribute_data=0,
    )
    for _ in range(2048):
        writer.write(ump.DeltaClockstamp(ticks=1))
        writer.write(note)
    writer.write(ump.DeltaClockstamp(ticks=0))
    writer.write(ump.end_of_clip())
    writer.finish()

    def read_clip() -> None:
        reader = clip.Reader(path)
        try:
            assert sum(isinstance(e.message, ump.Midi2NoteOn) for e in reader) == 2048
        finally:
            reader.close()

    measure("python_clip_read", 2048, read_clip)
    clock = events.Clock(identity="qualification")
    path = tmp_path / "streams.meedee"
    note1 = messages.NoteOn(channel=1, note=60, velocity=100)

    def archive_roundtrip() -> None:
        writer = archive.Writer(path)
        writer.write(clock)
        for n in range(4):
            writer.write(
                archive.Stream(
                    id=str(n), label="synthetic", clock_identity=clock.identity
                )
            )
        for n in range(2048):
            writer.write(
                events.Event(
                    stream=str(n % 4),
                    timestamp=events.Timestamp(ticks=n, clock=clock),
                    message=note1,
                )
            )
        writer.finish()
        reader = archive.Reader(path)
        try:
            assert sum(isinstance(e, events.Event) for e in reader) == 2048
            assert reader.complete
        finally:
            reader.close()

    measure("python_four_stream_archive_write_read", 2048, archive_roundtrip)
    emit(
        "file_sizes",
        archive_bytes=path.stat().st_size,
        clip_bytes=(tmp_path / "score.midi2").stat().st_size,
        durable_flush=False,
    )


def test_recording_admission(tmp_path: Path) -> None:
    clock = events.Clock(identity="capture")
    note = messages.ControlChange(channel=1, controller=7, value=100)
    for paced in (True, False):
        recorders = [
            archive.Recorder(
                tmp_path / f"{paced}-{n}.meedee", clock=clock, stream=str(n)
            )
            for n in range(4)
        ]
        accepted = 0
        start = perf_counter_ns()
        try:
            for n in range(100 if paced else 2048):
                for i, recorder in enumerate(recorders):
                    accepted += recorder.record(
                        events.Event(
                            stream=str(i),
                            timestamp=events.Timestamp(ticks=n, clock=clock),
                            message=note,
                        )
                    )
                if paced:
                    sleep(0.01)
        finally:
            reports = [r.stop() for r in recorders]
        emit(
            "python_recording_paced" if paced else "python_recording_burst",
            streams=4,
            attempted_records=400 if paced else 8192,
            accepted_records=accepted,
            written_events=[r.records - 2 for r in reports],
            wall_ns=perf_counter_ns() - start,
            complete=[r.complete for r in reports],
            failures=[r.failure for r in reports],
        )
        for i, report in enumerate(reports):
            reader = archive.Reader(tmp_path / f"{paced}-{i}.meedee", recover=True)
            try:
                count = sum(isinstance(e, events.Event) for e in reader)
                assert reader.complete == report.complete
                assert count + 2 == report.records
                if report.complete:
                    assert count == (100 if paced else 2048)
            finally:
                reader.close()


@pytest.mark.parametrize("mode", ["in_process", "isolated", "rust"])
def test_native_supervised(mode: str) -> None:
    """Each backend attempt has a separate process and a 30 second outer limit."""
    project = Path(__file__).resolve().parents[2]
    if mode == "rust":
        executable = project / "target/release/examples/qualification_transport"
        if sys.platform == "win32":
            executable = executable.with_suffix(".exe")
        if not executable.exists():
            pytest.skip("build the Rust qualification_transport example first")
        command = [str(executable)]
    else:
        command = [
            sys.executable,
            "-m",
            "pytest",
            "-q",
            "-s",
            str(Path(__file__).with_name("native.py")),
            "-k",
            mode,
        ]
    child = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=os.name == "posix",
    )
    try:
        stdout, stderr = child.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(child.pid, signal.SIGKILL)
        else:
            subprocess.run(
                ["taskkill", "/PID", str(child.pid), "/T", "/F"],
                check=False,
                capture_output=True,
                timeout=10,
            )
            child.kill()
        stdout, stderr = child.communicate(timeout=10)
        emit("native", mode=mode, status="timeout", stdout=stdout, stderr=stderr)
        pytest.fail(f"{mode} exceeded the native qualification deadline")
    emit(
        "native",
        mode=mode,
        status="passed" if child.returncode == 0 else "failed",
        returncode=child.returncode,
        stdout=stdout,
        stderr=stderr,
    )
    assert child.returncode == 0, (
        f"{mode}: native qualification failed; see measurement output"
    )
