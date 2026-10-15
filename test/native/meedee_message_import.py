"""Import pure values while native worker initialization is forbidden."""

import sys
from unittest.mock import patch


def main() -> None:
    with patch(
        "atexit.register", side_effect=AssertionError("cleanup was initialized")
    ):
        from meedee import (
            archive,
            clip,
            codecs,
            controls,
            events,
            gm,
            messages,
            mpe,
            smf,
            syx,
            timecode,
            tuning,
            ump,
            universal,
        )

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
        assert gm.program(program=1).name == "Acoustic Grand Piano"
        assert (
            len(codecs.expand(mpe.ZoneConfiguration(manager_channel=1, members=1))) == 5
        )
        assert codecs.encode(universal.IdentityRequest(device_id=127)) == bytes.fromhex(
            "f0 7e 7f 06 01 f7"
        )
        assert timecode.FullFrame.__module__ == "meedee.timecode"
        assert tuning.Pitch.from_frequency(frequency_hz=440.0).pitch.semitone == 69
        assert codecs.encode(
            tuning.BulkDumpRequest(device_id=1, program=0)
        ) == bytes.fromhex("f0 7e 01 08 00 00 f7")

        assert messages.NoteOn(channel=1, note=60, velocity=100).note == 60
        assert messages.SysEx(data=memoryview(bytearray([0, 127]))).data == bytes(
            [0, 127]
        )
        assert (
            gm.KeyBasedControl(device_id=1, channel=10, note=36, entries=[]).entries
            == ()
        )
        assert (
            mpe.Allocator(zone=mpe.ZoneConfiguration(manager_channel=1, members=1))
            .allocate(note=60, velocity=100)
            .channel
            == 2
        )
        assert not timecode.Assembler(timeout_ns=100).locked
        assert controls.VelocityInterpreter().feed(messages.TimingClock()) is None

        assert ump.encode(ump.JrClock(ticks=65535)) == (0x0010FFFF,)
        assert ump.Assembler().active_streams == 0
        bend = ump.decode((0x40E00000, 0x80000000))
        assert isinstance(bend, ump.Midi2PitchBend) and bend.value == 0

        assert smf.Header(format=0, tracks=1, division=480).division == 480
        assert syx.Reader.__module__ == "meedee.syx"
        assert clip.Reader.__module__ == "meedee.clip"
        clock = events.Clock(identity="file", tick_numerator=1, tick_denominator=480)
        assert archive.Stream(id="s", label="", clock_identity=clock.identity).id == "s"
        view = controls.EventInterpreter()
        assert view.feed(events.Event(stream="s", message=messages.TimingClock())) == []

        assert "meedee.midi" not in sys.modules
        assert "meedee._native" not in sys.modules
        assert "meedee._clients" not in sys.modules


if __name__ == "__main__":
    main()
