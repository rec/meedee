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
    wheels = list((project / "dist").glob("meedee-*.whl"))
    assert wheels, "Build a wheel into dist/ before running distribution checks"
    for w in wheels:
        with ZipFile(w) as archive:
            names = archive.namelist()
            assert "meedee/midi.pyi" in names
            assert {
                "meedee/_core.pyi",
                "meedee/messages.py",
                "meedee/messages.pyi",
                "meedee/midi.py",
                "meedee/events.py",
                "meedee/events.pyi",
                "meedee/codecs.py",
                "meedee/codecs.pyi",
                "meedee/controls.py",
                "meedee/controls.pyi",
                "meedee/ufor_tuning.py",
                "meedee/tuning.py",
                "meedee/tuning.pyi",
                "meedee/gm.py",
                "meedee/ump.py",
                "meedee/ump.pyi",
                "meedee/gm.pyi",
                "meedee/universal.py",
                "meedee/universal.pyi",
                "meedee/mpe.py",
                "meedee/mpe.pyi",
                "meedee/timecode.py",
                "meedee/timecode.pyi",
                "meedee/ci.py",
                "meedee/ci.pyi",
                "meedee/_typed.py",
                "meedee/smf.py",
                "meedee/smf.pyi",
                "meedee/syx.py",
                "meedee/syx.pyi",
                "meedee/clip.py",
                "meedee/clip.pyi",
                "meedee/archive.py",
                "meedee/archive.pyi",
                "meedee/_errors.py",
                "meedee/_wire.py",
            } <= set(names)
            assert "meedee/py.typed" in names
            assert {
                "meedee/_clients.py",
                "meedee/_transport.py",
                "meedee/_worker.py",
                "meedee/_native.pyi",
            } <= set(names)
            assert archive.read("meedee/__init__.py") == b""
            assert any(
                n.startswith("meedee/_core.") and n.endswith((".so", ".pyd"))
                for n in names
            )
            (metadata_name,) = (n for n in names if n.endswith(".dist-info/METADATA"))
            metadata = BytesParser().parsebytes(archive.read(metadata_name))
            assert metadata["Name"] == "meedee"
            assert metadata["Version"] == version
            assert metadata["Requires-Python"] == ">=3.11"
            requirements = [
                Requirement(r) for r in metadata.get_all("Requires-Dist", [])
            ]
            assert [r.name for r in requirements] == ["ufor"]
            assert metadata.get_all("Provides-Extra") == ["ufor"]
            for python in ["3.11", "3.13", "3.15"]:
                assert not any(
                    r.marker is None
                    or r.marker.evaluate(
                        {
                            "extra": "",
                            "python_version": python,
                            "python_full_version": f"{python}.0",
                        }
                    )
                    for r in requirements
                )
                assert requirements[0].marker is not None
                assert requirements[0].marker.evaluate(
                    {
                        "extra": "ufor",
                        "python_version": python,
                        "python_full_version": f"{python}.0",
                    }
                ) == (python != "3.11")
            assert metadata["License-Expression"] == "MIT"
            assert any(n.endswith("/licenses/LICENSE") for n in names)
            assert any(
                n.endswith("/licenses/third_party/libremidi/LICENSE.md") for n in names
            )
            assert any(n.endswith("/licenses/THIRD_PARTY_NOTICES.md") for n in names)
            assert (
                archive.read("meedee/midi.pyi")
                == (project / "meedee" / "midi.pyi").read_bytes()
            )
            if "manylinux" in w.name:
                assert not any(".libs/libasound" in n for n in names)


