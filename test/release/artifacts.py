"""Opt-in distribution checks, run after building into the repository's dist/."""

import tarfile
import tomllib
from email.parser import BytesParser
from pathlib import Path
from zipfile import ZipFile


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
                "meedee/tuning.py",
                "meedee/tuning.pyi",
                "meedee/gm.py",
                "meedee/gm.pyi",
                "meedee/universal.py",
                "meedee/universal.pyi",
                "meedee/mpe.py",
                "meedee/mpe.pyi",
                "meedee/timecode.py",
                "meedee/timecode.pyi",
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
            assert metadata.get_all("Requires-Dist") is None
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
                "LICENSE",
                "THIRD_PARTY_NOTICES.md",
                "meedee/midi.pyi",
                "meedee/py.typed",
                "meedee/_clients.py",
                "meedee/_transport.py",
                "meedee/_worker.py",
                "meedee/_native.pyi",
                "src/lib.rs",
                "src/messages.rs",
                "src/codecs.rs",
                "src/controls.rs",
                "src/tuning.rs",
                "src/profiles.rs",
                "src/profile_engines.rs",
                "src/packets.rs",
                "src/ports.rs",
                "src/transport_tests.rs",
                "meedee/events.py",
                "meedee/events.pyi",
                "meedee/codecs.py",
                "meedee/codecs.pyi",
                "meedee/controls.py",
                "meedee/controls.pyi",
                "meedee/tuning.py",
                "meedee/tuning.pyi",
                "meedee/gm.py",
                "meedee/gm.pyi",
                "meedee/universal.py",
                "meedee/universal.pyi",
                "meedee/mpe.py",
                "meedee/mpe.pyi",
                "meedee/timecode.py",
                "meedee/timecode.pyi",
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
