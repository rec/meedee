"""CLI entry point; unit and CLI dependencies are supplied by reccy."""

import sys


def main(args: list[str] | None = None) -> None:
    try:
        from ._cli import run
    except ModuleNotFoundError as error:
        if error.name not in {"tyro", "pydantic"}:
            raise
        sys.exit("Install meedee[cli] to use the meedee command")
    sys.exit(run(args))
