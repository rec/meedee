"""Source-local timing, typed adapters, recording and supervised cancellation."""

import gc
import weakref
from collections.abc import Callable
from io import BytesIO
from pathlib import Path
from threading import Thread

import pytest

from meedee import _native, archive, events, messages, midi, ump


def clock() -> events.Clock:
    return events.Clock(
        identity="c", backend=midi.Backend.CoreMidi, mode=midi.TimestampMode.Absolute
    )


def packet(
    data: bytes | midi.UmpPacket, sequence: int, ticks: int
) -> events.ReceivedPacket:
    return events.ReceivedPacket(
        timestamp_ns=ticks, clock=clock(), packet=data, sequence=sequence
    )


def event(ticks: int) -> messages.Message:
    return (messages.NoteOn(channel=1, note=60, velocity=100)).replace(
        stream="s", timestamp=events.Timestamp(ticks=ticks, clock=clock())
    )


def test_native_packet_spans_use_completion_time_without_realtime_contamination() -> (
    None
):
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    assert decoder.feed(packet(b"\x90\x3c", 0, 10)) == []
    result = decoder.feed(packet(b"\xf8\x64\x3d\x65", 1, 20))
    assert len(result) == 3
    first, note, running = result
    assert isinstance(first, messages.Message) and first.span is not None
    assert first.span.first_packet == first.span.last_packet == 1
    assert (
        isinstance(note, messages.Message)
        and note.timestamp is not None
        and note.span is not None
    )
    assert note.timestamp.ticks == 20
    assert (
        note.span.first_timestamp is not None and note.span.first_timestamp.ticks == 10
    )
    assert (note.span.first_packet, note.span.last_packet) == (0, 1)
    assert (
        isinstance(running, messages.Message)
        and running.span is not None
        and running.span.first_packet == 1
    )
    decoder.reset()
    assert decoder.feed(packet(b"\x90\x3c", 2, 30)) == []
    lost = decoder.feed(packet(b"\x64", 4, 40))
    assert (
        isinstance(lost[0], events.StreamDiagnostic)
        and lost[0].kind == "loss"
        and lost[0].lost == 1
    )
    assert not any(isinstance(v, messages.Message) for v in lost)


def test_ump_framing_and_typed_send_never_downgrade_or_send_opaque_values() -> None:
    decoder = events.PacketDecoder(stream="u", protocol=midi.Protocol.Ump)
    assert decoder.feed(packet(midi.UmpPacket(words=(0x40903C00,)), 0, 1)) == []
    value = decoder.feed(packet(midi.UmpPacket(words=(0xFFFF0000,)), 1, 2))[0]
    assert isinstance(value, messages.Message) and isinstance(value, ump.Midi2NoteOn)
    assert value.span is not None and value.span.first_packet == 0
    with pytest.raises(midi.CapabilityError):
        _native.prepare_output(midi.Protocol.Midi1, value)
    opaque = ump.decode((0x60000000,), preserve_unknown=True)
    with pytest.raises(midi.CapabilityError, match="opaque"):
        _native.prepare_output(midi.Protocol.Ump, opaque)


def test_clock_mapping_and_rounding_require_explicit_policy() -> None:
    fractional = events.Clock(
        identity="fraction", tick_numerator=1, tick_denominator=3, origin="test"
    )
    a = event(0).replace(timestamp=events.Timestamp(ticks=0, clock=fractional))
    b = a.replace(timestamp=events.Timestamp(ticks=1, clock=fractional))
    with pytest.raises(ValueError, match="unmapped"):
        events.PlaybackPlan(origin=events.Timestamp(ticks=0, clock=clock())).deadline(a)
    mapping = events.ClockMapping(source=fractional, target=clock())
    planner = events.PlaybackPlan(
        origin=events.Timestamp(ticks=0, clock=clock()), mappings=[mapping]
    )
    assert planner.deadline(a).delay_ns == 0
    with pytest.raises(ValueError, match="rounding"):
        planner.deadline(b)
    planner = events.PlaybackPlan(
        origin=events.Timestamp(ticks=0, clock=fractional), allow_rounding=True
    )
    planner.deadline(a)
    assert planner.deadline(b).delay_ns == 333_333_333


class Sink:
    def __init__(self, fail_at: int | None = None) -> None:
        self.sent: list[messages.Message] = []
        self.fail_at = fail_at

    def __call__(self, message: messages.Message) -> None:
        if len(self.sent) == self.fail_at:
            raise midi.SendError("device failed")
        self.sent.append(message)


