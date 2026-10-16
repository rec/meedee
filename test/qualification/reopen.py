"""Opt-in direct CoreMIDI discovery probe, independent of both wrappers."""

import os
import signal
import subprocess
import sys
from pathlib import Path

import pytest


@pytest.fixture(scope="module")
def executable(tmp_path_factory: pytest.TempPathFactory) -> Path:
    project = Path(__file__).resolve().parents[2]
    path = tmp_path_factory.mktemp("coremidi-reopen") / "probe"
    result = subprocess.run(
        [
            "/usr/bin/c++",
            "-std=c++20",
            str(project / "test/native/coremidi_reopen.cpp"),
            "-framework",
            "CoreMIDI",
            "-framework",
            "CoreFoundation",
            "-o",
            str(path),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    return path


@pytest.mark.skipif(sys.platform != "darwin", reason="CoreMIDI is macOS-only")
def test_direct_endpoint_replacement(executable: Path) -> None:
    child = subprocess.Popen(
        [str(executable)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    try:
        stdout, stderr = child.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        stdout, stderr = child.communicate(timeout=10)
        pytest.fail(f"Event-driven probe deadline exceeded\n{stdout}\n{stderr}")
    print(
        f"consumer output:\n{stdout}diagnostics (both processes):\n{stderr}", flush=True
    )
    assert child.returncode == 0, "Event-driven direct SDK discovery failed"
