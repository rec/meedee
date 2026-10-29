"""Optional source resolution and MTS generation, with no MIDI transport."""

from fractions import Fraction

import pytest

pytest.importorskip("ufor", reason="install meedee[ufor] for tuning integration tests")

from ufor import scala
from ufor.tuning import Computed, FrequencyTable, IntervalPattern, RatioTable, Tuning

from meedee import codecs, messages, tuning, ufor_tuning


def test_ratio_preview_preserves_exact_values_order_and_quantization_error() -> None:
    source = Tuning(source=RatioTable(values=["1", "3/2"]), root_note=69)
    result = ufor_tuning.preview(source, notes=[70, 69, 71])
    assert [k.note for k in result.entries] == [70, 69, 71]
    fifth, root, missing = result.entries
    assert fifth.frequency == Fraction(660)
    assert isinstance(fifth.frequency, Fraction)
    assert fifth.pitch == tuning.Pitch(semitone=76, fraction=320)
    assert fifth.error_cents is not None and -0.0031 < fifth.error_cents < 0
    assert fifth.pitch is not None
    assert fifth.encoded_frequency_hz == fifth.pitch.frequency_hz
    assert root.frequency == Fraction(440)
    assert root.pitch == tuning.Pitch(semitone=69, fraction=0)
    assert root.error_cents == 0
    assert missing.status == ufor_tuning.PitchStatus.UNMAPPED
    assert missing.frequency is None and missing.pitch is None


@pytest.mark.parametrize(
    "source",
    [
        Tuning(
            source=FrequencyTable(values=["440", "660"], first_note=69), root_note=0
        ),
        Tuning(source=IntervalPattern(intervals=["3/2"], repeat=False), root_note=69),
    ],
)
def test_finite_source_coverage_uses_its_actual_note_origin(source: Tuning) -> None:
    result = ufor_tuning.preview(source, notes=[68, 69, 70, 71])
    assert [k.status for k in result.entries] == [
        ufor_tuning.PitchStatus.UNMAPPED,
        ufor_tuning.PitchStatus.CONVERTED,
        ufor_tuning.PitchStatus.CONVERTED,
        ufor_tuning.PitchStatus.UNMAPPED,
    ]
    assert [k.frequency for k in result.entries] == [
        None,
        Fraction(440),
        Fraction(660),
        None,
    ]


def test_scala_and_computed_tunings_use_existing_ufor_resolution() -> None:
    table = scala.parse_scala("Three steps\n3\n9/8\n3/2\n2/1\n", name="three")
    source = Tuning(source=table)
    result = ufor_tuning.preview(source, notes=[69, 70, 71, 72])
    assert [k.frequency for k in result.entries] == [
        Fraction(440),
        Fraction(495),
        Fraction(660),
        Fraction(880),
    ]
    edo = Tuning(source=Computed(notes_per_octave=19))
    resolved = ufor_tuning.preview(edo, notes=[69, 70, 88])
    assert resolved.entries[0].frequency == Fraction(440)
    assert isinstance(resolved.entries[1].frequency, float)
    assert resolved.entries[1].frequency == edo(70)
    assert resolved.entries[2].frequency == Fraction(880)


def test_missing_out_of_range_and_evaluation_errors_are_distinct() -> None:
    source = Tuning(source=FrequencyTable(values=["1", "440", "100000"], first_note=60))
    result = ufor_tuning.preview(source, notes=[59, 60, 61, 62])
    assert [k.status for k in result.entries] == [
        ufor_tuning.PitchStatus.UNMAPPED,
        ufor_tuning.PitchStatus.OUT_OF_RANGE,
        ufor_tuning.PitchStatus.CONVERTED,
        ufor_tuning.PitchStatus.OUT_OF_RANGE,
    ]
    assert result.entries[1].frequency == Fraction(1)
    # A caller-mutated expression is an evaluation error, not a missing key.
    assert isinstance(source.source, FrequencyTable)
    source.source.values[1] = "1/0"
    failed = ufor_tuning.preview(source, notes=[61])
    assert failed.entries[0].status == ufor_tuning.PitchStatus.EVALUATION_ERROR
    assert failed.entries[0].reason
    for item in [result, failed]:
        with pytest.raises(ValueError):
            ufor_tuning.note_changes(
                item, device_id=1, program=0, realtime=True, preserve_unmapped=True
            )


