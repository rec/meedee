"""Source-local timing, typed adapters, recording and supervised cancellation."""

import gc
import weakref
from collections.abc import Callable, Iterator
from io import BytesIO
from pathlib import Path
from threading import Thread
from typing import Self

import pytest

from meedee import _native, archive, events, messages, midi, ump


def test_unopened_diagnostics_remain_readable_without_native_initialization(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def unexpected(**settings: object) -> None:
        raise AssertionError("diagnostics initialized native MIDI")

    monkeypatch.setattr(_native, "Input", unexpected)
    monkeypatch.setattr(_native, "Output", unexpected)
    source = midi.Input()
    output = midi.Output()
    assert not source.closed and not output.closed
    assert source.dropped_packets == 0
    assert source.failure is None
    assert source.last_message_time is None and source.recording_report is None
    source.close()
    output.close()
    assert source.closed and output.closed
    assert source.dropped_packets == 0 and source.failure is None
    assert source.last_message_time is None and source.recording_report is None
    for field in ("stream", "clock"):
        with pytest.raises(midi.StateError):
            getattr(source, field)


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
    assert isinstance(value, messages.Message) and isinstance(value, messages.NoteOn)
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


class Sink(midi.Output):
    def __init__(self, fail_at: int | None = None) -> None:
        super().__init__()
        self.sent: list[messages.Message] = []
        self.fail_at = fail_at

    def _send(self, message: messages.Message) -> None:
        if len(self.sent) == self.fail_at:
            raise midi.SendError("device failed")
        self.sent.append(message)

    def __enter__(self) -> Self:
        return self


def test_playback_cancellation_and_partial_failure_do_not_send_cleanup() -> None:
    sink = Sink(fail_at=1)
    with pytest.raises(midi.SendError) as error:
        sink.send(
            [event(0), event(0), event(0)],
            origin=events.Timestamp(ticks=0, clock=clock()),
        )
    assert error.value.completed_events == 1 and len(sink.sent) == 1
    assert isinstance(error.value, midi.SendError)
    cancel = events.Cancellation()
    cancel.cancel()
    report = sink.send(
        [event(0)], origin=events.Timestamp(ticks=0, clock=clock()), cancel=cancel
    )
    assert report.cancelled and report.sent_events == 0
    sink = Sink()
    cancel = events.Cancellation()
    reports: list[events.PlaybackReport] = []
    worker = Thread(
        target=lambda: reports.append(
            sink.send(
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
    assert recorder.records is None
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


def test_memory_recorder_retains_diagnostics_and_stops_admission() -> None:
    recorder = archive.Recorder(clock=clock(), stream="s")
    assert recorder.records is None
    assert recorder.record(event(1))
    assert recorder.record(archive.Gap(stream="s", lost=1, reason="lost input"))
    assert recorder.record(archive.Discontinuity(stream="s", reason="clock changed"))
    assert recorder.record(event(2))
    report = recorder.stop()
    assert not report.complete and report.failure == "lost input"
    assert report.records == 6 and report.gap_records == 1
    records = recorder.records
    assert records is not None
    assert [r for r in records if isinstance(r, messages.Message)] == [
        event(1),
        event(2),
    ]
    assert not recorder.record(event(3))
    assert recorder.stop().records == report.records
    assert recorder.records == records


def test_memory_recording_stops_without_closing_input(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
) -> None:
    with midi.Input() as receiver:
        recorder = receiver.start()
        iterable_input[0](packet(b"\x90\x3e\x64", 1, 2))
        report = receiver.stop()
        assert report.complete and receiver.recording_report is report
        assert not receiver.closed
        records = recorder.records
        assert records is not None
        assert [r.note for r in records if isinstance(r, messages.NoteOn)] == [60, 62]
        iterable_input[0](packet(b"\x90\x40\x64", 2, 3))
        assert recorder.records == records
        assert receiver.receive(timeout=0) == messages.NoteOn(60, 100, 1)
        # The receive queue remains independent of recording.
        assert receiver.receive(timeout=0) == messages.NoteOn(62, 100, 1)
        assert receiver.receive(timeout=0) == messages.NoteOn(64, 100, 1)


def test_memory_capture_preserves_buffered_prefix_after_receive_overflow(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
) -> None:
    receiver = midi.Input(event_limit=1)
    receiver.connect(receiver.ports()[0])
    iterable_input[0](packet(b"\x90\x3e\x64", 1, 2))
    records = receiver.record(0)
    assert [r.note for r in records if isinstance(r, messages.NoteOn)] == [60]
    failures = [r for r in records if isinstance(r, archive.Discontinuity)]
    assert len(failures) == 1 and "overflow" in failures[0].reason
    assert receiver.recording_report is not None
    assert not receiver.recording_report.complete


def test_memory_capture_declares_changed_clocks_and_retains_later_events(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], tmp_path: Path
) -> None:
    receiver = midi.Input()
    recorder = receiver.start()
    changed = events.Clock(
        identity="changed",
        backend=midi.Backend.CoreMidi,
        mode=midi.TimestampMode.Absolute,
    )
    iterable_input[0](
        events.ReceivedPacket(
            timestamp_ns=2, clock=changed, packet=b"\x90\x3e\x64", sequence=1
        )
    )
    report = receiver.stop()
    assert not report.complete
    records = recorder.records
    assert records is not None
    assert [r.identity for r in records if isinstance(r, events.Clock)] == [
        "c",
        "changed",
    ]
    assert [r.note for r in records if isinstance(r, messages.NoteOn)] == [60, 62]
    assert len([r for r in records if isinstance(r, archive.Discontinuity)]) == 1
    path = tmp_path / "changed.meedee"
    with path.open("wb") as destination:
        writer = archive.Writer(destination)
        for r in records:
            writer.write(r)
        writer.finish()
    reader = archive.Reader(path)
    try:
        assert list(reader) == records
    finally:
        reader.close()
    receiver.close()


def test_replay_rejects_loss_records_and_missing_times() -> None:
    sink = Sink()
    with pytest.raises(ValueError, match="mix"):
        sink.send(
            [event(0), event(0).replace(timestamp=None)],
            origin=events.Timestamp(ticks=0, clock=clock()),
        )
    stream = BytesIO()
    writer = archive.Writer(stream)
    writer.write(clock())
    writer.write(archive.Stream(id="s", label=""))
    writer.write(archive.Gap(stream="s", lost=1, reason="loss"))
    writer.finish()
    with pytest.raises(ValueError, match="gap"):
        sink.send(
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
        last_message_time = None
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
    receiver = midi.Input(run_in_subprocess=False).create_virtual("test-only")
    path = tmp_path / "typed.meedee"
    receiver.start(path)
    callback = callbacks[0]
    callback(packet(b"\x90\x3c\x64", 0, 1))
    value = receiver.receive(timeout=0)
    assert isinstance(value, messages.Message) and value == event(1)
    connection.dropped_packets = int(lost)
    report = receiver.stop()
    assert report.complete is not lost
    receiver.close()
    assert receiver.closed
    assert receiver.clock == capture_clock
    assert receiver.stream
    assert receiver.dropped_packets == int(lost)
    assert receiver.recording_report == report
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
        last_message_time = None
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
    client = midi.Input(raw=raw, run_in_subprocess=False)
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
                receiver.start(tmp_path / "raw.meedee")
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

    capture_clock = clock()

    class Connection:
        clock = capture_clock
        closed = False
        dropped_packets = 0
        last_message_time = None
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
    with midi.Input(raw=True, run_in_subprocess=False, event_limit=1).create_virtual(
        "fixture"
    ) as receiver:
        first = packet(b"\xf8", 0, 100)
        callbacks[0](first)
        callbacks[0](packet(b"\xfa", 1, 200))
        assert receiver.closed
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
        last_message_time = None
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
    client = midi.Input(run_in_subprocess=False)
    iterator = client.with_errors()
    assert next(iterator) == messages.NoteOn(60, 100, 1)
    iterator.close()
    assert client.closed
    with pytest.raises(midi.StateError, match="closed"):
        client.__enter__()


def test_close_failure_retains_original_error_and_input_metadata(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    capture_clock = clock()
    failure = midi.MidiError("native close failed")

    class Connection:
        clock = capture_clock
        closed = False
        dropped_packets = 3
        worker_failure = None
        last_message_time = None

        def close(self) -> None:
            raise failure

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def create_virtual(
            self, name: str, callback: object, identity: str
        ) -> Connection:
            return Connection()

    monkeypatch.setattr(_native, "Input", Client)
    client = midi.Input(raw=True).create_virtual("fixture")
    stream = client.stream
    with pytest.raises(midi.MidiError) as caught:
        client.close()
    assert caught.value is failure and client.failure is failure
    assert client.closed and client.dropped_packets == 3
    assert client.stream == stream and client.clock == capture_clock
    client.close()
    with pytest.raises(midi.StateError, match="closed"):
        client.__enter__()


@pytest.mark.parametrize("channels", [1, [1, 3], (1, 3), {1, 3}, []])
def test_input_channel_selection_filters_messages_but_keeps_diagnostics(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    channels: int | list[int] | tuple[int, ...] | set[int],
) -> None:
    with midi.Input(channel=channels, run_in_subprocess=False) as receiver:
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
    assert any(isinstance(i, messages.TimingClock) for i in items)
    assert all(
        isinstance(i, (messages.NoteOn, messages.TimingClock, events.StreamDiagnostic))
        for i in items
    )


@pytest.mark.parametrize("include_errors", [False, True])
def test_input_iteration_reports_diagnostics_and_continues(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    capsys: pytest.CaptureFixture[str],
    include_errors: bool,
) -> None:
    with midi.Input(run_in_subprocess=False) as receiver:
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
    with midi.Input(run_in_subprocess=False, event_limit=1) as receiver:
        iterable_input[0](packet(b"\x90\x3d\x64", 1, 2))
        assert list(receiver) == [messages.NoteOn(60, 100, 1)]
    assert "overflowed" in capsys.readouterr().err


@pytest.mark.parametrize("channel", [0, 17, [1, 17]])
def test_input_rejects_invalid_channel_selection(channel: int | list[int]) -> None:
    with pytest.raises(ValueError, match="1..16"):
        midi.Input(channel=channel, run_in_subprocess=False)


def test_raw_input_rejects_channel_selection() -> None:
    with pytest.raises(ValueError, match="decoded"):
        midi.Input(raw=True, channel=1, run_in_subprocess=False)


def test_channel_selection_applies_to_callbacks_and_recordings(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], tmp_path: Path
) -> None:
    observed: list[messages.Message | events.StreamDiagnostic] = []
    client = midi.Input(channel=[1], run_in_subprocess=False)
    path = tmp_path / "selected.meedee"
    with client.connect(client.ports()[0], observed.append) as receiver:
        receiver.start(path)
        iterable_input[0](packet(b"\x91\x3d\x64\x90\x3e\x64\xf8", 1, 2))
        report = receiver.stop()
    assert report.complete
    assert [i for i in observed if isinstance(i, messages.Message)] == [
        messages.NoteOn(60, 100, 1),
        messages.NoteOn(62, 100, 1),
        messages.TimingClock(),
    ]
    reader = archive.Reader(path)
    try:
        assert [i for i in reader if isinstance(i, messages.Message)] == [
            messages.NoteOn(62, 100, 1),
            messages.TimingClock(),
        ]
    finally:
        reader.close()


def test_record_returns_memory_capture_and_file_round_trip(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], tmp_path: Path
) -> None:
    path = tmp_path / "rec.meedee"
    client = midi.Input(run_in_subprocess=False)
    records = client.record(0, output=path)
    assert [r for r in records if isinstance(r, messages.Message)] == [
        messages.NoteOn(60, 100, 1)
    ]
    original = path.read_bytes()
    with pytest.raises(FileExistsError):
        midi.Input().record(0, output=path)
    assert path.read_bytes() == original
    midi.Input().record("0 seconds", output=path, overwrite=True)
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
        client = ObservedInput(run_in_subprocess=False)
        if connected:
            client.__enter__()
        return weakref.ref(client)

    reference = open_input()
    # Even externally retained transport callbacks must not keep the owner alive.
    gc.collect()
    assert reference() is None


@pytest.mark.parametrize("duration", [-1, float("inf"), float("nan")])
def test_record_rejects_invalid_duration_before_opening(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], duration: float
) -> None:
    with pytest.raises(ValueError, match="duration"):
        midi.Input(run_in_subprocess=False).record(duration)
    assert not iterable_input


def test_record_uses_duration_deadline_and_retains_terminal_failure(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from meedee import _clients

    ticks = iter([100.0, 100.0, 100.0, 110.0])
    monkeypatch.setattr(_clients, "monotonic", lambda: next(ticks, 110.0))
    records = midi.Input(run_in_subprocess=False).record(10)
    assert len([r for r in records if isinstance(r, messages.Message)]) == 1

    def failed_receive(self: midi.Input, *, timeout: float) -> None:
        raise midi.ResourceError("capture overflow")

    ticks = iter([100.0, 100.0, 100.0])
    monkeypatch.setattr(midi.Input, "receive", failed_receive)
    records = midi.Input(run_in_subprocess=False).record(10)
    failures = [r for r in records if isinstance(r, archive.Discontinuity)]
    assert len(failures) == 1
    assert failures[0].reason == "capture overflow"


@pytest.mark.parametrize("pattern", ["Key", "Key*"])
def test_input_pattern_connects_same_resource_and_close_is_terminal(
    iterable_input: list[Callable[[events.ReceivedPacket], object]], pattern: str
) -> None:
    client = midi.Input(port=pattern)
    with client as receiver:
        assert receiver is client
        assert receiver.receive(timeout=0) == messages.NoteOn(60, 100, 1)
    with pytest.raises(midi.StateError, match="closed"):
        client.__enter__()


def test_closed_input_releases_native_handles_but_retains_final_diagnostics(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native_type = _native.Input
    original_connect = native_type.connect
    handles: list[weakref.ReferenceType[object]] = []

    def connect(client: _native.Input, *args: object) -> object:
        connection = original_connect(client, *args)
        handles.extend([weakref.ref(client), weakref.ref(connection)])
        return connection

    monkeypatch.setattr(native_type, "connect", connect)
    client = midi.Input()
    client.__enter__()
    captured_clock = client.clock
    client.close()
    gc.collect()
    assert all(h() is None for h in handles)
    assert client.clock == captured_clock
    assert client.dropped_packets == 0
    assert client.closed


def test_cancelled_receive_preserves_data_and_filtered_packets_update_activity(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
) -> None:
    from time import monotonic

    source = midi.Input(channel=2)
    source.__enter__()
    callback = iterable_input[-1]
    before = monotonic()
    callback(packet(b"\x90\x3c\x64", 1, 1))
    activity = source.last_message_time
    assert activity is not None and before <= activity <= monotonic()
    assert source.receive(timeout=0) is None
    callback(packet(b"\x91\x3e\x64", 2, 2))
    cancel = events.Cancellation()
    cancel.cancel()
    with pytest.raises(InterruptedError):
        source.receive(timeout=0, cancel=cancel)
    assert not source.closed and source.failure is None
    value = source.receive(timeout=0)
    assert isinstance(value, messages.NoteOn) and value.note == 62
    source.close()
    final_activity = source.last_message_time
    callback(packet(b"\x91\x40\x64", 3, 3))
    assert source.last_message_time == final_activity
    with pytest.raises(midi.StateError, match="closed"):
        source.receive(timeout=0)
    assert list(source.with_errors()) == []


def test_stopping_recording_can_be_retried_after_timeout_and_input_close(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    original = archive.Recorder

    class Recorder:
        def __init__(self, destination: Path, **settings: object) -> None:
            self.inner = original(destination, **settings)
            self.attempts = 0

        def record(self, value: object) -> bool:
            return self.inner.record(value)

        def fail(self, reason: str) -> None:
            self.inner.fail(reason)

        def stop(self, *, timeout: float = 5.0) -> archive.RecordingReport:
            self.attempts += 1
            if self.attempts <= 2:
                raise TimeoutError("writer remains busy")
            return self.inner.stop(timeout=timeout)

    monkeypatch.setattr(archive, "Recorder", Recorder)
    source = midi.Input()
    source.__enter__()
    recorder = source.start(tmp_path / "retry.meedee")
    with pytest.raises(TimeoutError):
        source.stop(timeout=0.001)
    assert source.recording_report is None
    with pytest.raises(midi.StateError, match="active"):
        source.start(tmp_path / "second.meedee")
    with pytest.raises(TimeoutError):
        source.close()
    assert source.closed and source.recording_report is None
    report = source.stop(timeout=1)
    assert report.complete
    assert source.recording_report == report
    assert recorder.attempts == 3


@pytest.mark.parametrize("during_connect", [False, True])
def test_callback_failure_is_retained_stops_delivery_and_closes_transport(
    monkeypatch: pytest.MonkeyPatch,
    during_connect: bool,
    tmp_path: Path,
) -> None:
    import sys
    from threading import Event

    capture_clock = clock()
    closed = Event()
    release = Event()
    completed = Event()
    close_calls: list[int] = []
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []
    reported: list[object] = []
    failure = ValueError("user callback failed")
    seen: list[object] = []

    def callback(value: object) -> None:
        seen.append(value)
        raise failure

    class Connection:
        clock = capture_clock
        closed = False
        dropped_packets = 0
        worker_failure = None
        last_message_time = None

        def close(self) -> None:
            close_calls.append(1)
            closed.set()
            assert release.wait(2)
            self.closed = True

    class Client:
        def __init__(self, **settings: object) -> None:
            pass

        def create_virtual(
            self,
            name: str,
            deliver: Callable[[events.ReceivedPacket], object],
            identity: str,
        ) -> Connection:
            callbacks.append(deliver)
            if during_connect:
                deliver(packet(b"\x90\x3c\x64", 0, 1))
            return Connection()

    monkeypatch.setattr(_native, "Input", Client)
    monkeypatch.setattr(sys, "unraisablehook", reported.append)
    source = midi.Input().create_virtual("callback-failure", callback)
    if not during_connect:
        source.start(tmp_path / "failed-callback.meedee")
        callbacks[0](packet(b"\x90\x3c\x64", 0, 1))
    assert closed.wait(2)

    def close_on_controlling_thread() -> None:
        source.close()
        completed.set()

    closer = Thread(target=close_on_controlling_thread)
    closer.start()
    try:
        assert not completed.wait(0.02)
    finally:
        release.set()
        closer.join(2)
    assert completed.is_set() and close_calls == [1]
    assert source.closed and source.failure is failure
    if not during_connect:
        try:
            report = source.stop(timeout=2)
        except midi.StateError:
            report = source.recording_report
        assert report is not None and not report.complete
        assert source.recording_report == report
    callbacks[0](packet(b"\x90\x3e\x64", 1, 2))
    assert len(seen) == 1 and len(reported) == 1
    with pytest.raises(midi.StateError, match="closed"):
        source.__enter__()
    source.close()


def test_raw_recording_attachment_is_rejected_without_opening_a_port(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def unexpected(**settings: object) -> None:
        raise AssertionError("raw recording initialized native MIDI")

    monkeypatch.setattr(_native, "Input", unexpected)
    source = midi.Input(raw=True)
    path = tmp_path / "raw.meedee"
    with pytest.raises(midi.StateError, match="decoded input"):
        source.start(path)
    assert not source.closed and not path.exists()
    source.close()


def test_callback_cleanup_allows_reentrant_close_from_callable_destruction(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import sys
    from threading import Event

    destroyed = Event()
    entered = Event()
    release = Event()
    callbacks: list[Callable[[events.ReceivedPacket], object]] = []
    capture_clock = clock()

    class Connection:
        clock = capture_clock
        closed = False
        dropped_packets = 0
        worker_failure = None
        last_message_time = None

        def close(self) -> None:
            entered.set()
            assert release.wait(2)
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

    class Callback:
        def __call__(self, value: object) -> Iterator[object]:
            return (v for v in ())

        def __del__(self) -> None:
            source.close()
            destroyed.set()

    monkeypatch.setattr(_native, "Input", Client)
    monkeypatch.setattr(sys, "unraisablehook", lambda error: None)
    source = midi.Input()
    source.create_virtual("reentrant-close", Callback())
    callbacks[0](packet(b"\x90\x3c\x64", 0, 1))
    assert entered.wait(2)
    closer = Thread(target=source.close, daemon=True)
    closer.start()
    try:
        assert not destroyed.wait(0.02)
    finally:
        release.set()
    closer.join(2)
    assert destroyed.wait(2) and not closer.is_alive()
    assert source.closed
    assert isinstance(source.failure, TypeError)
    source.close()


def test_recorded_fragmented_sysex_exports_and_replays_with_timing(
    iterable_input: list[Callable[[events.ReceivedPacket], object]],
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    from meedee import sysex

    client = midi.Input()
    client.connect(client.ports()[0])
    iterable_input[0](packet(b"\xf0\x7d\x01", 1, 10))
    iterable_input[0](packet(b"\xf8\x02\xf7", 2, 20))
    records = client.record(0)
    dump = [r for r in records if isinstance(r, messages.SysEx)]
    assert len(dump) == 1 and dump[0].data == b"\x7d\x01\x02"
    assert dump[0].timestamp is not None and dump[0].timestamp.ticks == 20
    path = tmp_path / "dump.syx"
    sysex.write(path, dump)
    assert list(sysex.Reader(path)) == dump
    sent: list[object] = []
    monkeypatch.setattr(midi.Output, "__enter__", lambda self: self)
    monkeypatch.setattr(midi.Output, "_send", lambda self, value: sent.append(value))
    output = midi.Output()
    report = output.send(records)
    assert report.sent_events == 3
    assert b"\xf0\x7d\x01\x02\xf7" in sent
    output.close()
