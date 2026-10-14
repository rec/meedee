"""Import pure values while native worker initialization is forbidden."""

import sys
from unittest.mock import patch


def main() -> None:
    with patch(
        "atexit.register", side_effect=AssertionError("cleanup was initialized")
    ):
        from meedee import events, messages

        assert events.ReceivedPacket.__module__ == "meedee.events"

        assert messages.NoteOn(channel=1, note=60, velocity=100).note == 60
        assert messages.SysEx(data=memoryview(bytearray([0, 127]))).data == bytes(
            [0, 127]
        )
        assert "meedee.midi" not in sys.modules
        assert "meedee._native" not in sys.modules
        assert "meedee._clients" not in sys.modules


if __name__ == "__main__":
    main()