def test_playback_cancellation_and_partial_failure_do_not_send_cleanup() -> None:
    sink = Sink(fail_at=1)
    with pytest.raises(events.PlaybackError) as error:
        _native.play(
            sink,
            [event(0), event(0), event(0)],
            origin=events.Timestamp(ticks=0, clock=clock()),
        )
    assert error.value.completed_events == 1 and len(sink.sent) == 1
    assert isinstance(error.value.__cause__, midi.SendError)
    cancel = events.Cancellation()
    cancel.cancel()
    report = _native.play(
        sink, [event(0)], origin=events.Timestamp(ticks=0, clock=clock()), cancel=cancel
    )
    assert report.cancelled and report.sent_events == 0
    sink = Sink()
    cancel = events.Cancellation()
    reports: list[events.PlaybackReport] = []
    worker = Thread(
        target=lambda: reports.append(
            _native.play(
                sink,
                [event(0), event(60_000_000_000)],
                origin=events.Timestamp(ticks=0, clock=clock()),
                cancel=cancel,
            )
        )
    )
    worker.start()
    cancel.cancel()
    worker.join(2)
    assert not worker.is_alive() and reports[0].cancelled
    assert len(sink.sent) <= 1


def test_recording_preserves_verified_events_and_reports_observed_loss(
    tmp_path: Path,
) -> None:
    path = tmp_path / "capture.meedee"
    recorder = archive.Recorder(path, clock=clock(), stream="s")
    assert recorder.record(event(1))
    report = recorder.stop()
    assert report.complete and report.gap_records == 0
    reader = archive.Reader(path)
    assert len(list(reader)) == 3 and reader.complete
    recorder = archive.Recorder(path, clock=clock(), stream="s")
    assert not recorder.record(archive.Gap(stream="s", lost=1, reason="lost input"))
    assert not recorder.stop().complete
    with pytest.raises(OSError, match="incomplete"):
        list(archive.Reader(path))
    recorder = archive.Recorder(path, clock=clock(), stream="s", best_effort=True)
    assert recorder.record(archive.Gap(stream="s", lost=1, reason="lost input"))
    report = recorder.stop()
    assert report.complete and report.gap_records == 1
    assert len(list(archive.Reader(path))) == 3


def test_replay_rejects_loss_records_and_missing_times() -> None:
    sink = Sink()
    with pytest.raises(events.PlaybackError, match="timestamp"):
        _native.play(
            sink,
            [event(0).replace(timestamp=None)],
            origin=events.Timestamp(ticks=0, clock=clock()),
        )
    stream = BytesIO()
    writer = archive.Writer(stream)
    writer.write(clock())
    writer.write(archive.Stream(id="s", label=""))
    writer.write(archive.Gap(stream="s", lost=1, reason="loss"))
    writer.finish()
    with pytest.raises(events.PlaybackError, match="gap"):
        _native.play(
            sink,
            archive.Reader(BytesIO(stream.getvalue())),
            origin=events.Timestamp(ticks=0, clock=clock()),
        )


@pytest.mark.parametrize("lost", [False, True])
def test_typed_capture_reports_a_final_loss_even_without_a_following_packet(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, lost: bool
) -> None:
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []
    capture_clock = clock()

    class Connection:
        closed = False
        worker_failure = None
        dropped_packets = 0
        clock = capture_clock

        def close(self) -> None:
            self.closed = True

    connection = Connection()

    class Input:
        def __init__(self, **settings: object) -> None:
            pass

        def create_virtual(
            self,
            name: str,
            callback: Callable[[events.ReceivedPacket], object],
            identity: str,
        ) -> Connection:
            callbacks.append(callback)
            return connection

    monkeypatch.setattr(_native, "Input", Input)
    receiver = midi.Input(isolated=False).create_virtual("test-only")
    path = tmp_path / "typed.meedee"
    receiver.start_recording(path)
    callback = callbacks[0]
    callback(packet(b"\x90\x3c\x64", 0, 1))
    value = receiver.receive(timeout=0)
    assert isinstance(value, messages.Message) and value == event(1)
    connection.dropped_packets = int(lost)
    report = receiver.stop_recording()
    assert report.complete is not lost
    receiver.close()
    reader = archive.Reader(path, recover=lost)
    values = list(reader)
    assert len([v for v in values if isinstance(v, messages.Message)]) == 1
    assert reader.complete is not lost
    reader.close()


