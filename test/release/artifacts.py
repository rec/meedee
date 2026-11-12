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
    wheels = list((project / "dist").glob("middi-*.whl"))
    assert wheels, "Build a wheel into dist/ before running distribution checks"
    for w in wheels:
        with ZipFile(w) as archive:
            names = archive.namelist()
            assert "middi/midi.pyi" in names
            assert {
                "middi/_core.pyi",
                "middi/messages.py",
                "middi/messages.pyi",
                "middi/midi.py",
                "middi/events.py",
                "middi/events.pyi",
                "middi/codecs.py",
                "middi/codecs.pyi",
                "middi/controls.py",
                "middi/controls.pyi",
                "middi/_ufor_cli.py",
                "middi/tuning.py",
                "middi/tuning.pyi",
                "middi/_tuning_cli.py",
                "middi/gm.py",
                "middi/ump.py",
                "middi/ump.pyi",
                "middi/gm.pyi",
                "middi/universal.py",
                "middi/universal.pyi",
                "middi/mpe.py",
                "middi/mpe.pyi",
                "middi/timecode.py",
                "middi/timecode.pyi",
                "middi/ci.py",
                "middi/ci.pyi",
                "middi/testing.py",
                "middi/testing.pyi",
                "middi/metrics.py",
                "middi/metrics.pyi",
                "middi/_ci_cli.py",
                "middi/inspection.py",
                "middi/inspection.pyi",
                "middi/notes.py",
                "middi/notes.pyi",
                "middi/routing.py",
                "middi/routing.pyi",
                "middi/topology.py",
                "middi/topology.pyi",
                "middi/discovery.py",
                "middi/_transfer_cli.py",
                "middi/_topology_cli.py",
                "middi/_routing_cli.py",
                "middi/_patch.py",
                "middi/cli.py",
                "middi/_cli.py",
                "middi/_monitor.py",
                "middi/smf.py",
                "middi/smf.pyi",
                "middi/sysex.py",
                "middi/sysex.pyi",
                "middi/clip.py",
                "middi/clip.pyi",
                "middi/archive.py",
                "middi/archive.pyi",
                "middi/_errors.py",
            } <= set(names)
            assert "middi/py.typed" in names
            assert {
                "middi/_clients.py",
                "middi/_transport.py",
                "middi/_worker.py",
                "middi/_native.pyi",
            } <= set(names)
            assert archive.read("middi/__init__.py") == b""
            assert any(
                n.startswith("middi/_core.") and n.endswith((".so", ".pyd"))
                for n in names
            )
            (metadata_name,) = (n for n in names if n.endswith(".dist-info/METADATA"))
            metadata = BytesParser().parsebytes(archive.read(metadata_name))
            entry_points = archive.read(
                metadata_name.rsplit("/", 1)[0] + "/entry_points.txt"
            ).decode()
            assert "middi=middi.cli:main" in entry_points.replace(" ", "")
            assert metadata["Name"] == "middi"
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
                n.endswith("/licenses/crates/middi/third_party/libremidi/LICENSE.md")
                for n in names
            )
            assert any(
                n.endswith("/licenses/crates/middi/THIRD_PARTY_NOTICES.md")
                for n in names
            )
            assert (
                archive.read("middi/midi.pyi")
                == (project / "middi" / "midi.pyi").read_bytes()
            )
            if "manylinux" in w.name:
                assert not any(".libs/libasound" in n for n in names)


