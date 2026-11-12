"""Opt-in distribution checks, run after building into the repository's dist/."""

import tarfile
import tomllib
from email.parser import BytesParser
from pathlib import Path
from zipfile import ZipFile

from packaging.requirements import Requirement


def test_wheels_include_typed_api_and_matching_version_metadata() -> None:
    project = Path(__file__).resolve().parents[2]
    version = tomllib.loads((project / "Cargo.toml").read_text())["package"]["version"]
    wheels = list((project / "dist").glob("middy-*.whl"))
    assert wheels, "Build a wheel into dist/ before running distribution checks"
    for w in wheels:
        with ZipFile(w) as archive:
            names = archive.namelist()
            assert "middy/midi.pyi" in names
            assert {
                "middy/_core.pyi",
                "middy/messages.py",
                "middy/messages.pyi",
                "middy/midi.py",
                "middy/events.py",
                "middy/events.pyi",
                "middy/codecs.py",
                "middy/codecs.pyi",
                "middy/controls.py",
                "middy/controls.pyi",
                "middy/_ufor_cli.py",
                "middy/tuning.py",
                "middy/tuning.pyi",
                "middy/_tuning_cli.py",
                "middy/gm.py",
                "middy/ump.py",
                "middy/ump.pyi",
                "middy/gm.pyi",
                "middy/universal.py",
                "middy/universal.pyi",
                "middy/mpe.py",
                "middy/mpe.pyi",
                "middy/timecode.py",
                "middy/timecode.pyi",
                "middy/ci.py",
                "middy/ci.pyi",
                "middy/testing.py",
                "middy/testing.pyi",
                "middy/metrics.py",
                "middy/metrics.pyi",
                "middy/_ci_cli.py",
                "middy/inspection.py",
                "middy/inspection.pyi",
                "middy/notes.py",
                "middy/notes.pyi",
                "middy/routing.py",
                "middy/routing.pyi",
                "middy/topology.py",
                "middy/topology.pyi",
                "middy/discovery.py",
                "middy/_transfer_cli.py",
                "middy/_topology_cli.py",
                "middy/_routing_cli.py",
                "middy/_patch.py",
                "middy/cli.py",
                "middy/_cli.py",
                "middy/_monitor.py",
                "middy/smf.py",
                "middy/smf.pyi",
                "middy/sysex.py",
                "middy/sysex.pyi",
                "middy/clip.py",
                "middy/clip.pyi",
                "middy/archive.py",
                "middy/archive.pyi",
                "middy/_errors.py",
            } <= set(names)
            assert "middy/py.typed" in names
            assert {
                "middy/_clients.py",
                "middy/_transport.py",
                "middy/_worker.py",
                "middy/_native.pyi",
            } <= set(names)
            assert archive.read("middy/__init__.py") == b""
            assert any(
                n.startswith("middy/_core.") and n.endswith((".so", ".pyd"))
                for n in names
            )
            (metadata_name,) = (n for n in names if n.endswith(".dist-info/METADATA"))
            metadata = BytesParser().parsebytes(archive.read(metadata_name))
            entry_points = archive.read(
                metadata_name.rsplit("/", 1)[0] + "/entry_points.txt"
            ).decode()
            assert "middy=middy.cli:main" in entry_points.replace(" ", "")
            assert metadata["Name"] == "middy"
            assert metadata["Version"] == version
            assert metadata["Requires-Python"] == ">=3.13"
            requirements = [
                Requirement(r) for r in metadata.get_all("Requires-Dist", [])
            ]
            assert {r.name for r in requirements} == {
                "ufor",
                "pydantic",
                "tyro",
                "reccy",
            }
            assert set(metadata.get_all("Provides-Extra", [])) == {"ufor", "cli"}
            for python in ["3.11", "3.13", "3.15"]:
                environment = {
                    "extra": "",
                    "python_version": python,
                    "python_full_version": f"{python}.0",
                }
                assert {
                    r.name
                    for r in requirements
                    if r.marker is None or r.marker.evaluate(environment)
                } == {"pydantic", "reccy"}
                ufor = next(r for r in requirements if r.name == "ufor")
                assert ufor.marker is not None
                assert ufor.marker.evaluate(
                    {
                        "extra": "ufor",
                        "python_version": python,
                        "python_full_version": f"{python}.0",
                    }
                ) == (python != "3.11")
                for r in requirements:
                    assert (
                        r.marker is None
                        or r.marker.evaluate(
                            {
                                "extra": "cli",
                                "python_version": python,
                                "python_full_version": f"{python}.0",
                            }
                        )
                    ) == (r.name in {"tyro", "pydantic", "reccy"})
            assert metadata["License-Expression"] == "MIT"
            assert any(n.endswith("/licenses/LICENSE") for n in names)
            assert any(
                n.endswith("/licenses/crates/middy/third_party/libremidi/LICENSE.md")
                for n in names
            )
            assert any(
                n.endswith("/licenses/crates/middy/THIRD_PARTY_NOTICES.md")
                for n in names
            )
            assert (
                archive.read("middy/midi.pyi")
                == (project / "middy" / "midi.pyi").read_bytes()
            )
            if "manylinux" in w.name:
                assert not any(".libs/libasound" in n for n in names)


