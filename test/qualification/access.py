"""Opt-in direct CoreMIDI access probe, independent of either MIDI wrapper."""

import subprocess
import sys
from pathlib import Path

import pytest


@pytest.mark.skipif(sys.platform != "darwin", reason="CoreMIDI is macOS-only")
def test_coremidi_without_wrappers(tmp_path: Path) -> None:
    project = Path(__file__).resolve().parents[2]
    executable = tmp_path / "coremidi-access"
    build = subprocess.run(
        [
            "/usr/bin/c++",
            "-std=c++20",
            str(project / "test/native/coremidi_access.cpp"),
            "-framework",
            "CoreMIDI",
            "-framework",
            "CoreFoundation",
            "-o",
            str(executable),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert build.returncode == 0, build.stderr
    result = subprocess.run(
        [str(executable)], capture_output=True, text=True, timeout=10
    )
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", flush=True)
    assert result.returncode == 0, (
        "Direct CoreMIDI creation/disposal failed before either wrapper was involved"
    )