def test_source_distribution_contains_build_typing_and_test_sources() -> None:
    project = Path(__file__).resolve().parents[2]
    sources = list((project / "dist").glob("meedee-*.tar.gz"))
    assert sources, "Build a source distribution into dist/ first"
    for s in sources:
        with tarfile.open(s) as archive:
            names = {n.partition("/")[2] for n in archive.getnames()}
            assert {
                "Cargo.toml",
                "Cargo.lock",
                "pyproject.toml",
                "README.md",
                "crates/meedee/examples/qualification.rs",
                "crates/meedee/examples/qualification_transport.rs",
                "test/qualification/measure.py",
                "test/qualification/native.py",
                "test/qualification/startup.py",
                "test/qualification/access.py",
                "test/native/coremidi_access.cpp",
                "test/native/meedee_reopen.py",
                "third_party/libremidi/include/libremidi/backends/coremidi/notifications.hpp",
                "third_party/libremidi/tests/unit/coremidi_keeper.cpp",
                "third_party/libremidi/tests/unit/winmm_discovery.cpp",
                "LICENSE",
                "THIRD_PARTY_NOTICES.md",
                "meedee/midi.pyi",
                "meedee/py.typed",
                "meedee/_clients.py",
                "meedee/_transport.py",
                "meedee/_worker.py",
                "meedee/_native.pyi",
                "src/ci.rs",
                "crates/meedee/src/ci.rs",
                "crates/meedee/src/ci_session.rs",
                "crates/meedee/tests/ci.rs",
                "crates/meedee/tests/fixtures/ci.txt",
                "test/test_ci.py",
                "test/native/meedee_ci_loopback.py",
                "src/files.rs",
                "src/archive.rs",
                "src/event_values.rs",
                "src/live.rs",
                "src/recording.rs",
                "src/playback.rs",
                "src/file_timing.rs",
                "src/event_views.rs",
                "crates/meedee/src/smf.rs",
                "crates/meedee/src/syx.rs",
                "crates/meedee/src/clip.rs",
                "crates/meedee/src/archive.rs",
                "crates/meedee/src/events.rs",
                "crates/meedee/src/live.rs",
                "crates/meedee/src/recording.rs",
                "crates/meedee/src/playback.rs",
                "crates/meedee/src/file_timing.rs",
                "crates/meedee/src/event_views.rs",
                "crates/meedee/tests/files.rs",
                "crates/meedee/tests/archive.rs",
                "crates/meedee/tests/live.rs",
                "crates/meedee/tests/file_timing.rs",
                "crates/meedee/tests/event_views.rs",
                "test/test_files.py",
                "test/test_archive.py",
                "test/test_live.py",
                "test/test_file_timing.py",
                "test/test_event_views.py",
                "test/native/meedee_typed_loopback.py",
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
                "meedee/events.py",
                "meedee/events.pyi",
                "meedee/codecs.py",
                "meedee/codecs.pyi",
                "meedee/controls.py",
                "meedee/controls.pyi",
                "meedee/ufor_tuning.py",
                "meedee/tuning.py",
                "meedee/tuning.pyi",
                "meedee/gm.py",
                "meedee/ump.py",
                "meedee/ump.pyi",
                "meedee/gm.pyi",
                "meedee/universal.py",
                "meedee/universal.pyi",
                "meedee/mpe.py",
                "meedee/mpe.pyi",
                "meedee/timecode.py",
                "meedee/timecode.pyi",
                "meedee/ci.py",
                "meedee/ci.pyi",
                "meedee/_typed.py",
                "meedee/smf.py",
                "meedee/smf.pyi",
                "meedee/syx.py",
                "meedee/syx.pyi",
                "meedee/clip.py",
                "meedee/clip.pyi",
                "meedee/archive.py",
                "meedee/archive.pyi",
                "meedee/_errors.py",
                "meedee/_wire.py",
                "crates/meedee/Cargo.toml",
                "crates/meedee/build.rs",
                "crates/meedee/native/CMakeLists.txt",
                "crates/meedee/native/bridge.h",
                "crates/meedee/native/bridge.cpp",
                "crates/meedee/src/transport/mod.rs",
                "crates/meedee/src/codecs.rs",
                "crates/meedee/src/controls.rs",
                "crates/meedee/src/tuning.rs",
                "crates/meedee/src/gm.rs",
                "crates/meedee/src/gm_extensions.rs",
                "crates/meedee/src/ump.rs",
                "crates/meedee/src/ump_values.rs",
                "crates/meedee/src/ump_stream.rs",
                "crates/meedee/src/ump_translation.rs",
                "crates/meedee/src/ump_helpers.rs",
                "crates/meedee/tests/ump.rs",
                "crates/meedee/tests/fixtures/ump.txt",
                "test/test_ump.py",
                "crates/meedee/src/velocity.rs",
                "crates/meedee/tests/profile_engines.rs",
                "crates/meedee/tests/fixtures/profile-engines.txt",
                "test/test_profile_engines.py",
                "crates/meedee/src/gm_tables.rs",
                "crates/meedee/src/mpe.rs",
                "crates/meedee/src/universal.rs",
                "crates/meedee/src/timecode.rs",
                "crates/meedee/tests/profiles.rs",
                "crates/meedee/tests/fixtures/profiles.txt",
                "test/test_profiles.py",
                "crates/meedee/tests/tuning.rs",
                "crates/meedee/tests/fixtures/tuning.txt",
                "test/test_ufor_tuning.py",
                "test/test_tuning.py",
                "crates/meedee/tests/controls.rs",
                "crates/meedee/tests/fixtures/commands.txt",
                "test/test_controls.py",
                "crates/meedee/tests/byte_codec.rs",
                "crates/meedee/tests/fixtures/midi1.txt",
                "test/test_codecs.py",
                "crates/meedee/src/transport/native.rs",
                "crates/meedee/src/transport/worker.rs",
                "crates/meedee/src/transport/ffi.rs",
                "crates/meedee/tests/coremidi_boundary.rs",
                "crates/meedee/tests/macos_loopback.rs",
                "crates/meedee/tests/coremidi-fixture/Cargo.toml",
                "crates/meedee/tests/coremidi-fixture/build.rs",
                "crates/meedee/tests/coremidi-fixture/src/lib.rs",
                "crates/meedee/tests/coremidi-fixture/coremidi.cpp",
                "third_party/libremidi/include/libremidi/libremidi-c.cpp",
                "third_party/libremidi/LICENSE.md",
                "crates/meedee/src/lib.rs",
                "crates/meedee/src/messages.rs",
                "src/lifecycle.rs",
                "test/native/meedee_callbacks.py",
                "test/native/meedee_worker.py",
                "test/native/meedee_messages.py",
                "test/test_isolation.py",
            } <= names
            assert not any(n.startswith(("target/", ".venv/", ".git/")) for n in names)
