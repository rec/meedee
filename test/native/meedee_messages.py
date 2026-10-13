"""Device-free contract and measurements for the Rust representation prototype."""

import gc
import inspect
import sys
import tracemalloc
from dataclasses import dataclass
from statistics import median
from time import perf_counter_ns
from types import ModuleType
from unittest import TestCase


def verify(module: ModuleType) -> None:
    case = TestCase()
    with case.assertRaisesRegex(TypeError, "abstract"):
        module.Message()
    assert issubclass(module.NoteOn, module.Message)
    assert issubclass(module.SysEx, module.Message)
    assert module.NoteOn.__module__ == "meedee.messages"
    assert module.NoteOn.__annotations__ == {
        "channel": int,
        "note": int,
        "velocity": int,
    }
    assert module.SysEx.__annotations__ == {"data": bytes}
    assert str(inspect.signature(module.NoteOn)) == "(*, channel, note, velocity)"
    assert str(inspect.signature(module.SysEx)) == "(*, data)"

    note = module.NoteOn(channel=1, note=60, velocity=100)
    assert isinstance(note, module.Message)
    assert (note.channel, note.note, note.velocity) == (1, 60, 100)
    assert repr(note) == "NoteOn(channel=1, note=60, velocity=100)"
    assert not hasattr(note, "__dict__")
    for n in ("channel", "note", "velocity", "extra"):
        with case.assertRaises(AttributeError):
            setattr(note, n, 3)
    with case.assertRaises(AttributeError):
        object.__setattr__(note, "note", 61)
    with case.assertRaises(AttributeError):
        del note.note
    with case.assertRaises(TypeError):
        module.NoteOn(1, 60, 100)
    for n in (True, False, 1.0, "1", None):
        with case.assertRaises(TypeError):
            module.NoteOn(channel=n, note=60, velocity=100)
        with case.assertRaises(TypeError):
            note.replace(note=n)
    for n in (0, 17, -1, 10**100):
        with case.assertRaises(ValueError):
            module.NoteOn(channel=n, note=60, velocity=100)
    for n in (-1, 128, 10**100):
        with case.assertRaises(ValueError):
            module.NoteOn(channel=1, note=n, velocity=100)
        with case.assertRaises(ValueError):
            note.replace(velocity=n)
    for n in (
        module.NoteOn(channel=1, note=0, velocity=0),
        module.NoteOn(channel=16, note=127, velocity=127),
    ):
        assert n == n.replace()
        assert hash(n) == hash(n.replace())
    changed = note.replace(note=61)
    assert changed.note == 61 and note.note == 60
    assert changed != note
    assert note != object()
    with case.assertRaises(TypeError):
        note.replace(unknown=3)
    match note:
        case module.NoteOn(1, 60, 100):
            pass
        case _:
            raise AssertionError("native class pattern matching failed")

    source = bytearray([0x7E, 0x7F, 0x06, 0x01])
    exclusive = module.SysEx(data=memoryview(source))
    source[0] = 1
    assert exclusive.data == b"\x7e\x7f\x06\x01"
    assert type(exclusive.data) is bytes
    assert repr(exclusive) == "SysEx(data=b'~\\x7f\\x06\\x01')"
    assert exclusive == module.SysEx(data=exclusive.data)
    assert hash(exclusive) == hash(exclusive.replace())
    assert not hasattr(exclusive, "__dict__")
    with case.assertRaises(AttributeError):
        exclusive.data = b"\x01"
    with case.assertRaises(TypeError):
        exclusive.data[0] = 1  # ty: ignore[invalid-assignment]
    for d in (b"\x00\x7f", bytearray([0, 127]), memoryview(b"\x00\x7f")):
        assert module.SysEx(data=d).data == b"\x00\x7f"
    for d in (b"\x80", b"\xf0\x01\xf7"):
        with case.assertRaises(ValueError):
            module.SysEx(data=d)
    for d in ([1, 2], "text", memoryview(b"abc")[::2]):
        with case.assertRaises((TypeError, BufferError)):
            module.SysEx(data=d)
    replacement = bytearray([1, 2])
    replaced = exclusive.replace(data=replacement)
    replacement[0] = 3
    assert replaced.data == b"\x01\x02"
    assert exclusive.data == b"\x7e\x7f\x06\x01"
    with case.assertRaises(TypeError):
        exclusive.replace(unknown=b"")


def measure(module: ModuleType) -> dict[str, object]:
    """Report host observations without imposing timing thresholds in tests."""
    results: dict[str, object] = {"python": sys.version.split()[0]}
    count = 10_000
    for name, constructor in (
        ("native", module.NoteOn),
        ("stdlib_dataclass", Baseline),
    ):
        samples = []
        for _ in range(5):
            gc.collect()
            start = perf_counter_ns()
            notes = [
                constructor(channel=1, note=60, velocity=100) for _ in range(count)
            ]
            samples.append((perf_counter_ns() - start) / count)
            assert notes[-1].note == 60
            del notes
        results[f"{name}_construction_ns"] = round(median(samples))
        gc.collect()
        tracemalloc.start()
        notes = [constructor(channel=1, note=60, velocity=100) for _ in range(count)]
        _, peak = tracemalloc.get_traced_memory()
        tracemalloc.stop()
        results[f"{name}_python_size_bytes"] = sys.getsizeof(notes[0])
        results[f"{name}_10000_peak_python_bytes"] = peak
        del notes
    return results


@dataclass(frozen=True, slots=True, kw_only=True)
class Baseline:
    """Standard-library comparison only, not an alternative production model."""

    channel: int
    note: int
    velocity: int
