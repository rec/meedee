"""MTS values, explicit decoding and caller-driven reply tracking, without I/O."""

import inspect
from collections.abc import Iterator
from pathlib import Path

import pytest

from meedee import codecs, controls, messages, tuning


def tuning_vectors() -> Iterator[tuple[messages.Message, bytes]]:
    table = tuning.TuningTable(values=[tuning.NoChange()] * 128)
    changes = [tuning.NoteTuning(note=69, pitch=tuning.Pitch(semitone=69, fraction=0))]
    fixture = (
        Path(__file__).resolve().parents[1] / "crates/meedee/tests/fixtures/tuning.txt"
    )
    for line in fixture.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, tokens = line.split("|")
        wire = b"".join(
            {
                "NAME": b" " * 16,
                "TABLE": b"\x7f" * 384,
                "Z12": b"\x40" * 12,
                "Z24": b"\x40\x00" * 12,
            }[t]
            if t in ("NAME", "TABLE", "Z12", "Z24")
            else bytes.fromhex(t)
            for t in tokens.split()
        )
        match name:
            case "BulkDumpRequest":
                value = tuning.BulkDumpRequest(device_id=1, program=2)
            case "BankedDumpRequest":
                value = tuning.BankedDumpRequest(device_id=1, bank=3, program=2)
            case "BulkDump":
                value = tuning.BulkDump(device_id=1, program=2, name="", table=table)
            case "KeyBasedDump":
                value = tuning.KeyBasedDump(
                    device_id=1, bank=3, program=2, name="", table=table
                )
            case "SingleNoteChange":
                value = tuning.SingleNoteChange(device_id=1, program=2, changes=changes)
            case _ if name.startswith("ScaleOctaveDump"):
                value = tuning.ScaleOctaveDump(
                    device_id=1,
                    bank=3,
                    program=2,
                    name="",
                    offsets=tuning.ScaleOffsets(
                        resolution=14 if "14" in name else 7, values=[0] * 12
                    ),
                )
            case _ if name.startswith("BankedNoteChange"):
                value = tuning.BankedNoteChange(
                    device_id=1,
                    bank=3,
                    program=2,
                    realtime=name.endswith("True"),
                    changes=changes,
                )
            case _:
                value = tuning.ScaleOctaveChange(
                    device_id=1,
                    realtime=name.endswith("True"),
                    channels=[1, 8, 16],
                    offsets=tuning.ScaleOffsets(
                        resolution=14 if "14" in name else 7, values=[0] * 12
                    ),
                )
        yield value, wire


def test_shared_vectors_encode_and_decode_only_when_explicitly_requested() -> None:
    for m, b in tuning_vectors():
        assert isinstance(m, messages.Message)
        assert codecs.encode(m) == b
        assert controls.Interpreter().feed(m) == []
        decoder = codecs.Decoder()
        raw = decoder.feed(b)
        assert raw == [messages.SysEx(data=b[1:-1])]
        result = tuning.decode(raw[0])  # ty: ignore[invalid-argument-type]
        assert result is not None
        assert result.message == m
        assert result.checksum in (
            tuning.ChecksumStatus.Matches,
            tuning.ChecksumStatus.NotPresent,
        )
        assert result == tuning.decode(messages.SysEx(data=b[1:-1]))
        assert hash(result) == hash(tuning.decode(messages.SysEx(data=b[1:-1])))
    assert tuning.decode(messages.SysEx(data=b"\x01\x02")) is None
    assert tuning.decode(messages.SysEx(data=b"\x7e\x01\x08\x7f")) is None
    with pytest.raises(TypeError):
        tuning.decode(b"\xf0\xf7")  # ty: ignore[invalid-argument-type]