def test_source_distribution_contains_build_typing_and_test_sources() -> None:
    project = Path(__file__).resolve().parents[2]
    sources = list((project / "dist").glob("middi-*.tar.gz"))
    assert sources, "Build a source distribution into dist/ first"
    for s in sources:
        with tarfile.open(s) as archive:
            names = {n.partition("/")[2] for n in archive.getnames()}
            assert {
                "Cargo.toml",
                "Cargo.lock",
                "pyproject.toml",
                "README.md",
                "crates/middi/examples/qualification.rs",
                "crates/middi/examples/qualification_transport.rs",
                "test/qualification/measure.py",
                "test/qualification/native.py",
                "test/qualification/startup.py",
                "test/qualification/access.py",
                "test/native/coremidi_access.cpp",
                "test/native/middi_reopen.py",
                "crates/middi/third_party/libremidi/include/libremidi/backends/coremidi/notifications.hpp",
                "crates/middi/third_party/libremidi/tests/unit/coremidi_keeper.cpp",
                "crates/middi/third_party/libremidi/tests/unit/winmm_discovery.cpp",
                "LICENSE",
                "crates/middi/LICENSE",
                "crates/middi/THIRD_PARTY_NOTICES.md",
                "middi/midi.pyi",
                "middi/py.typed",
                "middi/_clients.py",
                "middi/_transport.py",
                "middi/_worker.py",
                "middi/_native.pyi",
                "src/ci.rs",
                "src/inspection.rs",
                "src/notes.rs",
                "src/routing.rs",
                "src/topology.rs",
                "crates/middi/src/topology.rs",
                "crates/middi/tests/tuning_query.rs",
                "middi/_tuning_cli.py",
                "test/test_tuning_query.py",
                "test/test_tuning_cli.py",
                "middi/_ufor_cli.py",
                "test/test_ufor_cli.py",
                "test/test_with_gap.py",
                "test/test_capture_cli.py",
                "middi/_transfer_cli.py",
                "test/test_transfer_cli.py",
                "crates/middi/tests/topology.rs",
                "test/test_topology.py",
                "test/test_discovery.py",
                "crates/middi/src/routing.rs",
                "crates/middi/tests/routing.rs",
                "test/test_routing.py",
                "test/test_patch.py",
                "crates/middi/src/notes.rs",
                "crates/middi/tests/notes.rs",
                "test/test_notes.py",
                "test/test_note_inspection.py",
                "src/comparison.rs",
                "crates/middi/src/comparison.rs",
                "crates/middi/src/inspection_timing.rs",
                "crates/middi/tests/comparison.rs",
                "crates/middi/tests/inspection_timing.rs",
                "middi/cli.py",
                "middi/_cli.py",
                "test/test_cli.py",
                "test/test_monitor.py",
                "middi/_monitor.py",
                "test/test_comparison.py",
                "crates/middi/src/inspection.rs",
                "crates/middi/tests/inspection.rs",
                "test/test_inspection.py",
                "crates/middi/src/ci.rs",
                "crates/middi/src/testing.rs",
                "crates/middi/tests/testing.rs",
                "src/testing.rs",
                "src/metrics.rs",
                "crates/middi/src/metrics.rs",
                "crates/middi/tests/metrics.rs",
                "middi/metrics.py",
                "middi/metrics.pyi",
                "test/test_metrics.py",
                "test/test_testing.py",
                "crates/middi/tests/ci_browser.rs",
                "test/test_ci_browser.py",
                "crates/middi/tests/ci.rs",
                "crates/middi/tests/fixtures/ci.txt",
                "test/test_ci.py",
                "test/native/middi_ci_loopback.py",
                "src/files.rs",
                "src/archive.rs",
                "src/event_values.rs",
                "src/live.rs",
                "src/recording.rs",
                "src/playback.rs",
                "src/file_timing.rs",
                "src/event_views.rs",
                "crates/middi/src/smf.rs",
                "crates/middi/src/sysex.rs",
                "crates/middi/src/clip.rs",
                "crates/middi/src/archive.rs",
                "crates/middi/src/events.rs",
                "crates/middi/src/live.rs",
                "crates/middi/src/recording.rs",
                "crates/middi/src/playback.rs",
                "crates/middi/src/file_timing.rs",
                "crates/middi/src/event_views.rs",
                "crates/middi/tests/files.rs",
                "crates/middi/tests/archive.rs",
                "crates/middi/tests/fixtures/archive-v1.txt",
                "crates/middi/tests/live.rs",
                "crates/middi/tests/file_timing.rs",
                "crates/middi/tests/event_views.rs",
                "test/test_files.py",
                "test/test_archive.py",
                "test/test_live.py",
                "test/test_file_timing.py",
                "test/test_event_views.py",
                "test/native/middi_typed_loopback.py",
                "test/native/middi_sysex_loopback.py",
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
                "middi/events.py",
                "middi/events.pyi",
                "middi/codecs.py",
                "middi/codecs.pyi",
                "middi/controls.py",
                "middi/controls.pyi",
                "middi/tuning.py",
                "middi/tuning.pyi",
                "middi/gm.py",
                "middi/ump.py",
                "middi/ump.pyi",
                "middi/gm.pyi",
                "middi/universal.py",
                "middi/universal.pyi",
                "middi/mpe.py",
                "middi/mpe.pyi",
                "middi/timecode.py",
                "middi/timecode.pyi",
                "middi/ci.py",
                "middi/ci.pyi",
                "middi/testing.py",
                "middi/testing.pyi",
                "middi/_ci_cli.py",
                "middi/smf.py",
                "middi/smf.pyi",
                "middi/sysex.py",
                "middi/sysex.pyi",
                "middi/clip.py",
                "middi/clip.pyi",
                "middi/archive.py",
                "middi/archive.pyi",
                "middi/_errors.py",
                "crates/middi/Cargo.toml",
                "crates/middi/build.rs",
                "crates/middi/native/CMakeLists.txt",
                "crates/middi/native/bridge.h",
                "crates/middi/native/bridge.cpp",
                "crates/middi/src/transport/mod.rs",
                "crates/middi/src/codecs.rs",
                "crates/middi/src/controls.rs",
                "crates/middi/src/tuning.rs",
                "crates/middi/src/gm.rs",
                "crates/middi/src/gm_extensions.rs",
                "crates/middi/src/ump.rs",
                "crates/middi/src/ump_values.rs",
                "crates/middi/src/ump_stream.rs",
                "crates/middi/src/ump_translation.rs",
                "crates/middi/src/ump_helpers.rs",
                "crates/middi/tests/ump.rs",
                "crates/middi/tests/fixtures/ump.txt",
                "test/test_ump.py",
                "crates/middi/src/velocity.rs",
                "crates/middi/tests/profile_engines.rs",
                "crates/middi/tests/fixtures/profile-engines.txt",
                "test/test_profile_engines.py",
                "crates/middi/src/gm_tables.rs",
                "crates/middi/src/mpe.rs",
                "crates/middi/src/universal.rs",
                "crates/middi/src/timecode.rs",
                "crates/middi/tests/profiles.rs",
                "crates/middi/tests/fixtures/profiles.txt",
                "test/test_profiles.py",
                "crates/middi/tests/tuning.rs",
                "crates/middi/tests/fixtures/tuning.txt",
                "test/test_ufor_tuning.py",
                "test/test_tuning.py",
                "crates/middi/tests/controls.rs",
                "crates/middi/tests/fixtures/commands.txt",
                "test/test_controls.py",
                "crates/middi/tests/byte_codec.rs",
                "crates/middi/tests/fixtures/midi1.txt",
                "test/test_codecs.py",
                "crates/middi/src/transport/native.rs",
                "crates/middi/src/transport/worker.rs",
                "crates/middi/src/transport/ffi.rs",
                "crates/middi/tests/coremidi_boundary.rs",
                "crates/middi/tests/macos_loopback.rs",
                "crates/middi/tests/coremidi-fixture/Cargo.toml",
                "crates/middi/tests/coremidi-fixture/build.rs",
                "crates/middi/tests/coremidi-fixture/src/lib.rs",
                "crates/middi/tests/coremidi-fixture/coremidi.cpp",
                "crates/middi/third_party/libremidi/include/libremidi/libremidi-c.cpp",
                "crates/middi/third_party/libremidi/LICENSE.md",
                "crates/middi/src/lib.rs",
                "crates/middi/src/messages.rs",
                "src/lifecycle.rs",
                "test/native/middi_callbacks.py",
                "test/native/middi_worker.py",
                "test/native/middi_messages.py",
                "test/test_isolation.py",
            } <= names
            assert not any(n.startswith(("target/", ".venv/", ".git/")) for n in names)
