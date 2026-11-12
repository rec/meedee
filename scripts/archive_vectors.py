"""Regenerate the archive v2 wire fixtures without importing middi.

Run with ``uv run --extra cli scripts/archive_vectors.py``.
Fields follow plan/archive-format.md; framing uses the standard CRC implementation.
"""

from pathlib import Path
from struct import pack
from zlib import crc32

import tyro
from pydantic import BaseModel


class Generate(BaseModel, frozen=True):
    def run(self) -> None:
        c = clock("c")
        s = stream("s", "c")
        commands = [
            (1, [1, 7, 16383]),
            (2, [2, 0, 8192, 14]),
            (3, [3, 16383, 127, 7]),
            (4, [1, 0]),
            (5, [1, 1]),
            (6, [1, 2]),
            (7, [1, 3]),
            (8, [1, 121, 0, 128]),
            (9, [1, 15]),
        ]
        span = pack(">QQ", 1, 2) + timestamp(-(2**63)) + timestamp(-1)
        records = [
            c,
            s,
            event(b"\x01\x00\x9f\x7f\x7f", ticks=-(2**63), span=span),
            event(b"\x01\x00\xf0\x7d\x00\x01\x7f\xf7"),
            event(b"\x04" + bytes.fromhex("4fbf0700fedcba98")),
            *[
                event(b"\x03\x00" + pack(">H", i) + b"".join(pack(">q", v) for v in f))
                for i, f in commands
            ],
            pack(">H", 4) + blob(b"s") + pack(">Q", 2) + blob(b"loss"),
            pack(">H", 5) + b"\x00\x00" + blob(b"global") + blob(b"\x00\xff"),
            pack(">H", 5)
            + b"\x01"
            + blob(b"s")
            + timestamp(0)
            + blob(b"tempo")
            + blob(b"\x07\xa1\x20"),
            pack(">H", 6) + blob(b"s") + blob(b"reset"),
            pack(">H", 100) + b"\x00\xff",
        ]
        cases = [("all-records", "valid", records)]
        for kind, payload in [
            ("representation", b"\x63\xde\xad"),
            ("command", b"\x03\x00\x00\x0a\xde\xad"),
        ]:
            cases.extend(
                [
                    (f"future-{kind}", "valid", [c, s, event(payload, ticks=0)]),
                    (
                        f"undeclared-{kind}",
                        "undeclared",
                        [event(payload, source="ghost")],
                    ),
                    (
                        f"reversed-{kind}",
                        "reversed packet span",
                        [c, s, event(payload, span=pack(">QQ", 2, 1) + b"\x00\x00")],
                    ),
                    (
                        f"disabled-{kind}",
                        "disabled",
                        [clock("c", disabled=True), s, event(payload, ticks=0)],
                    ),
                    (
                        f"mismatched-{kind}",
                        "differs from its stream",
                        [c, clock("d"), s, event(payload, ticks=0, identity="d")],
                    ),
                    (
                        f"cross-clock-{kind}",
                        "clock generations",
                        [
                            c,
                            clock("d"),
                            stream("s", None),
                            event(
                                payload,
                                span=pack(">QQ", 1, 2)
                                + timestamp(0)
                                + timestamp(0, "d"),
                            ),
                        ],
                    ),
                ]
            )
        cases.append(("reserved-zero", "reserved record", [b"\x00\x00"]))
        cases.extend(
            [
                ("invalid-message-group", "group", [c, s, event(b"\x01\x11\xf8")]),
                (
                    "invalid-future-command-group",
                    "group",
                    [c, s, event(b"\x03\x11\x00\x0a\xde\xad")],
                ),
            ]
        )
        lines = [
            "# Generated from plan/archive-format.md by scripts/archive_vectors.py."
        ]
        lines.extend(f"{n}|{e}|{archive(r).hex()}" for n, e, r in cases)
        path = (
            Path(__file__).resolve().parents[1]
            / "crates/middi/tests/fixtures/archive-v2.txt"
        )
        path.write_text("\n".join(lines) + "\n")


def blob(value: bytes) -> bytes:
    return pack(">I", len(value)) + value


def timestamp(ticks: int | None, identity: str = "c") -> bytes:
    return (
        b"\x00"
        if ticks is None
        else b"\x01" + pack(">q", ticks) + blob(identity.encode())
    )


def clock(identity: str, *, disabled: bool = False) -> bytes:
    return (
        pack(">H", 1)
        + blob(identity.encode())
        + pack(">QQ", 1001, 30000)
        + blob(b"SMPTE")
        + b"\x01"
        + pack(">Q", 1)
        + b"\x01"
        + pack(">i", 1)
        + bytes([1 if disabled else 2])
    )


def stream(identity: str, clock_identity: str | None) -> bytes:
    return (
        pack(">H", 2)
        + blob(identity.encode())
        + blob(b"keys")
        + (
            b"\x00"
            if clock_identity is None
            else b"\x01" + blob(clock_identity.encode())
        )
    )


def event(
    payload: bytes,
    *,
    source: str = "s",
    ticks: int | None = None,
    identity: str = "c",
    span: bytes | None = None,
) -> bytes:
    return (
        pack(">H", 3)
        + blob(source.encode())
        + timestamp(ticks, identity)
        + (b"\x00" if span is None else b"\x01" + span)
        + payload
    )


def archive(records: list[bytes]) -> bytes:
    trailer = pack(">HQ", 65535, len(records)) + b"\x01" + blob(b"")
    return b"MIDDI\0\r\n\x00\x02\x00\x00" + b"".join(
        pack(">I", len(r)) + r + pack(">I", crc32(r)) for r in [*records, trailer]
    )


if __name__ == "__main__":
    tyro.cli(Generate).run()
