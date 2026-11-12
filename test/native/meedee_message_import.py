"""Import pure values while native worker initialization is forbidden."""

import sys
from io import BytesIO
from unittest.mock import patch


def main() -> None:
    with patch(
        "atexit.register", side_effect=AssertionError("cleanup was initialized")
    ):
        import meedee as md
        from meedee import (
            archive,
            ci,
            clip,
            codecs,
            controls,
            events,
            gm,
            inspection,
            messages,
            metrics,
            mpe,
            notes,
            routing,
            smf,
            sysex,
            testing,
            timecode,
            topology,
            tuning,
            ump,
            universal,
        )

        assert md.Note is messages.Note and md.Output is md.midi.Output
        assert "ufor" not in sys.modules
        assert "meedee._cli" not in sys.modules
        assert "tyro" not in sys.modules
        assert "pint" not in sys.modules
        client = md.Output()
        assert not client.closed
        client.close()

        histogram = metrics.Histogram()
        histogram.observe(1)
        assert histogram.snapshot().sum_ns == 1
        deliveries = metrics.Delivery(start_ns=0)
        deliveries.observe(5)
        assert deliveries.snapshot(10).elapsed_ns == 10
        simulated = testing.Input(stream="pure", protocol="midi1")
        synthetic_clock = events.Clock(identity="simulation", origin="manual")
        simulated.inject(b"\xf8", clock=synthetic_clock)
        assert isinstance(simulated.receive(), messages.Message)
        output = testing.Output(protocol="midi1")
        output.send(messages.TimingClock())
        assert output.take_packet() == b"\xf8"
        ump_output = testing.Output(protocol="ump")
        ump_input = testing.Input(stream="ump-test", protocol="ump")
        ump_output.send(
            messages.ControlChange(
                version=2, group=1, channel=1, controller=7, value=0xFFFFFFFF
            )
        )
        recorded = ump_output.take_packet()
        assert recorded is not None
        ump_input.inject(recorded, clock=synthetic_clock)
        assert isinstance(ump_input.receive(), messages.Message)
        ump_output.close()
        ump_input.close()
        time = testing.ManualClock()
        time.advance(10)
        assert time.now_ns == 10
        simulated.close()
        output.close()
        browser = ci.Browser(
            local_muid=1,
            identity=ci.Identity(
                manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
            ),
            route=ci.Route(input="pure", output="none"),
        )
        assert browser.snapshot().status == "ready" and callable(ci.browse)
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
        assert controls.ControlInterpreter().feed(messages.TimingClock()) == []
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
        query = tuning.Query(
            request=tuning.BulkDumpRequest(device_id=1, program=0),
            stream="pure",
            timeout="10 ns",
        )
        query.prepare(0)
        query.confirm(1)
        assert query.snapshot().request_sent and callable(tuning.query)

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
        assert not timecode.Assembler(timeout="100 ns").locked
        assert controls.VelocityInterpreter().feed(messages.TimingClock()) == []

        assert ump.encode(ump.JrClock(ticks=65535)) == (0x0010FFFF,)
        assert ump.Assembler().active_streams == 0
        bend = ump.decode((0x40E00000, 0x80000000))
        assert isinstance(bend, messages.PitchBend) and bend.value == 0

        assert ci.Header(source_muid=1, destination_muid=2).version == 2
        assert ci.Address.channel(channel=16).channel_number == 16
        assert smf.Header(format=0, tracks=1, division=480).division == 480
        assert sysex.Reader.__module__ == "meedee.sysex"
        assert clip.Reader.__module__ == "meedee.clip"
        assert inspection.inspect_file(BytesIO(b"\xf0\x7d\xf7"), format="syx").complete
        assert inspection.compare_files(
            BytesIO(b"\xf0\x7d\xf7"), BytesIO(b"\xf0\x7d\xf7"), format="syx"
        ).equal
        clock = events.Clock(identity="file", tick_numerator=1, tick_denominator=480)
        observer = inspection.Observer(stream="pure", clock=clock)
        observer.feed((messages.TimingClock()).replace(stream="pure"))
        assert observer.snapshot().records == 1
        tracker = notes.Tracker(stream="pure")
        tracker.feed(
            (messages.NoteOn(channel=1, note=60, velocity=1)).replace(stream="pure")
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
            (messages.NoteOn(channel=1, note=60, velocity=1)).replace(stream="pure")
        )
        assert router.snapshot().admitted == 1
        tracker = topology.Tracker()
        assert tracker.observe([]).generation == 1
        assert list(sysex.Reader(BytesIO(b"\xf0\x7d\xf7")))[0].data == b"\x7d"
        assert list(events.with_gap(iter(["item"]), 0.1)) == ["item", 0.1]
        from meedee import discovery

        poller = discovery.Poller(protocol="midi1")
        assert poller.snapshot().generation == 0
        poller.close()
        assert archive.Stream(id="s", label="", clock_identity=clock.identity).id == "s"
        view = controls.ControlInterpreter()
        assert view.feed((messages.TimingClock()).replace(stream="s")) == []

        assert "meedee.midi" in sys.modules
        assert "meedee._native" in sys.modules
        assert "meedee._clients" in sys.modules


if __name__ == "__main__":
    main()
