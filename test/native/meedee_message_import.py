"""Import pure values while native worker initialization is forbidden."""

import sys
from io import BytesIO
from unittest.mock import patch


def main() -> None:
    with patch(
        "atexit.register", side_effect=AssertionError("cleanup was initialized")
    ):
        from meedee import (
            archive,
            ci,
            ci_live,
            clip,
            codecs,
            controls,
            events,
            gm,
            inspection,
            messages,
            mpe,
            notes,
            routing,
            smf,
            syx,
            timecode,
            topology,
            transfer,
            transfer_output,
            tuning,
            ump,
            universal,
        )

        browser = ci.Browser(
            local_muid=1,
            identity=ci.Identity(
                manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
            ),
            route=ci.Route(input="pure", output="none"),
        )
        assert browser.snapshot().status == "ready" and callable(ci_live.run)
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

        assert ci.Header(source_muid=1, destination_muid=2).version == 2
        assert ci.Address.channel(channel=16).channel_number == 16
        assert smf.Header(format=0, tracks=1, division=480).division == 480
        assert syx.Reader.__module__ == "meedee.syx"
        assert clip.Reader.__module__ == "meedee.clip"
        assert inspection.inspect_file(BytesIO(b"\xf0\x7d\xf7"), format="syx").complete
        assert inspection.compare_files(
            BytesIO(b"\xf0\x7d\xf7"), BytesIO(b"\xf0\x7d\xf7"), format="syx"
        ).equal
        clock = events.Clock(identity="file", tick_numerator=1, tick_denominator=480)
        observer = inspection.Observer(stream="pure", clock=clock)
        observer.feed(events.Event(stream="pure", message=messages.TimingClock()))
        assert observer.snapshot().records == 1
        tracker = notes.Tracker(stream="pure")
        tracker.feed(
            events.Event(
                stream="pure", message=messages.NoteOn(channel=1, note=60, velocity=1)
            )
        )
        assert tracker.snapshot().channels[0].notes[0].held == 1
        router = routing.Router(
            routes=[
                routing.Route(
                    name="r", source="pure", destination="out", protocol="midi1"
                )
            ]
        )
        router.feed(
            events.Event(
                stream="pure", message=messages.NoteOn(channel=1, note=60, velocity=1)
            )
        )
        assert router.snapshot().admitted == 1
        tracker = topology.Tracker()
        assert tracker.observe([]).generation == 1
        plan = transfer.read_file(BytesIO(b"\xf0\x7d\xf7"), gap_ns=0)
        assert transfer.Session(plan).snapshot().total_messages == 1
        assert callable(transfer_output.run)
        from meedee import discovery

        poller = discovery.Poller(protocol="midi1")
        assert poller.snapshot().generation == 0
        poller.close()
        assert archive.Stream(id="s", label="", clock_identity=clock.identity).id == "s"
        view = controls.EventInterpreter()
        assert view.feed(events.Event(stream="s", message=messages.TimingClock())) == []

        assert "meedee.midi" not in sys.modules
        assert "meedee._native" not in sys.modules
        assert "meedee._clients" not in sys.modules


if __name__ == "__main__":
    main()
