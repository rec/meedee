"""Source-local timing, typed adapters, recording and supervised cancellation."""

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


def event(ticks: int) -> events.Event:
    return events.Event(
        stream="s",
        timestamp=events.Timestamp(ticks=ticks, clock=clock()),
        message=messages.NoteOn(channel=1, note=60, velocity=100),
    )


def test_native_packet_spans_use_completion_time_without_realtime_contamination() -> (
    None
):
    decoder = events.PacketDecoder(stream="s", protocol=midi.Protocol.Midi1)
    assert decoder.feed(packet(b"\x90\x3c", 0, 10)) == []
    result = decoder.feed(packet(b"\xf8\x64\x3d\x65", 1, 20))
    assert len(result) == 3
    first, note, running = result
    assert isinstance(first, events.Event) and first.span is not None
    assert first.span.first_packet == first.span.last_packet == 1
    assert (
        isinstance(note, events.Event)
        and note.timestamp is not None
        and note.span is not None
    )
    assert note.timestamp.ticks == 20
    assert (
        note.span.first_timestamp is not None and note.span.first_timestamp.ticks == 10
    )
    assert (note.span.first_packet, note.span.last_packet) == (0, 1)
    assert (
        isinstance(running, events.Event)
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
    assert not any(isinstance(v, events.Event) for v in lost)


def test_ump_framing_and_typed_send_never_downgrade_or_send_opaque_values() -> None:
    decoder = events.PacketDecoder(stream="u", protocol=midi.Protocol.Ump)
    assert decoder.feed(packet(midi.UmpPacket(words=(0x40903C00,)), 0, 1)) == []
    value = decoder.feed(packet(midi.UmpPacket(words=(0xFFFF0000,)), 1, 2))[0]
    assert isinstance(value, events.Event) and isinstance(
        value.message, ump.Midi2NoteOn
    )
    assert value.span is not None and value.span.first_packet == 0
    with pytest.raises(midi.CapabilityError):
        _native.prepare_output(midi.Protocol.Midi1, value.message)
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

    def send(self, message: messages.Message) -> None:
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
            self, name: str, callback: Callable[[events.ReceivedPacket], object]
        ) -> Connection:
            callbacks.append(callback)
            return connection

    monkeypatch.setattr("meedee._typed.Input", Input)
    receiver = midi.TypedInput().create_virtual("test-only")
    path = tmp_path / "typed.meedee"
    receiver.start_recording(path)
    callback = callbacks[0]
    callback(packet(b"\x90\x3c\x64", 0, 1))
    value = receiver.receive(timeout=0)
    assert isinstance(value, events.Event) and value.message == event(1).message
    connection.dropped_packets = int(lost)
    report = receiver.stop_recording()
    assert report.complete is not lost
    receiver.close()
    reader = archive.Reader(path, recover=lost)
    values = list(reader)
    assert len([v for v in values if isinstance(v, events.Event)]) == 1
    assert reader.complete is not lost
    reader.close()
