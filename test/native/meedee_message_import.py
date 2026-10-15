"""Import pure values while native worker initialization is forbidden."""

import sys
from unittest.mock import patch


def main() -> None:
    with patch(
        "atexit.register", side_effect=AssertionError("cleanup was initialized")
    ):
        from meedee import codecs, controls, events, messages, tuning

        assert events.ReceivedPacket.__module__ == "meedee.events"
        assert (
            codecs.encode(messages.ProgramChange(channel=16, program=128))
            == b"\xcf\x7f"
        )
        assert codecs.Decoder().feed(b"\xf8") == [messages.TimingClock()]
        assert (
            len(
                codecs.expand(
                    messages.NrpnSet(
                        channel=1, parameter=1234, value=10000, resolution=14
                    )
                )
            )
            == 6
        )
        assert controls.Interpreter().feed(messages.TimingClock()) == []
        assert tuning.Pitch.from_frequency(frequency_hz=440.0).pitch.semitone == 69
        assert codecs.encode(
            tuning.BulkDumpRequest(device_id=1, program=0)
        ) == bytes.fromhex("f0 7e 01 08 00 00 f7")

        assert messages.NoteOn(channel=1, note=60, velocity=100).note == 60
        assert messages.SysEx(data=memoryview(bytearray([0, 127]))).data == bytes(
            [0, 127]
        )
        assert "meedee.midi" not in sys.modules
        assert "meedee._native" not in sys.modules
        assert "meedee._clients" not in sys.modules


if __name__ == "__main__":
    main()