def test_frozen_keyword_values_have_checked_replacement_and_matching() -> None:
    values: list[object] = [m for m, _ in tuning_vectors()]
    values.extend(
        [
            tuning.Pitch(semitone=69, fraction=0),
            tuning.TuningTable(values=[tuning.NoChange()] * 128),
            tuning.NoteTuning(note=69, pitch=tuning.NoChange()),
            tuning.ScaleOffsets(resolution=14, values=[0] * 12),
        ]
    )
    for original in values:
        cls = getattr(tuning, type(original).__name__)
        fields = cls.__annotations__
        v = cls(**{k: getattr(original, k) for k in fields})
        assert v == original
        assert cls.__module__ == "meedee.tuning"
        assert not hasattr(v, "__dict__")
        replace = v.replace
        assert v == replace()
        assert hash(v) == hash(replace())
        assert repr(v).startswith(cls.__name__ + "(")
        assert isinstance(cls.__match_args__, tuple)
        assert tuple(fields) == cls.__match_args__
        with pytest.raises(TypeError):
            replace(unknown=1)
        with pytest.raises(TypeError):
            cls(*[getattr(v, k) for k in fields])
        for f in fields:
            assert (
                inspect.signature(cls).parameters[f].kind
                == inspect.Parameter.KEYWORD_ONLY
            )
            with pytest.raises(AttributeError):
                setattr(v, f, 1)
            if fields[f] is int:
                for x in (True, 1.0, "1", None):
                    with pytest.raises(TypeError):
                        replace(**{f: x})
            if f == "realtime":
                with pytest.raises(TypeError):
                    replace(realtime=1)
    match tuning.Pitch(semitone=69, fraction=0):
        case tuning.Pitch(69, 0):
            pass
        case _:
            pytest.fail("pitch pattern did not match")
    assert tuning.NoChange() == tuning.NoChange()
    assert hash(tuning.NoChange()) == hash(tuning.NoChange())
    assert tuning.NoChange() != tuning.Pitch(semitone=127, fraction=16382)


def test_mutable_collections_are_snapshotted_and_wire_order_is_preserved() -> None:
    notes: list[tuning.Pitch | tuning.NoChange] = [tuning.NoChange()] * 128
    table = tuning.TuningTable(values=notes)
    notes[0] = tuning.Pitch(semitone=0, fraction=0)
    assert table.values[0] == tuning.NoChange()
    assert isinstance(table.values, tuple)
    entries = [
        tuning.NoteTuning(note=69, pitch=tuning.NoChange()),
        tuning.NoteTuning(note=69, pitch=tuning.Pitch(semitone=69, fraction=0)),
    ]
    value = tuning.SingleNoteChange(device_id=127, program=127, changes=entries)
    entries.clear()
    assert len(value.changes) == 2
    assert [c.note for c in value.changes] == [69, 69]
    selected = [16, 1, 8]
    offsets = tuning.ScaleOffsets(resolution=7, values=[0] * 12)
    change = tuning.ScaleOctaveChange(
        device_id=1, realtime=False, channels=selected, offsets=offsets
    )
    selected.clear()
    assert change.channels == (1, 8, 16)
    assert codecs.encode(change)[5:8] == bytes([2, 1, 1])
    empty = change.replace(channels=[])
    decoded = tuning.decode(messages.SysEx(data=codecs.encode(empty)[1:-1]))
    assert decoded is not None and decoded.message == empty
    empty_notes = value.replace(changes=[])
    decoded = tuning.decode(messages.SysEx(data=codecs.encode(empty_notes)[1:-1]))
    assert decoded is not None and decoded.message == empty_notes
    with pytest.raises(ValueError):
        change.replace(channels=[1, 1])
    with pytest.raises(TypeError):
        tuning.TuningTable(values=iter(notes))  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        tuning.TuningTable(values=notes[:127])
    with pytest.raises(TypeError):
        tuning.NoteTuning(note=0, pitch=None)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        tuning.Pitch(semitone=127, fraction=16383)


