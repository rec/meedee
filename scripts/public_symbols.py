"""Inventory module-level Python and Rust public classes/types and functions.

Run ``uv run scripts/public_symbols.py`` from a checkout with the CLI extras
installed. This rebuilds Rustdoc, then writes public-symbols.txt, preserving any
existing row annotations. Use ``--output PATH`` to write elsewhere without
changing that inventory. Relative output paths are resolved from the repo root.

Python declarations come from .pyi files where available, otherwise .py files.
Private modules, imported helpers and root re-exports are excluded, matching the
previous inventory's Python scope. Rust includes public re-export paths, but not
traits, aliases, constants, fields or methods. The inventory describes the current
Rust target/configuration, not all platform-specific APIs or behavioral parity.
No meedee modules are imported and no MIDI runtime is started.
"""

from __future__ import annotations

import ast
import json
import subprocess
from datetime import date
from html.parser import HTMLParser
from pathlib import Path


class Listing(HTMLParser):
    """Read Rustdoc's item links, including displayed names of re-exports."""

    def __init__(self) -> None:
        super().__init__()
        self.links: list[tuple[str, str, str, str]] = []
        self.link: tuple[str, str, str] | None = None
        self.text: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag != "a":
            return
        attributes = dict(attrs)
        kind = attributes.get("class", "")
        href = attributes.get("href")
        title = attributes.get("title", "")
        if kind in ("mod", "struct", "enum", "fn") and href and title:
            self.link = (kind, href, title)
            self.text = []

    def handle_data(self, data: str) -> None:
        if self.link is not None:
            self.text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "a" and self.link is not None:
            self.links.append((*self.link, "".join(self.text)))
            self.link = None


def python_symbols(package: Path) -> dict[str, str]:
    """Read declarations once per module, preferring stubs over implementations."""
    symbols: dict[str, str] = {}
    modules = {
        p.stem
        for p in package.iterdir()
        if p.suffix in (".py", ".pyi") and not p.stem.startswith("_")
    }
    for module in sorted(modules):
        source = package / f"{module}.pyi"
        if not source.exists():
            source = source.with_suffix(".py")
        for node in ast.parse(source.read_text()).body:
            if isinstance(
                node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
            ) and not node.name.startswith("_"):
                symbols[f"{module}.{node.name}"] = (
                    "C" if isinstance(node, ast.ClassDef) else "F"
                )
    return symbols


def rust_symbols(index: Path) -> dict[str, str]:
    """Follow public module paths while excluding dependency links and cycles."""
    index = index.resolve()
    pending: list[tuple[str, Path, tuple[Path, ...]]] = [("", index, ())]
    visited: set[tuple[str, Path]] = set()
    symbols: dict[str, str] = {}
    while pending:
        module, page, ancestors = pending.pop()
        if page in ancestors or (module, page) in visited:
            continue
        visited.add((module, page))
        listing = Listing()
        listing.feed(page.read_text())
        for kind, href, title, name in listing.links:
            if not title.startswith(f"{kind} meedee::") or not name.isidentifier():
                continue
            path = f"{module}.{name}".lstrip(".")
            if kind == "mod":
                target = (page.parent / href).resolve()
                if target.is_relative_to(index.parent):
                    pending.append((path, target, (*ancestors, page)))
            else:
                symbols[path] = {"struct": "S", "enum": "E", "fn": "F"}[kind]
    return symbols


def read_notes(path: Path) -> dict[str, str]:
    """Retain existing row annotations, without asserting they are still current."""
    if not path.exists():
        return {}
    notes: dict[str, str] = {}
    for line in path.read_text().splitlines():
        columns = line.split("|", 2)
        if len(columns) != 3:
            continue
        python, rust, note = (c.strip() for c in columns)
        symbol = rust if rust != "--" else python
        if symbol.endswith((" [C]", " [S]", " [E]", " [F]")):
            notes[symbol.rsplit(" [", 1)[0].replace("::", ".").lstrip(".")] = note
    return notes


def render(
    python: dict[str, str], rust: dict[str, str], notes: dict[str, str], commit: str
) -> str:
    """Render exact-name comparisons separately from inherited audit notes."""
    shared = python.keys() & rust.keys()
    keys = sorted(python.keys() | rust.keys())
    lines = [
        "PUBLIC PYTHON AND RUST INTERFACES, SIDE BY SIDE",
        f"Generated: {date.today().isoformat()}; "
        f"source commit {commit} (working checkout).",
        "",
        "SCOPE AND METHOD",
        "- Python module-level classes/functions from stubs or source; "
        "overloads counted once.",
        "- Rust structs/enums/functions and re-export paths from rebuilt Rustdoc.",
        "- Python private modules, imported helpers and root re-exports are excluded.",
        "- Excludes methods, properties, fields, variants, constants, modules, "
        "traits and aliases.",
        "- Rust reflects the current target/configuration, not all platforms.",
        "- Names do not establish behavioral parity. "
        "Preserved annotations are historical, not revalidated.",
        "- See plan/fix-differences.md for the reviewed mappings and their scope.",
        "- No meedee import, MIDI runtime or device flow was run.",
        "- Keep public-symbols.txt uncommitted.",
        "",
        "COUNTS",
        f"Python: {len(python)} module-level classes/functions.",
        f"Rust:   {len(rust)} documented public type/function paths.",
        f"{len(shared)} paths have the same module and symbol name.",
        f"{len(python.keys() - rust.keys())} Python paths and "
        f"{len(rust.keys() - python.keys())} Rust paths have no exact counterpart.",
        f"{len(keys)} comparison rows. "
        "Unmatched names are not counts of missing features.",
        "",
        "INVENTORY",
        "[C] Python class  [S] Rust struct  [E] Rust enum  [F] free function",
    ]
    module = None
    for key in keys:
        owner = key.rpartition(".")[0]
        if owner != module:
            module = owner
            lines.extend(["", f"[{owner or '(root)'}]", "PYTHON | RUST | NOTE"])
        left = f"{key} [{python[key]}]" if key in python else "--"
        right = f"{key.replace('.', '::')} [{rust[key]}]" if key in rust else "--"
        note = notes.get(key)
        if not note:
            note = (
                "Same public module/name; signatures and behavior not compared"
                if key in shared
                else "Python-only path"
                if key in python
                else "Rust-only path"
            )
        lines.append(f"{left} | {right} | {note}")
    return "\n".join(lines) + "\n"


def main() -> None:
    import tyro
    from pydantic import BaseModel

    class Options(BaseModel, frozen=True):
        output: Path = Path("public-symbols.txt")
        """Output path; existing annotations are read from public-symbols.txt."""

    options = tyro.cli(Options)
    root = Path(__file__).resolve().parents[1]
    notes = read_notes(root / "public-symbols.txt")
    subprocess.run(
        ["cargo", "doc", "--locked", "-p", "meedee", "--no-deps"], cwd=root, check=True
    )
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"],
            cwd=root,
            text=True,
        )
    )
    index = Path(metadata["target_directory"]) / "doc" / "meedee" / "index.html"
    python = python_symbols(root / "meedee")
    rust = rust_symbols(index)
    commit = subprocess.check_output(
        ["git", "rev-parse", "--short", "HEAD"], cwd=root, text=True
    ).strip()
    output = root / options.output
    output.write_text(render(python, rust, notes, commit))
    print(f"Wrote {output}: {len(python)} Python paths, {len(rust)} Rust paths.")


if __name__ == "__main__":
    main()
