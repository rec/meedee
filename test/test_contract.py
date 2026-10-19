import sys
from importlib import import_module
from importlib.metadata import version
from importlib.resources import files
from pathlib import Path
from subprocess import run

import pytest

from meedee import midi


def test_free_threaded_build_is_rejected_before_runtime_initialization() -> None:
    fixture = Path(__file__).parent / "native" / "meedee_import.py"
    run([sys.executable, str(fixture)], check=True, timeout=10)


def test_extension_version_matches_distribution() -> None:
    assert midi.__version__ == version("meedee")


def test_installed_package_includes_typing() -> None:
    package = files("meedee")
    assert package.joinpath("py.typed").is_file()
    assert package.joinpath("midi.pyi").is_file()


@pytest.mark.parametrize(
    "error", [midi.InitError, midi.PortInfoError, midi.ConnectError, midi.SendError]
)
def test_native_errors_have_a_common_base(error: type[midi.MidiError]) -> None:
    instance = error("native failure")
    assert isinstance(instance, midi.MidiError)
    assert str(instance) == "native failure"
    assert error.__module__ == "meedee.midi"


@pytest.mark.parametrize(
    "error, base",
    [
        (midi.StateError, RuntimeError),
        (midi.CallbackThreadError, RuntimeError),
        (midi.NativePanicError, RuntimeError),
        (midi.ResourceError, RuntimeError),
        (midi.WorkerError, RuntimeError),
        (midi.WorkerTimeoutError, TimeoutError),
    ],
)
def test_binding_error_categories_keep_their_builtin_bases(
    error: type[Exception], base: type[Exception]
) -> None:
    instance = error("failure detail")
    assert isinstance(instance, base)
    assert str(instance) == "failure detail"
    assert error.__module__ == "meedee.midi"


@pytest.mark.parametrize(
    "handle",
    [
        midi.Port,
        midi.InputConnection,
        midi.OutputConnection,
    ],
)
def test_resource_handles_cannot_be_constructed_directly(handle: type[object]) -> None:
    with pytest.raises(TypeError):
        handle()


@pytest.mark.parametrize("client", [midi.Input, midi.Output])
def test_client_rejects_positional_arguments_before_native_initialization(
    client: type[midi.Input] | type[midi.Output],
) -> None:
    with pytest.raises(TypeError):
        client(37)  # ty: ignore[too-many-positional-arguments]


@pytest.mark.skipif(
    sys.version_info >= (3, 13), reason="bridge is supported on Python 3.13+"
)
def test_optional_ufor_bridge_explains_its_python_requirement() -> None:
    with pytest.raises(ImportError, match="Python 3.13"):
        import_module("meedee.ufor_tuning")