def test_source_distribution_contains_build_typing_and_test_sources() -> None:
    project = Path(__file__).resolve().parents[2]
    sources = list((project / "dist").glob("middy-*.tar.gz"))
    assert sources, "Build a source distribution into dist/ first"
    for s in sources:
        with tarfile.open(s) as archive:
            names = {n.partition("/")[2] for n in archive.getnames()}
            assert {
                "Cargo.toml",
                "Cargo.lock",
                "pyproject.toml",
                "README.md",
                "crates/middy/examples/qualification.rs",
                "crates/middy/examples/qualification_transport.rs",
                "test/qualification/measure.py",
                "test/qualification/native.py",
                "test/qualification/startup.py",
                "test/qualification/access.py",
                "test/native/coremidi_access.cpp",
                "test/native/middy_reopen.py",
                "crates/middy/third_party/libremidi/include/libremidi/backends/coremidi/notifications.hpp",
                "crates/middy/third_party/libremidi/tests/unit/coremidi_keeper.cpp",
                "crates/middy/third_party/libremidi/tests/unit/winmm_discovery.cpp",
                "LICENSE",
                "crates/middy/LICENSE",
                "crates/middy/THIRD_PARTY_NOTICES.md",
                "middy/midi.pyi",
                "middy/py.typed",
                "middy/_clients.py",
                "middy/_transport.py",
                "middy/_worker.py",
                "middy/_native.pyi",
                "src/ci.rs",
                "src/inspection.rs",
                "src/notes.rs",
                "src/routing.rs",
                "src/topology.rs",
                "crates/middy/src/topology.rs",
                "crates/middy/tests/tuning_query.rs",
                "middy/_tuning_cli.py",
                "test/test_tuning_query.py",
                "test/test_tuning_cli.py",
                "middy/_ufor_cli.py",
                "test/test_ufor_cli.py",
                "test/test_with_gap.py",
                "test/test_capture_cli.py",
                "middy/_transfer_cli.py",
                "test/test_transfer_cli.py",
                "crates/middy/tests/topology.rs",
                "test/test_topology.py",
                "test/test_discovery.py",
                "crates/middy/src/routing.rs",
                "crates/middy/tests/routing.rs",
                "test/test_routing.py",
                "test/test_patch.py",
                "crates/middy/src/notes.rs",
                "crates/middy/tests/notes.rs",
                "test/test_notes.py",
                "test/test_note_inspection.py",
                "src/comparison.rs",
                "crates/middy/src/comparison.rs",
                "crates/middy/src/inspection_timing.rs",
                "crates/middy/tests/comparison.rs",
                "crates/middy/tests/inspection_timing.rs",
                "middy/cli.py",
                "middy/_cli.py",
                "test/test_cli.py",
                "test/test_monitor.py",
                "middy/_monitor.py",
                "test/test_comparison.py",
                "crates/middy/src/inspection.rs",
                "crates/middy/tests/inspection.rs",
                "test/test_inspection.py",
                "crates/middy/src/ci.rs",
                "crates/middy/src/testing.rs",
                "crates/middy/tests/testing.rs",
                "src/testing.rs",
                "src/metrics.rs",
                "crates/middy/src/metrics.rs",
                "crates/middy/tests/metrics.rs",
                "middy/metrics.py",
                "middy/metrics.pyi",
                "test/test_metrics.py",
                "test/test_testing.py",
                "crates/middy/tests/ci_browser.rs",
                "test/test_ci_browser.py",
                "crates/middy/tests/ci.rs",
                "crates/middy/tests/fixtures/ci.txt",
                "test/test_ci.py",
                "test/native/middy_ci_loopback.py",
                "src/files.rs",
                "src/archive.rs",
                "src/event_values.rs",
                "src/live.rs",
                "src/recording.rs",
                "src/playback.rs",
                "src/file_timing.rs",
                "src/event_views.rs",
                "crates/middy/src/smf.rs",
                "crates/middy/src/sysex.rs",
                "crates/middy/src/clip.rs",
                "crates/middy/src/archive.rs",
                "crates/middy/src/events.rs",
                "crates/middy/src/live.rs",
                "crates/middy/src/recording.rs",
                "crates/middy/src/playback.rs",
                "crates/middy/src/file_timing.rs",
                "crates/middy/src/event_views.rs",
                "crates/middy/tests/files.rs",
                "crates/middy/tests/archive.rs",
                "crates/middy/tests/fixtures/archive-v1.txt",
                "crates/middy/tests/live.rs",
                "crates/middy/tests/file_timing.rs",
                "crates/middy/tests/event_views.rs",
                "test/test_files.py",
                "test/test_archive.py",
                "test/test_live.py",
                "test/test_file_timing.py",
                "test/test_event_views.py",
                "test/native/middy_typed_loopback.py",
                "test/native/middy_sysex_loopback.py",
                "src/lib.rs",
                "src/messages.rs",
                "src/codecs.rs",
                "src/controls.rs",
                "src/tuning.rs",
                "src/profiles.rs",
                "src/profile_engines.rs",
                "src/ump.rs",
                "src/packets.rs",
                "src/ports.rs",
                "src/transport_tests.rs",
                "middy/events.py",
                "middy/events.pyi",
                "middy/codecs.py",
                "middy/codecs.pyi",
                "middy/controls.py",
                "middy/controls.pyi",
                "middy/tuning.py",
                "middy/tuning.pyi",
                "middy/gm.py",
                "middy/ump.py",
                "middy/ump.pyi",
                "middy/gm.pyi",
                "middy/universal.py",
                "middy/universal.pyi",
                "middy/mpe.py",
                "middy/mpe.pyi",
                "middy/timecode.py",
                "middy/timecode.pyi",
                "middy/ci.py",
                "middy/ci.pyi",
                "middy/testing.py",
                "middy/testing.pyi",
                "middy/_ci_cli.py",
                "middy/smf.py",
                "middy/smf.pyi",
                "middy/sysex.py",
                "middy/sysex.pyi",
                "middy/clip.py",
                "middy/clip.pyi",
                "middy/archive.py",
                "middy/archive.pyi",
                "middy/_errors.py",
                "crates/middy/Cargo.toml",
                "crates/middy/build.rs",
                "crates/middy/native/CMakeLists.txt",
                "crates/middy/native/bridge.h",
                "crates/middy/native/bridge.cpp",
                "crates/middy/src/transport/mod.rs",
                "crates/middy/src/codecs.rs",
                "crates/middy/src/controls.rs",
                "crates/middy/src/tuning.rs",
                "crates/middy/src/gm.rs",
                "crates/middy/src/gm_extensions.rs",
                "crates/middy/src/ump.rs",
                "crates/middy/src/ump_values.rs",
                "crates/middy/src/ump_stream.rs",
                "crates/middy/src/ump_translation.rs",
                "crates/middy/src/ump_helpers.rs",
                "crates/middy/tests/ump.rs",
                "crates/middy/tests/fixtures/ump.txt",
                "test/test_ump.py",
                "crates/middy/src/velocity.rs",
                "crates/middy/tests/profile_engines.rs",
                "crates/middy/tests/fixtures/profile-engines.txt",
                "test/test_profile_engines.py",
                "crates/middy/src/gm_tables.rs",
                "crates/middy/src/mpe.rs",
                "crates/middy/src/universal.rs",
                "crates/middy/src/timecode.rs",
                "crates/middy/tests/profiles.rs",
                "crates/middy/tests/fixtures/profiles.txt",
                "test/test_profiles.py",
                "crates/middy/tests/tuning.rs",
                "crates/middy/tests/fixtures/tuning.txt",
                "test/test_ufor_tuning.py",
                "test/test_tuning.py",
                "crates/middy/tests/controls.rs",
                "crates/middy/tests/fixtures/commands.txt",
                "test/test_controls.py",
                "crates/middy/tests/byte_codec.rs",
                "crates/middy/tests/fixtures/midi1.txt",
                "test/test_codecs.py",
                "crates/middy/src/transport/native.rs",
                "crates/middy/src/transport/worker.rs",
                "crates/middy/src/transport/ffi.rs",
                "crates/middy/tests/coremidi_boundary.rs",
                "crates/middy/tests/macos_loopback.rs",
                "crates/middy/tests/coremidi-fixture/Cargo.toml",
                "crates/middy/tests/coremidi-fixture/build.rs",
                "crates/middy/tests/coremidi-fixture/src/lib.rs",
                "crates/middy/tests/coremidi-fixture/coremidi.cpp",
                "crates/middy/third_party/libremidi/include/libremidi/libremidi-c.cpp",
                "crates/middy/third_party/libremidi/LICENSE.md",
                "crates/middy/src/lib.rs",
                "crates/middy/src/messages.rs",
                "src/lifecycle.rs",
                "test/native/middy_callbacks.py",
                "test/native/middy_worker.py",
                "test/native/middy_messages.py",
                "test/test_isolation.py",
            } <= names
            assert not any(n.startswith(("target/", ".venv/", ".git/")) for n in names)