@pytest.mark.parametrize("raw", [False, True])
@pytest.mark.parametrize("callback_mode", [False, True])
def test_unified_input_delivers_decoded_messages_or_unchanged_packets(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    raw: bool,
    callback_mode: bool,
) -> None:
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []
    observed: list[object] = []
    capture_clock = clock()

    class Connection:
        closed = False
        worker_failure = None
        dropped_packets = 0
        clock = capture_clock

        def close(self) -> None:
            self.closed = True

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def create_virtual(
            self,
            name: str,
            callback: Callable[[events.ReceivedPacket], object],
            identity: str,
        ) -> Connection:
            callbacks.append(callback)
            return Connection()

    monkeypatch.setattr(_native, "Input", Client)
    client = midi.Input(raw=raw, isolated=False)
    with client.create_virtual(
        "fixture", observed.append if callback_mode else None
    ) as receiver:
        first = packet(b"\xf0\x7d\x01", 0, 100)
        last = packet(b"\x02\xf7", 1, 200)
        callbacks[0](first)
        callbacks[0](last)
        if callback_mode:
            with pytest.raises(midi.StateError, match="callback"):
                receiver.receive(timeout=0)
        else:
            while (item := receiver.receive(timeout=0)) is not None:
                observed.append(item)
        if raw:
            assert observed == [first, last]
            with pytest.raises(midi.StateError, match="decoded"):
                receiver.start_recording(tmp_path / "raw.meedee")
            assert not (tmp_path / "raw.meedee").exists()
        else:
            assert len(observed) == 1
            message = observed[0]
            assert isinstance(message, messages.Message)
            assert message == messages.SysEx(b"\x7d\x01\x02")
            assert message.timestamp is not None and message.timestamp.ticks == 200
    assert receiver.closed


def test_raw_receive_queue_is_bounded_and_drains_before_reporting_overflow(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []

    class Connection:
        closed = False
        dropped_packets = 0
        worker_failure = None

        def close(self) -> None:
            self.closed = True

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def create_virtual(
            self,
            name: str,
            callback: Callable[[events.ReceivedPacket], object],
            identity: str,
        ) -> Connection:
            callbacks.append(callback)
            return Connection()

    monkeypatch.setattr(_native, "Input", Client)
    with midi.Input(raw=True, isolated=False, event_limit=1).create_virtual(
        "fixture"
    ) as receiver:
        first = packet(b"\xf8", 0, 100)
        callbacks[0](first)
        callbacks[0](packet(b"\xfa", 1, 200))
        assert receiver.receive(timeout=0) == first
        with pytest.raises(midi.ResourceError, match="overflowed"):
            receiver.receive(timeout=0)
        assert isinstance(receiver.failure, midi.ResourceError)


@pytest.fixture
def iterable_input(
    monkeypatch: pytest.MonkeyPatch,
) -> list[Callable[[events.ReceivedPacket], object]]:
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []
    capture_clock = clock()
    port = midi.Port._from_wire(
        "fixture", 0, 0, "Keyboard", midi.Backend.CoreMidi, 1, True
    )

    class Connection:
        closed = False
        worker_failure = None
        dropped_packets = 0
        clock = capture_clock

        def close(self) -> None:
            self.closed = True

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[midi.Port]:
            return [port]

        def connect(
            self,
            port: midi.Port,
            callback: Callable[[events.ReceivedPacket], object],
            identity: str,
        ) -> Connection:
            callbacks.append(callback)
            callback(packet(b"\x90\x3c\x64", 0, 1))
            return Connection()

    monkeypatch.setattr(_native, "Input", Client)
    return callbacks


def test_direct_input_iteration_opens_first_port_and_closes_on_generator_close(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
) -> None:
    client = midi.Input(isolated=False)
    iterator = client.with_errors()
    assert next(iterator) == messages.NoteOn(60, 100, 1)
    iterator.close()
    # A second context can open after the iterator releases its connection.
    with client as receiver:
        assert next(iter(receiver)) == messages.NoteOn(60, 100, 1)
    assert receiver.closed


@pytest.mark.parametrize("channels", [1, [1, 3], (1, 3), {1, 3}, []])
def test_input_channel_selection_filters_messages_but_keeps_diagnostics(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    channels: int | list[int] | tuple[int, ...] | set[int],
) -> None:
    with midi.Input(channel=channels, isolated=False) as receiver:
        iterable_input[0](packet(b"\x91\x3d\x64\x92\x3e\x64\xf8", 1, 2))
        iterable_input[0](packet(b"\x90\x3f\x64", 3, 3))
        receiver.close()
        items = list(receiver.with_errors())
    notes = [i.note for i in items if isinstance(i, messages.NoteOn)]
    assert notes == ([60, 63] if channels == 1 else [60, 62, 63] if channels else [])
    assert (
        len(
            [
                i
                for i in items
                if isinstance(i, events.StreamDiagnostic) and i.kind == "loss"
            ]
        )
        == 1
    )
    assert all(isinstance(i, (messages.NoteOn, events.StreamDiagnostic)) for i in items)


@pytest.mark.parametrize("include_errors", [False, True])
def test_input_iteration_reports_diagnostics_and_continues(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    capsys: pytest.CaptureFixture[str],
    include_errors: bool,
) -> None:
    with midi.Input(isolated=False) as receiver:
        iterable_input[0](packet(b"\x90\x3d\x64", 2, 2))
        receiver.close()
        items = list(receiver.with_errors() if include_errors else receiver)
    assert [i.note for i in items if isinstance(i, messages.NoteOn)] == [60, 61]
    assert any(isinstance(i, events.StreamDiagnostic) for i in items) is include_errors
    assert bool(capsys.readouterr().err) is not include_errors


def test_input_iteration_drains_then_reports_terminal_overflow(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    capsys: pytest.CaptureFixture[str],
) -> None:
    with midi.Input(isolated=False, event_limit=1) as receiver:
        iterable_input[0](packet(b"\x90\x3d\x64", 1, 2))
        assert list(receiver) == [messages.NoteOn(60, 100, 1)]
    assert "overflowed" in capsys.readouterr().err


@pytest.mark.parametrize("channel", [0, 17, [1, 17]])
def test_input_rejects_invalid_channel_selection(channel: int | list[int]) -> None:
    with pytest.raises(ValueError, match="1..16"):
        midi.Input(channel=channel, isolated=False)


def test_raw_input_rejects_channel_selection() -> None:
    with pytest.raises(ValueError, match="decoded"):
        midi.Input(raw=True, channel=1, isolated=False)


def test_channel_selection_applies_to_callbacks_and_recordings(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], tmp_path: Path
) -> None:
    observed: list[messages.Message | events.StreamDiagnostic] = []
    client = midi.Input(channel=[1], isolated=False)
    path = tmp_path / "selected.meedee"
    with client.connect(client.ports()[0], observed.append) as receiver:
        receiver.start_recording(path)
        iterable_input[0](packet(b"\x91\x3d\x64\x90\x3e\x64\xf8", 1, 2))
        report = receiver.stop_recording()
    assert report.complete
    assert [i for i in observed if isinstance(i, messages.Message)] == [
        messages.NoteOn(60, 100, 1),
        messages.NoteOn(62, 100, 1),
    ]
    reader = archive.Reader(path)
    try:
        assert [i for i in reader if isinstance(i, messages.Message)] == [
            messages.NoteOn(62, 100, 1)
        ]
    finally:
        reader.close()


