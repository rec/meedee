"""Unified output behavior against a device-free native transport stand-in."""

from __future__ import annotations

import gc
import weakref
from collections.abc import Callable, Iterable, Iterator
from pathlib import Path
from time import monotonic

import pytest

from meedee import codecs, events, messages, midi, testing


class NativeConnection:
    def __init__(self) -> None:
        self.sink = testing.Output(encoding="bytes")
        self.closed = False
        self.fail_release = False
        self.partial: int | None = None

    def send(self, packet: bytes) -> None:
        if self.fail_release and packet[0] & 0xF0 == 0x80:
            raise midi.SendError("release failed")
        self.sink.send(packet)

    def send_command(
        self, command: messages.ControlChange | messages.ParameterStep
    ) -> None:
        for i, message in enumerate(codecs.expand(command)):
            if self.partial == i:
                raise midi.CompoundSendError("partial", i, 2)
            self.sink.send(message)

    def close(self) -> None:
        self.closed = True

    def __enter__(self) -> NativeConnection:
        return self

    def packets(self) -> list[bytes | midi.UmpPacket]:
        result: list[bytes | midi.UmpPacket] = []
        while (packet := self.sink.take_packet()) is not None:
            result.append(packet)
        return result


@pytest.fixture
def transport(monkeypatch: pytest.MonkeyPatch) -> Iterator[NativeConnection]:
    from meedee import _native

    connection = NativeConnection()

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[midi.Port]:
            return [
                midi.Port._from_wire("test", 0, i, n, midi.Backend.CoreMidi, i, False)
                for i, n in enumerate(["FLOW 8 MIDI", "Other"])
            ]

        def connect(self, port: midi.Port) -> NativeConnection:
            return connection

    monkeypatch.setattr(_native, "Output", Client)
    yield connection


def test_output_channel_overrides_messages_and_tracks_cleanup(
    transport: NativeConnection,
) -> None:
    note = messages.NoteOn(60)
    with midi.Output(channel=2, port="FLOW 8") as out:
        out.send(note, note, messages.NoteOff(60), messages.NoteOn(63, 70, 3))
    assert note.channel is None
    assert transport.packets() == [
        b"\x91\x3c\x5a",
        b"\x91\x3c\x5a",
        b"\x81\x3c\x00",
        b"\x91\x3f\x46",
        b"\x81\x3c\x00",
        b"\x81\x3f\x00",
    ]
    assert transport.closed


def test_midi2_output_resolves_channel_without_scaling_or_cleanup(
    transport: NativeConnection,
) -> None:
    transport.sink = testing.Output(encoding="ump")
    with midi.Output(encoding=midi.Encoding.Ump, channel=3) as out:
        out.send(messages.NoteOn(60, 65535, version=2))
        out.send(messages.NoteOff(60, channel=9, version=2))
    assert transport.packets() == [
        midi.UmpPacket(words=(0x40923C00, 0xFFFF0000)),
        midi.UmpPacket(words=(0x40823C00, 0)),
    ]
    assert transport.closed


def test_parameter_step_uses_output_channel_and_preserves_nrpn_family(
    transport: NativeConnection,
) -> None:
    step = messages.ParameterStep(16383, -1, family="nrpn")
    with midi.Output(channel=2) as out:
        out.send(step)
    assert step.channel is None
    assert transport.packets() == [
        b"\xb1\x63\x7f",
        b"\xb1\x62\x7f",
        b"\xb1\x61\x00",
        b"\xb1\x65\x7f",
        b"\xb1\x64\x7f",
    ]


def test_ump_channel_override_covers_every_musical_family_and_grouped_commands(
    transport: NativeConnection,
) -> None:
    from meedee import ump

    transport.sink = testing.Output(encoding="ump")
    values = [
        messages.RegisteredPerNoteController(
            group=5, channel=7, note=60, controller=3, value=1234
        ),
        messages.AssignablePerNoteController(
            group=5, channel=7, note=60, controller=3, value=1234
        ),
        messages.PerNoteManagement(group=5, channel=7, note=60, flags=3),
        messages.PerNotePitchBend(group=5, channel=7, note=60, value=-1234),
        messages.RegisteredController(group=5, channel=7, bank=1, index=2, value=1234),
        messages.AssignableController(group=5, channel=7, bank=1, index=2, value=1234),
        messages.RelativeRegisteredController(
            group=5, channel=7, bank=1, index=2, value=-1234
        ),
        messages.RelativeAssignableController(
            group=5, channel=7, bank=1, index=2, value=-1234
        ),
    ]
    with midi.Output(encoding=midi.Encoding.Ump, channel=3) as output:
        report = output.send(values)
        assert report.sent_events == 8
        original = [ump.encode(v) for v in values]
        expected = [
            midi.UmpPacket(words=((w[0] & ~0xF0000) | 0x20000, w[1])) for w in original
        ]
        assert transport.packets() == expected
        assert all(v.channel == 7 and v.group == 5 for v in values)
        report = output.send(
            messages.ControlChange(7, 10000, 2, resolution=14, group=3)
        )
        assert report.sent_events == 1
        assert transport.packets() == [midi.UmpPacket(words=(0x22B2074E, 0x22B22710))]
        with pytest.raises(ValueError, match="group"):
            output.send(
                [messages.NoteOn(60, channel=1, group=1), messages.TimingClock()]
            )
        assert transport.packets() == []


