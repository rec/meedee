"""Opt-in distribution checks, run after building into the repository's dist/."""

import tarfile
import tomllib
from email.parser import BytesParser
from pathlib import Path
from zipfile import ZipFile


def test_wheels_include_typed_api_and_matching_version_metadata() -> None:
    project = Path(__file__).resolve().parents[2]
    version = tomllib.loads((project / "Cargo.toml").read_text())["package"]["version"]
    wheels = list((project / "dist").glob("*.whl"))
    assert wheels, "Build a wheel into dist/ before running distribution checks"
    for w in wheels:
        with ZipFile(w) as archive:
            names = archive.namelist()
            assert "meedee/midi.pyi" in names
            assert "meedee/py.typed" in names
            assert {
                "meedee/_clients.py",
                "meedee/_transport.py",
                "meedee/_worker.py",
                "meedee/_native.pyi",
            } <= set(names)
            assert archive.read("meedee/__init__.py") == b""
            assert any(
                n.startswith("meedee/midi.") and n.endswith((".so", ".pyd"))
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
            assert any(n.endswith("/licenses/THIRD_PARTY_NOTICES.md") for n in names)
            assert (
                archive.read("meedee/midi.pyi")
                == (project / "meedee" / "midi.pyi").read_bytes()
            )
            if "manylinux" in w.name:
                assert any(".libs/libasound" in n for n in names)
                assert list((project / "dist").glob("alsa-lib-*.src.rpm")), (
                    "Supply the matching ALSA source RPM alongside the Linux wheel"
                )


def test_source_distribution_contains_build_typing_and_test_sources() -> None:
    project = Path(__file__).resolve().parents[2]
    sources = list((project / "dist").glob("*.tar.gz"))
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
                "src/api_prototype.rs",
                "src/lifecycle.rs",
                "test/native/meedee_callbacks.py",
                "test/native/meedee_worker.py",
                "test/native/meedee_messages.py",
                "test/test_isolation.py",
            } <= names
            assert not any(n.startswith(("target/", ".venv/", ".git/")) for n in names)