def test_record_returns_memory_capture_and_file_round_trip(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], tmp_path: Path
) -> None:
    path = tmp_path / "rec.meedee"
    client = midi.Input(isolated=False)
    records = client.record(0, output=path)
    assert [r for r in records if isinstance(r, messages.Message)] == [
        messages.NoteOn(60, 100, 1)
    ]
    original = path.read_bytes()
    with pytest.raises(FileExistsError):
        client.record(0, output=path)
    assert path.read_bytes() == original
    client.record("0 seconds", output=path, overwrite=True)
    reader = archive.Reader(path)
    try:
        assert [r for r in reader if isinstance(r, messages.Message)] == [
            messages.NoteOn(60, 100, 1)
        ]
        assert reader.complete
    finally:
        reader.close()


@pytest.mark.parametrize("connected", [False, True])
def test_input_is_automatically_collected_after_scope_exit(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    connected: bool,
) -> None:
    class ObservedInput(midi.Input):
        pass

    def open_input() -> weakref.ReferenceType[ObservedInput]:
        client = ObservedInput(isolated=False)
        if connected:
            client.__enter__()
        return weakref.ref(client)

    reference = open_input()
    # The test fixture stores callbacks externally; a real transport owns them.
    iterable_input.clear()
    gc.collect()
    assert reference() is None


@pytest.mark.parametrize("duration", [-1, float("inf"), float("nan")])
def test_record_rejects_invalid_duration_before_opening(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], duration: float
) -> None:
    with pytest.raises(ValueError, match="duration"):
        midi.Input(isolated=False).record(duration)
    assert not iterable_input


def test_record_uses_duration_deadline_and_retains_terminal_failure(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _clients

    ticks = iter([100.0, 100.0, 100.0, 110.0])
    monkeypatch.setattr(_clients, "monotonic", lambda: next(ticks, 110.0))
    records = midi.Input(isolated=False).record(10)
    assert len([r for r in records if isinstance(r, messages.Message)]) == 1

    def failed_receive(self: midi.InputConnection, *, timeout: float) -> None:
        raise midi.ResourceError("capture overflow")

    ticks = iter([100.0, 100.0])
    monkeypatch.setattr(midi.InputConnection, "receive", failed_receive)
    records = midi.Input(isolated=False).record(10)
    failures = [r for r in records if isinstance(r, archive.Discontinuity)]
    assert len(failures) == 1
    assert failures[0].reason == "capture overflow"
