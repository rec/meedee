"""Inventory parsing preserves public paths without importing package code."""

from pathlib import Path

from scripts.public_symbols import python_symbols, read_notes, render, rust_symbols


def test_python_inventory_prefers_stubs_and_counts_only_module_declarations(
    tmp_path: Path,
) -> None:
    (tmp_path / "values.py").write_text("raise RuntimeError('must not import')\n")
    (tmp_path / "values.pyi").write_text(
        "from typing import overload\n"
        "from collections import Counter\n"
        "class Value:\n    def method(self) -> None: ...\n"
        "@overload\ndef convert(value: int) -> int: ...\n"
        "@overload\ndef convert(value: str) -> str: ...\n"
        "def _private() -> None: ...\n"
    )
    (tmp_path / "extra.pyi").write_text("class Extra: ...\n")
    (tmp_path / "_private.py").write_text("class Hidden: ...\n")
    assert python_symbols(tmp_path) == {
        "extra.Extra": "C",
        "values.Value": "C",
        "values.convert": "F",
    }


def test_rust_inventory_follows_reexports_and_aliases_without_recursing_cycles(
    tmp_path: Path,
) -> None:
    (tmp_path / "nested").mkdir()
    (tmp_path / "index.html").write_text(
        '<a class="mod" href="nested/index.html" title="mod middi::nested">nested</a>'
        '<a class="mod" href="nested/index.html" title="mod middi::nested">alias</a>'
        '<a class="struct" href="nested/struct.Value.html"'
        ' title="struct middi::nested::Value">Root<wbr>Value</a>'
        '<a class="struct" href="../dependency/struct.Foreign.html"'
        ' title="struct dependency::Foreign">Foreign</a>'
        '<a class="trait" href="trait.Trait.html" title="trait middi::Trait">Trait</a>'
    )
    (tmp_path / "nested/index.html").write_text(
        '<a class="mod" href="../index.html" title="mod middi::cycle">cycle</a>'
        '<a class="struct" href="struct.Value.html"'
        ' title="struct middi::nested::Value">Value</a>'
        '<a class="enum" href="enum.Kind.html"'
        ' title="enum middi::nested::Kind">Kind</a>'
        '<a class="fn" href="fn.convert.html"'
        ' title="fn middi::nested::convert">convert</a>'
    )
    assert rust_symbols(tmp_path / "index.html") == {
        "RootValue": "S",
        "nested.Value": "S",
        "nested.Kind": "E",
        "nested.convert": "F",
        "alias.Value": "S",
        "alias.Kind": "E",
        "alias.convert": "F",
    }


def test_refresh_preserves_notes_and_removes_retired_rows(tmp_path: Path) -> None:
    previous = tmp_path / "inventory.txt"
    previous.write_text(
        "PYTHON | RUST | NOTE\n"
        "values.Value [C] | values::Value [S] | Reviewed adaptation\n"
        "values.Retired [C] | -- | Old note\n"
        "values.Python [C] | -- | Python annotation\n"
    )
    assert read_notes(tmp_path / "missing.txt") == {}
    result = render(
        {"values.Value": "C", "values.Python": "C"},
        {"values.Value": "S", "values.Rust": "E"},
        read_notes(previous),
        "abc123",
    )
    assert "values.Value [C] | values::Value [S] | Reviewed adaptation" in result
    assert "values.Python [C] | -- | Python annotation" in result
    assert "-- | values::Rust [E] | Rust-only path" in result
    assert "Retired" not in result and "abc123" in result
    assert "historical, not revalidated" in result