def test_call_schedules_cumulative_delays_including_trailing_time(
    transport: NativeConnection,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native

    deadlines: list[int] = []

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        timeline = [(t, m) for t, m, _ in scheduled]
        for t, m in timeline:
            deadlines.append(t)
            if m is not None:
                send(m)

    monkeypatch.setattr(_native, "output_sequence", sequence)
    with midi.Output() as out:
        out.send(
            messages.NoteOn(60),
            "300ms",
            messages.NoteOn(63),
            "3 / 10",
            messages.NoteOn(65),
            1,
        )
    assert deadlines == [0, 300_000_000, 600_000_000, 1_600_000_000]
    assert len(transport.packets()) == 6


def test_delay_only_call_waits_without_sending(transport: NativeConnection) -> None:
    with midi.Output() as out:
        start = monotonic()
        out.send("2ms")
        assert monotonic() - start >= 0.002
    assert transport.packets() == []


def test_with_gap_schedules_sysex_and_trailing_wait(
    transport: NativeConnection, monkeypatch: pytest.MonkeyPatch
) -> None:
    from meedee import _native

    deadlines: list[int] = []

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        for t, m, _ in scheduled:
            deadlines.append(t)
            if m is not None:
                send(m)

    monkeypatch.setattr(_native, "output_sequence", sequence)
    message = messages.SysEx(data=b"\x7d")
    with midi.Output() as out:
        out.send(events.with_gap(iter([message, message]), 0.1))
    assert deadlines == [0, 100_000_000, 200_000_000]
    assert transport.packets() == [b"\xf0\x7d\xf7"] * 2


@pytest.mark.parametrize(
    "delay", [-1, True, float("inf"), float("nan"), "3 Hz", "2 beats"]
)
def test_invalid_delay_rejects_the_whole_call_before_sending(
    transport: NativeConnection,
    delay: float | str,
) -> None:
    with midi.Output() as out:
        with pytest.raises((TypeError, ValueError, OverflowError)):
            out.send(messages.NoteOn(60), delay)
    assert transport.packets() == []


def test_cleanup_resets_only_changed_controls_and_tracks_raw_packets(
    transport: NativeConnection,
) -> None:
    with midi.Output() as out:
        out.send(b"\x90\x3c\x5a", messages.NoteOn(61, 0))
        out.send(
            messages.PitchBend(200),
            messages.ChannelPressure(40),
            messages.PolyPressure(60, 20),
        )
        for controller in [1, 64, 66, 67, 7, 10, 11]:
            out.send(messages.ControlChange(controller, 100))
        out.send(messages.ControlChange(1, 0))
        sent = transport.packets()
    assert len(sent) == 13
    assert transport.packets() == [
        b"\x80\x3c\x00",
        b"\xb0\x40\x00",
        b"\xb0\x42\x00",
        b"\xb0\x43\x00",
        b"\xe0\x00\x40",
        b"\xd0\x00",
        b"\xa0\x3c\x00",
    ]


def test_cleanup_orders_channels_and_notes_and_preserves_held_counts(
    transport: NativeConnection,
) -> None:
    with midi.Output() as out:
        out.send(
            messages.NoteOn(70, channel=3),
            messages.NoteOn(65, channel=1),
            messages.NoteOn(60, channel=1),
            messages.NoteOn(60, channel=1),
            messages.NoteOff(60, channel=1),
        )
        transport.packets()
    assert transport.packets() == [b"\x80\x3c\x00", b"\x80\x41\x00", b"\x82\x46\x00"]


def test_cleanup_runs_on_body_failure_and_always_closes_transport(
    transport: NativeConnection,
) -> None:
    transport.fail_release = True
    with pytest.raises(ExceptionGroup) as failure:
        with midi.Output() as out:
            out.send(messages.NoteOn(60))
            raise ValueError("body failed")
    assert [type(e) for e in failure.value.exceptions] == [ValueError, midi.SendError]
    assert transport.closed


def test_compound_failure_cleanup_uses_confirmed_prefix(
    transport: NativeConnection,
) -> None:
    transport.partial = 1
    with pytest.raises(midi.CompoundSendError):
        with midi.Output() as out:
            out.send(messages.ControlChange(1, 128, resolution=14))
    assert transport.packets() == [b"\xb0\x01\x01", b"\xb0\x01\x00"]
    assert transport.closed


@pytest.mark.parametrize("pattern", ["missing", "*"])
def test_port_patterns_require_one_match(
    transport: NativeConnection, pattern: str
) -> None:
    with pytest.raises(ValueError, match="expected one"):
        with midi.Output(port=pattern):
            pass
    assert transport.packets() == []


@pytest.mark.parametrize(
    "pattern", ["FLOW [0-9] MIDI", "[FO]LOW ? MIDI", "[!A-E]LOW*", "FLOW [!9] MIDI"]
)
def test_port_selection_supports_shell_character_classes(
    transport: NativeConnection,
    pattern: str,
) -> None:
    with midi.Output(port=pattern) as out:
        out.send(messages.ProgramChange(1))
    assert transport.packets() == [b"\xc0\x00"]


def test_empty_port_patterns_are_rejected_before_discovery() -> None:
    with pytest.raises(ValueError, match="port"):
        midi.Input(port="")
    with pytest.raises(ValueError, match="port"):
        midi.Output(port="")


def test_two_stage_connection_and_close_are_idempotent(
    transport: NativeConnection,
) -> None:
    client = midi.Output(channel=4)
    with client.connect(client.ports()[0]) as out:
        out.send(messages.NoteOn(60))
    out.close()
    assert transport.packets() == [b"\x93\x3c\x5a", b"\x83\x3c\x00"]
    with pytest.raises(midi.StateError):
        out.send(messages.NoteOn(60))


def test_bank_selection_inherits_output_channel(transport: NativeConnection) -> None:
    command = messages.ProgramChange(12, channel=7, bank_msb=0, bank_lsb=0)
    with midi.Output(channel=3) as out:
        out.send(command)
    assert command.channel == 7
    assert transport.packets() == [b"\xb2\x00\x00", b"\xb2\x20\x00", b"\xc2\x0b"]


def test_full_notes_and_rests_schedule_beats_and_release_before_next_attack(
    transport: NativeConnection,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native

    deadlines: list[int] = []

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        timeline = [(t, m) for t, m, _ in scheduled]
        for t, m in timeline:
            deadlines.append(t)
            if m is not None:
                send(m)

    monkeypatch.setattr(_native, "output_sequence", sequence)
    note = messages.Note(1, 60)
    with midi.Output(bpm=120, channel=2) as out:
        out.send(
            note,
            messages.Note(0.5, 60, 70),
            messages.Rest("1 beat"),
            messages.Note("250ms", 64, channel=3),
        )
    assert note.channel is None
    assert deadlines == [
        0,
        500_000_000,
        500_000_000,
        750_000_000,
        1_250_000_000,
        1_500_000_000,
        1_500_000_000,
    ]
    assert transport.packets() == [
        b"\x91\x3c\x5a",
        b"\x81\x3c\x00",
        b"\x91\x3c\x46",
        b"\x81\x3c\x00",
        b"\x91\x40\x5a",
        b"\x81\x40\x00",
    ]


def test_full_note_defaults_to_seconds_and_interrupt_cleanup_releases_it(
    transport: NativeConnection,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        timeline = [(t, m) for t, m, _ in scheduled]
        assert [t for t, _ in timeline] == [0, 500_000_000, 500_000_000]
        send(timeline[0][1])
        raise KeyboardInterrupt

    monkeypatch.setattr(_native, "output_sequence", sequence)
    with pytest.raises(KeyboardInterrupt):
        with midi.Output() as out:
            out.send(messages.Note(0.5, 60))
    assert transport.packets() == [b"\x90\x3c\x5a", b"\x80\x3c\x00"]
    assert transport.closed


def test_fractional_beat_deadlines_round_totals_without_drift(
    transport: NativeConnection,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native

    deadlines: list[int] = []

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        timeline = [(t, m) for t, m, _ in scheduled]
        deadlines.extend(t for t, _ in timeline)

    monkeypatch.setattr(_native, "output_sequence", sequence)
    with midi.Output(bpm=123) as out:
        out.send(*(messages.Note("1/3 beat", 60) for _ in range(123)))
    assert deadlines[-1] == 20_000_000_000
    assert deadlines[1] == 162_601_626


@pytest.mark.parametrize(
    "value", [messages.Note("1 beat", 60), messages.Rest("1 beat"), "1 beat"]
)
def test_beats_require_tempo_before_any_sends(
    transport: NativeConnection, value: object
) -> None:
    with midi.Output() as out:
        with pytest.raises(ValueError, match="require"):
            out.send(messages.NoteOn(60), value)  # ty: ignore[invalid-argument-type]
    assert transport.packets() == []


@pytest.mark.parametrize("bpm", [0, -1, float("inf"), float("nan"), True, "fast"])
def test_invalid_tempo_is_rejected(transport: NativeConnection, bpm: object) -> None:
    with pytest.raises((ValueError, TypeError)):
        midi.Output(bpm=bpm)  # ty: ignore[invalid-argument-type]


def test_bare_delays_follow_tempo_but_explicit_seconds_do_not(
    transport: NativeConnection,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native

    deadlines: list[int] = []

    def sequence(
        send: Callable[[object], None],
        scheduled: Iterable[tuple[int, object, bool]],
        **options: object,
    ) -> None:
        timeline = [(t, m) for t, m, _ in scheduled]
        deadlines.extend(t for t, _ in timeline)

    monkeypatch.setattr(_native, "output_sequence", sequence)
    with midi.Output(bpm=120) as out:
        out.send(1, "1", "500 ms", messages.Rest(1))
    assert deadlines == [2_000_000_000]


def test_full_note_releases_at_its_deadline_before_context_exit(
    transport: NativeConnection,
) -> None:
    with midi.Output() as out:
        out.send(messages.Note("2 ms", 60))
        assert transport.packets() == [b"\x90\x3c\x5a", b"\x80\x3c\x00"]
    assert transport.packets() == []


@pytest.mark.parametrize("channel", [None, 1])
def test_output_distinguishes_no_channel_from_explicit_channel_one(
    transport: NativeConnection, channel: int | None
) -> None:
    with midi.Output(channel=channel) as out:
        out.send(messages.ProgramChange(34, channel=3), messages.ProgramChange(81))
    assert transport.packets() == [
        bytes([0xC2 if channel is None else 0xC0, 33]),
        b"\xc0\x50",
    ]


def test_client_play_infers_origin_and_closes_reader_and_output(
    transport: NativeConnection, tmp_path: Path
) -> None:
    from meedee import archive, events

    clock = events.Clock(identity="capture")
    path = tmp_path / "rec.meedee"
    writer = archive.Writer(path)
    writer.write(clock)
    writer.write(archive.Stream(id="s", label=""))
    writer.write(
        (messages.NoteOn(60, channel=2)).replace(
            stream="s", timestamp=events.Timestamp(ticks=123_000_000_000, clock=clock)
        )
    )
    writer.finish()
    reader = archive.Reader(path)
    report = midi.Output().send(reader)
    assert report.sent_events == 1
    assert transport.closed
    assert transport.packets() == [b"\x91\x3c\x5a", b"\x81\x3c\x00"]
    with pytest.raises(ValueError, match="closed"):
        next(reader)


def test_client_play_accepts_empty_capture(transport: NativeConnection) -> None:
    report = midi.Output().send([])
    assert report.sent_events == 0
    assert not transport.closed


def test_playback_keeps_a_retained_output_available_for_reuse(
    transport: NativeConnection,
) -> None:
    from meedee import events

    timestamp = events.Timestamp(ticks=0, clock=events.Clock(identity="capture"))
    note = messages.NoteOn(60, channel=1).replace(timestamp=timestamp)
    with midi.Output() as output:
        assert output.send([note]).sent_events == 1
        assert not output.closed
        assert output.send([note.transpose(7)]).sent_events == 1
        output.send(messages.NoteOff(60), messages.NoteOff(67))
        assert not output.closed
    assert transport.closed
    assert transport.packets() == [
        b"\x90\x3c\x5a",
        b"\x90\x43\x5a",
        b"\x80\x3c\x00",
        b"\x80\x43\x00",
    ]


@pytest.mark.parametrize("connected", [False, True])
def test_output_is_automatically_collected_and_releases_notes(
    transport: NativeConnection,
    connected: bool,
) -> None:
    class ObservedOutput(midi.Output):
        pass

    def open_output() -> weakref.ReferenceType[ObservedOutput]:
        client = ObservedOutput()
        if connected:
            connection = client.__enter__()
            connection.send(messages.NoteOn(60))
        return weakref.ref(client)

    reference = open_output()
    gc.collect()
    assert reference() is None
    if connected:
        assert transport.closed
        assert transport.packets() == [b"\x90\x3c\x5a", b"\x80\x3c\x00"]


def test_playback_accepts_timing_without_source_metadata(
    transport: NativeConnection,
) -> None:
    from meedee import events

    note = messages.NoteOn(60, channel=1).replace(
        timestamp=events.Timestamp(ticks=100, clock=events.Clock(identity="score"))
    )
    assert midi.Output().send([note]).sent_events == 1
    assert note.stream is None
    assert transport.packets() == [b"\x90\x3c\x5a", b"\x80\x3c\x00"]


@pytest.mark.parametrize("run_in_subprocess", [False, True])
@pytest.mark.parametrize("declarations", [False, True])
def test_empty_playback_does_not_initialize_transport(
    monkeypatch: pytest.MonkeyPatch, run_in_subprocess: bool, declarations: bool
) -> None:
    from meedee import _native, archive, events

    def fail_initialization(*args: object, **kwargs: object) -> None:
        raise AssertionError("empty playback opened a MIDI client")

    monkeypatch.setattr(_native, "Output", fail_initialization)
    source = (
        [events.Clock(identity="empty"), archive.Stream(id="s", label="")]
        if declarations
        else []
    )
    output = midi.Output(run_in_subprocess=run_in_subprocess)
    report = output.send(iter(source))
    assert report.sent_events == 0
    assert not output.closed
    output.close()


def test_send_combines_notes_recorded_timing_and_immediate_messages(
    transport: NativeConnection,
) -> None:
    from meedee import events

    clock = events.Clock(identity="recording")
    notes = [
        messages.NoteOn(n, channel=1).replace(
            timestamp=events.Timestamp(ticks=t, clock=clock)
        )
        for n, t in [(62, 1_000_000), (64, 2_000_000)]
    ]
    with midi.Output() as output:
        report = output.send(
            messages.Note("1 ms", 60), notes, messages.NoteOff(62), metrics=True
        )
        assert report.sent_events == 5
        assert report.lateness is not None and report.lateness.count == 5
        assert not report.cancelled
        assert not callable(output)
        assert not hasattr(output, "play")
        assert transport.packets() == [
            b"\x90\x3c\x5a",
            b"\x80\x3c\x00",
            b"\x90\x3e\x5a",
            b"\x90\x40\x5a",
            b"\x80\x3e\x00",
        ]


@pytest.mark.parametrize("timed_first", [False, True])
def test_send_rejects_mixed_timing_and_retains_confirmed_progress(
    transport: NativeConnection, timed_first: bool
) -> None:
    from meedee import events

    plain = messages.NoteOn(60, channel=1)
    timed = plain.replace(
        timestamp=events.Timestamp(ticks=0, clock=events.Clock(identity="s"))
    )
    source = [timed, plain] if timed_first else [plain, timed]
    with midi.Output() as output:
        with pytest.raises(ValueError, match="mix") as caught:
            output.send(source)
        if timed_first:
            assert caught.value.report.sent_events == 1
            assert transport.packets() == [b"\x90\x3c\x5a"]
        else:
            assert transport.packets() == []


def test_cancelled_send_does_not_consume_source_or_open_ports(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _native, events

    def no_ports(**settings: object) -> None:
        raise AssertionError("cancelled send opened a client")

    def source() -> Iterator[messages.Message]:
        raise AssertionError("cancelled send consumed its source")
        yield messages.NoteOn(60)

    monkeypatch.setattr(_native, "Output", no_ports)
    cancel = events.Cancellation()
    cancel.cancel()
    report = midi.Output().send(source(), cancel=cancel, metrics=True)
    assert report.cancelled and report.sent_events == 0
    assert report.lateness is not None and report.lateness.count == 0


def test_send_failure_keeps_original_type_and_report(
    transport: NativeConnection,
) -> None:
    transport.fail_release = True
    output = midi.Output()
    with pytest.raises(midi.SendError) as caught:
        output.send(messages.Note(0, 60))
    assert caught.value.report.sent_events == 1
    assert caught.value.completed_events == 1
    transport.fail_release = False
    output.close()


def test_scalar_note_report_counts_rounded_deadlines(
    transport: NativeConnection,
) -> None:
    with midi.Output(bpm=120_000_000_000) as output:
        report = output.send(messages.Note(1, 60))
        assert report.sent_events == 2
        assert report.rounded_events == 1


def test_root_api_sends_a_full_note(transport: NativeConnection) -> None:
    import meedee as md

    report = md.Output().send(md.Note("500 ms", 60))
    assert report.sent_events == 2
    assert transport.packets() == [b"\x90\x3c\x5a", b"\x80\x3c\x00"]
    assert transport.closed
