import sys
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
        client(37)  # ty: ignore[too-many-positional-arguments, no-matching-overload]


@pytest.mark.parametrize("client_type", [midi.Input, midi.Output])
@pytest.mark.parametrize("isolated", [False, True])
def test_client_initialization_is_deferred_until_discovery(
    monkeypatch: pytest.MonkeyPatch,
    client_type: type[midi.Input] | type[midi.Output],
    isolated: bool,
) -> None:
    def fail_initialization(*args: object, **kwargs: object) -> None:
        raise midi.InitError("initialization attempted")

    from meedee import _clients, _native

    target = _clients if isolated else _native
    monkeypatch.setattr(
        target, "Context" if isolated else client_type.__name__, fail_initialization
    )
    unopened = client_type(isolated=isolated)
    assert unopened.worker_failure is None
    unopened.close()
    assert unopened.closed
    with pytest.raises(midi.StateError):
        unopened.ports()
    client = client_type(isolated=isolated)
    with pytest.raises(midi.InitError, match="initialization attempted"):
        client.ports()


def test_root_exports_all_public_modules_and_only_curated_classes() -> None:
    from importlib import import_module
    from pkgutil import iter_modules
    from types import ModuleType

    import meedee as md

    modules = {m.name for m in iter_modules(md.__path__) if not m.name.startswith("_")}
    exported_modules = {n for n in md.__all__ if isinstance(getattr(md, n), ModuleType)}
    assert exported_modules == modules
    assert md.__all__ == sorted(set(md.__all__))
    assert {n for n in vars(md) if not n.startswith("_")} == set(md.__all__)
    for n in set(md.__all__) - modules:
        value = getattr(md, n)
        assert isinstance(value, type)
        assert getattr(import_module(value.__module__), value.__name__) is value
    assert md.Input is md.midi.Input
    assert md.Output is md.midi.Output
    assert md.Note is md.messages.Note
    assert md.Reader is md.file_io.Reader
    assert md.Writer is md.file_io.Writer
    assert md.Route is md.routing.Route
    assert md.ParameterStep is md.messages.ParameterStep
    assert not hasattr(md, "decode") and not hasattr(md, "run")
    assert not hasattr(md, "Tracker") and not hasattr(md, "Session")