@pytest.mark.parametrize("semitone, fraction, direction", [(0, 0, -1), (127, 16382, 1)])
def test_out_of_range_exact_pitch_cannot_round_onto_a_valid_endpoint(
    semitone: int,
    fraction: int,
    direction: int,
) -> None:
    edge = Fraction(tuning.Pitch(semitone=semitone, fraction=fraction).frequency_hz)
    outside = edge + direction * Fraction(1, 10**30)
    assert float(outside) == float(edge)
    source = Tuning(source=FrequencyTable(values=[str(outside)], first_note=0))
    result = ufor_tuning.preview(source, notes=[0])
    assert result.entries[0].frequency == outside
    assert result.entries[0].status == ufor_tuning.PitchStatus.OUT_OF_RANGE


def test_bulk_requires_complete_resolution_or_explicit_preservation() -> None:
    result = ufor_tuning.preview(
        Tuning(source=FrequencyTable(values=["440"], first_note=69)), notes=[68, 69]
    )
    with pytest.raises(ValueError):
        ufor_tuning.bulk_dump(result, device_id=1, program=2, name="test")
    for bank in [None, 3]:
        message = ufor_tuning.bulk_dump(
            result,
            device_id=1,
            program=2,
            name="test",
            bank=bank,
            preserve_unmapped=True,
        )
        assert isinstance(
            message, tuning.BulkDump if bank is None else tuning.KeyBasedDump
        )
        assert message.table.values[69] == tuning.Pitch(semitone=69, fraction=0)
        assert all(
            isinstance(p, tuning.NoChange)
            for i, p in enumerate(message.table.values)
            if i != 69
        )
        encoded = codecs.encode(message)
        decoded = tuning.decode(messages.SysEx(data=encoded[1:-1]))
        assert decoded is not None and decoded.message == message


def test_unselected_keys_need_preservation_for_bulk_but_not_for_note_changes() -> None:
    result = ufor_tuning.preview(Tuning(source=Computed()), notes=[69])
    with pytest.raises(ValueError, match="128"):
        ufor_tuning.bulk_dump(result, device_id=1, program=0, name="partial")
    message = ufor_tuning.note_changes(result, device_id=1, program=0, realtime=True)[0]
    assert isinstance(message, tuning.SingleNoteChange)
    assert [p.note for p in message.changes] == [69]
    assert codecs.encode(message) == bytes.fromhex(
        "F0 7F 01 08 02 00 01 45 45 00 00 F7"
    )


def test_full_keyboard_builds_both_bulk_formats_and_bounded_note_batches() -> None:
    result = ufor_tuning.preview(Tuning(source=Computed()), notes=list(range(128)))
    assert all(k.status == ufor_tuning.PitchStatus.CONVERTED for k in result.entries)
    for bank in [None, 0]:
        message = ufor_tuning.bulk_dump(
            result, device_id=127, program=127, name="equal", bank=bank
        )
        assert len(message.table.values) == 128
        assert not any(isinstance(p, tuning.NoChange) for p in message.table.values)
    for bank, realtime in [(None, True), (0, False), (0, True)]:
        outgoing = ufor_tuning.note_changes(
            result, device_id=1, program=0, bank=bank, realtime=realtime
        )
        assert [len(m.changes) for m in outgoing] == [127, 1]
        assert [p.note for m in outgoing for p in m.changes] == list(range(128))
        for message in outgoing:
            encoded = codecs.encode(message)
            decoded = tuning.decode(messages.SysEx(data=encoded[1:-1]))
            assert decoded is not None and decoded.message == message


