"""Interpreter bootstrap for the shared Rust subprocess worker."""

from . import _core

if __name__ == "__main__":
    _core.run_subprocess_worker()
