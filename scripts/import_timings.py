"""Measure public-module imports after preloading the Rust extension.

Run from the checkout with ``uv run scripts/import_timings.py``.
Each sample uses a fresh process; interpreter startup, benchmark imports, and
the Rust extension import are outside the timer. Filesystem caches stay warm.
"""

import importlib
import json
import statistics
import subprocess
import sys
from pathlib import Path
from time import perf_counter_ns


def measure(modules: list[str]) -> None:
    importlib.import_module("meedee._core")
    before = set(sys.modules)
    started = perf_counter_ns()
    for module in modules:
        importlib.import_module(module)
    elapsed = perf_counter_ns() - started
    added = sorted(set(sys.modules) - before)
    print(json.dumps({"nanoseconds": elapsed, "modules": added}))


def main() -> None:
    # Keep CLI dependencies out of the measurement subprocesses.
    import tyro
    from pydantic import BaseModel, Field

    class Options(BaseModel, frozen=True):
        samples: int = Field(default=5, ge=1)
        """Number of fresh processes per module."""

    options = tyro.cli(Options)
    root = Path(__file__).resolve().parents[1]
    files = {"meedee": root / "meedee" / "__init__.py"}
    files.update(
        {
            f"meedee.{p.stem}": p
            for p in sorted((root / "meedee").glob("*.py"))
            if not p.name.startswith("_")
        }
    )
    modules = list(files)
    lines = {m: len(p.read_text().splitlines()) for m, p in files.items()}
    lines["all modules"] = sum(lines.values())
    print(
        f"Python {sys.version.split()[0]}; {options.samples} fresh processes per module"
    )
    print("Rust preloaded; process startup excluded; filesystem caches warm.")
    print("meedee itself is already loaded by the Rust preload.")
    print("Lines: public .py files only, including comments and blank lines.")
    print(
        f"{'Module':28} {'Median ms':>10} {'Min ms':>10} "
        f"{'Max ms':>10} {'New modules':>12} {'Lines':>8}"
    )
    additions: dict[str, list[str]] = {}
    cases = {m: [m] for m in modules}
    cases["all modules"] = modules
    for module, imports in cases.items():
        durations: list[float] = []
        loaded: set[str] = set()
        for _ in range(options.samples):
            result = subprocess.run(
                [sys.executable, str(Path(__file__).resolve()), "--measure", *imports],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            )
            if result.stderr:
                print(result.stderr, file=sys.stderr, end="")
            sample = json.loads(result.stdout)
            durations.append(sample["nanoseconds"] / 1_000_000)
            loaded.update(sample["modules"])
        additions[module] = sorted(loaded)
        print(
            f"{module:28} {statistics.median(durations):10.3f} "
            f"{min(durations):10.3f} {max(durations):10.3f} "
            f"{len(loaded):12} {lines[module]:8}",
            flush=True,
        )
    print("\nNew sys.modules entries (union across samples):")
    for module, names in additions.items():
        print(f"{module}: {', '.join(names) or '(none)'}")


if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    if len(sys.argv) >= 3 and sys.argv[1] == "--measure":
        measure(sys.argv[2:])
    else:
        main()