def test_quantization_reports_only_wire_precision_loss() -> None:
    exact = tuning.Pitch.from_frequency(frequency_hz=440.0)
    assert exact.pitch == tuning.Pitch(semitone=69, fraction=0)
    assert exact.pitch.frequency_hz == 440.0
    assert exact.error_cents == 0.0
    converted = tuning.Pitch.from_frequency(frequency_hz=441.123456)
    assert converted.pitch.frequency_hz != 441.123456
    assert abs(converted.error_cents) <= 100 / 32768
    half = 100 / 32768
    assert tuning.Pitch.from_cents(note=0, cents=half).pitch.fraction == 0
    assert tuning.Pitch.from_cents(note=0, cents=3 * half).pitch.fraction == 2
    assert tuning.Pitch.from_cents(note=0, cents=half).error_cents == -half
    result = tuning.ScaleOffsets.from_cents(resolution=14, cents=[0.1] * 12)
    assert result.offsets.values == (8,) * 12
    assert result.error_cents == pytest.approx((-0.00234375,) * 12)
    assert isinstance(result.error_cents, tuple)
    assert hash(exact) == hash(tuning.Pitch.from_frequency(frequency_hz=440.0))
    assert hash(result) == hash(
        tuning.ScaleOffsets.from_cents(resolution=14, cents=[0.1] * 12)
    )
    for value in (exact, result):
        with pytest.raises(AttributeError):
            value.error_cents = 0.0  # ty: ignore[invalid-assignment]
    for x in (0, -1, float("nan"), float("inf")):
        with pytest.raises(ValueError):
            tuning.Pitch.from_frequency(frequency_hz=x)
    for x in (True, "440", None):
        with pytest.raises(TypeError):
            tuning.Pitch.from_frequency(frequency_hz=x)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        tuning.ScaleOffsets.from_cents(resolution=14, cents=[100.0] * 12)
    with pytest.raises(ValueError):
        tuning.ScaleOffsets(resolution=7, values=[64] * 12)
    with pytest.raises(ValueError):
        tuning.Pitch.from_cents(note=0, cents=-0.00001)


def test_dump_checksum_policy_and_ascii_names() -> None:
    for m, b in tuning_vectors():
        if not isinstance(
            m, (tuning.BulkDump, tuning.KeyBasedDump, tuning.ScaleOctaveDump)
        ):
            continue
        assert m.name == " " * 16
        raw = bytearray(b[1:-1])
        raw[-1] ^= 1
        if isinstance(m, tuning.BulkDump):
            result = tuning.decode(messages.SysEx(data=raw))
            assert result is not None
            assert result.checksum == tuning.ChecksumStatus.LegacyMismatch
            assert result.message == m
            assert codecs.encode(result.message) == b
        else:
            with pytest.raises(ValueError, match="checksum"):
                tuning.decode(messages.SysEx(data=raw))
        with pytest.raises(ValueError):
            m.replace(name="not ASCII: é")
        with pytest.raises(ValueError):
            m.replace(name="x" * 17)
        assert m.replace(name="x").name == "x" + " " * 15


def test_reply_tracker_matches_only_a_timely_reply_and_keeps_terminal_state() -> None:
    request = tuning.BankedDumpRequest(device_id=1, bank=3, program=2)
    tracker = tuning.ReplyTracker(request=request, now_ns=100, timeout_ns=10)
    assert tracker.deadline_ns == 110
    assert tracker.status == tuning.RequestStatus.Pending
    reply = tuning.KeyBasedDump(
        device_id=1,
        bank=3,
        program=2,
        name="",
        table=tuning.TuningTable(values=[tuning.NoChange()] * 128),
    )
    assert not tracker.observe(reply.replace(device_id=2), now_ns=101)
    assert not tracker.observe(reply.replace(bank=4), now_ns=102)
    assert not tracker.observe(reply.replace(program=3), now_ns=103)
    assert tracker.observe(reply, now_ns=109)
    assert tracker.status == tuning.RequestStatus.Completed
    assert tracker.poll(now_ns=110) == tuning.RequestStatus.Completed
    assert tracker.cancel() == tuning.RequestStatus.Completed
    assert not tracker.observe(reply, now_ns=111)
    tracker = tuning.ReplyTracker(request=request, now_ns=0, timeout_ns=10)
    assert not tracker.observe(reply, now_ns=10)
    assert tracker.status == tuning.RequestStatus.TimedOut
    with pytest.raises(ValueError):
        tracker.poll(now_ns=9)
    tracker = tuning.ReplyTracker(request=request, now_ns=0, timeout_ns=10)
    assert tracker.cancel() == tuning.RequestStatus.Cancelled
    assert not tracker.observe(reply, now_ns=1)
    for kwargs in (
        {"timeout_ns": 0},
        {"now_ns": 2**64 - 1},
        {"request": request.replace(device_id=127)},
    ):
        with pytest.raises(ValueError):
            tuning.ReplyTracker(
                **({"request": request, "now_ns": 0, "timeout_ns": 1} | kwargs)
            )
    with pytest.raises(TypeError):
        tuning.ReplyTracker(request=request, now_ns=True, timeout_ns=1)