def test_note_changes_preserve_only_explicit_unmapped_keys() -> None:
    result = ufor_tuning.preview(
        Tuning(source=RatioTable(values=["1"])), notes=[69, 70]
    )
    with pytest.raises(ValueError):
        ufor_tuning.note_changes(result, device_id=1, program=0, realtime=True)
    outgoing = ufor_tuning.note_changes(
        result, device_id=1, program=0, realtime=False, bank=1, preserve_unmapped=True
    )
    assert isinstance(outgoing[0].changes[1].pitch, tuning.NoChange)
    with pytest.raises(ValueError, match="real-time"):
        ufor_tuning.note_changes(
            result, device_id=1, program=0, realtime=False, preserve_unmapped=True
        )


@pytest.mark.parametrize(
    "notes, exception",
    [
        ([], ValueError),
        ([69, 69], ValueError),
        ([128], ValueError),
        ([True], TypeError),
        ([69.0], TypeError),
    ],
)
def test_key_selection_is_bounded_distinct_and_strict(
    notes: list[int], exception: type[Exception]
) -> None:
    with pytest.raises(exception):
        ufor_tuning.preview(Tuning(source=Computed()), notes=notes)


def test_message_builders_keep_native_validation_and_require_explicit_settings() -> (
    None
):
    result = ufor_tuning.preview(Tuning(source=Computed()), notes=[69])
    with pytest.raises(TypeError):
        ufor_tuning.note_changes(result, device_id=True, program=0, realtime=True)
    with pytest.raises(ValueError):
        ufor_tuning.note_changes(result, device_id=1, program=128, realtime=True)
    with pytest.raises(ValueError):
        ufor_tuning.bulk_dump(
            result, device_id=1, program=0, name="x" * 17, preserve_unmapped=True
        )


def test_detuning_uses_ufor_and_preview_records_do_not_change_with_source_edits() -> (
    None
):
    source = Tuning(source=RatioTable(values=["1"]), detune_cents=100)
    result = ufor_tuning.preview(source, notes=[69])
    assert result.entries[0].frequency == source(69)
    assert result.entries[0].pitch == tuning.Pitch(semitone=70, fraction=0)
    assert isinstance(source.source, RatioTable)
    source.source.values[0] = "2"
    assert result.entries[0].pitch == tuning.Pitch(semitone=70, fraction=0)
    result.entries.append(result.entries[0])
    with pytest.raises(ValueError, match="distinct"):
        ufor_tuning.bulk_dump(
            result, device_id=1, program=0, name="duplicate", preserve_unmapped=True
        )


@pytest.mark.parametrize("detune", [1e308, -1e308])
def test_ufor_numeric_overflow_and_underflow_cannot_generate_mts(detune: float) -> None:
    source = Tuning(source=Computed(), detune_cents=detune)
    result = ufor_tuning.preview(source, notes=[69])
    assert result.entries[0].status == ufor_tuning.PitchStatus.EVALUATION_ERROR
    with pytest.raises(ValueError, match="evaluation_error"):
        ufor_tuning.note_changes(
            result, device_id=1, program=0, realtime=True, preserve_unmapped=True
        )


@pytest.mark.parametrize("bank", [None, 3])
def test_bulk_plan_uses_existing_transfer_bounds_and_preserves_library_broadcast(
    bank: int | None,
) -> None:
    from meedee import transfer, transfer_output
    from meedee.testing import Output

    result = ufor_tuning.preview(Tuning(source=Computed()), notes=list(range(128)))
    plan = ufor_tuning.bulk_plan(
        result,
        device_id=127,
        program=2,
        name="planned",
        gap_ns=37,
        bank=bank,
    )
    assert plan.message_count == 1 and plan.gap_ns == 37
    wire = codecs.encode(plan.message(0))
    assert plan.wire_bytes == len(wire)
    decoded = tuning.decode(plan.message(0))
    assert decoded is not None and decoded.message.device_id == 127
    output = Output(protocol="midi1")
    report = transfer_output.run(transfer.Session(plan), output)  # ty: ignore[invalid-argument-type]
    assert report.status == "completed" and output.take_packet() == wire
    output.close()
