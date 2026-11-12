"""Optional source resolution and MTS generation, with no MIDI transport."""

from fractions import Fraction

import pytest

pytest.importorskip("ufor", reason="install middy[ufor] for tuning integration tests")

from ufor import scala
from ufor.tuning import Computed, FrequencyTable, IntervalPattern, RatioTable, Tuning

from middy import codecs, messages, tuning


def test_conversion_returns_a_native_note_tuning() -> None:
    source = Tuning(source=RatioTable(values=["1", "3/2"]), root_note=69)
    assert tuning.convert(source, 70) == tuning.NoteTuning(
        note=70, pitch=tuning.Pitch(semitone=76, fraction=320)
    )
    assert tuning.convert(source, 69).pitch == tuning.Pitch(semitone=69, fraction=0)
    with pytest.raises(KeyError, match="outside"):
        tuning.convert(source, 71)


@pytest.mark.parametrize(
    "source",
    [
        Tuning(source=FrequencyTable(values=["440", "660"], first_note=69)),
        Tuning(source=IntervalPattern(intervals=["3/2"], repeat=False), root_note=69),
    ],
)
def test_finite_source_coverage_uses_its_actual_note_origin(source: Tuning) -> None:
    for n in [68, 71]:
        with pytest.raises(KeyError):
            tuning.convert(source, n)
    assert tuning.convert(source, 69).pitch == tuning.Pitch(semitone=69, fraction=0)
    assert tuning.convert(source, 70).pitch == tuning.Pitch(semitone=76, fraction=320)


def test_scala_and_computed_tunings_use_existing_ufor_resolution() -> None:
    table = scala.parse_scala("Three steps\n3\n9/8\n3/2\n2/1\n", name="three")
    for source in [Tuning(source=table), Tuning(source=Computed(notes_per_octave=19))]:
        for n in [69, 70, 71, 72]:
            assert (
                tuning.convert(source, n).pitch
                == tuning.Pitch.from_frequency(frequency_hz=float(source(n))).pitch
            )


def test_invalid_frequencies_and_evaluation_errors_raise() -> None:
    source = Tuning(source=FrequencyTable(values=["1", "440", "100000"], first_note=60))
    for n in [60, 62]:
        with pytest.raises(ValueError, match="MTS frequency range"):
            tuning.convert(source, n)
    assert isinstance(source.source, FrequencyTable)
    source.source.values[1] = "1/0"
    with pytest.raises((ArithmeticError, ValueError)):
        tuning.convert(source, 61)


@pytest.mark.parametrize("semitone, fraction, direction", [(0, 0, -1), (127, 16382, 1)])
def test_out_of_range_exact_pitch_cannot_round_onto_a_valid_endpoint(
    semitone: int, fraction: int, direction: int
) -> None:
    edge = Fraction(tuning.Pitch(semitone=semitone, fraction=fraction).frequency_hz)
    outside = edge + direction * Fraction(1, 10**30)
    assert float(outside) == float(edge)
    source = Tuning(source=FrequencyTable(values=[str(outside)], first_note=0))
    with pytest.raises(ValueError, match="MTS frequency range"):
        tuning.convert(source, 0)


def test_bulk_requires_complete_resolution_or_explicit_preservation() -> None:
    result = [
        tuning.NoteTuning(note=68, pitch=tuning.NoChange()),
        tuning.convert(
            Tuning(source=FrequencyTable(values=["440"], first_note=69)), 69
        ),
    ]
    with pytest.raises(ValueError):
        tuning.bulk_dump(result, device_id=1, program=2, name="test")
    for bank in [None, 3]:
        message = tuning.bulk_dump(
            result,
            device_id=1,
            program=2,
            name="test",
            bank=bank,
            preserve_unmapped=True,
        )
        assert isinstance(message, tuning.BulkDump) and message.bank == bank
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
    result = [tuning.convert(Tuning(source=Computed()), 69)]
    with pytest.raises(ValueError, match="128"):
        tuning.bulk_dump(result, device_id=1, program=0, name="partial")
    message = tuning.note_changes(result, device_id=1, program=0, realtime=True)[0]
    assert isinstance(message, tuning.NoteChange)
    assert [p.note for p in message.changes] == [69]
    assert codecs.encode(message) == bytes.fromhex(
        "F0 7F 01 08 02 00 01 45 45 00 00 F7"
    )


def test_full_keyboard_builds_both_bulk_formats_and_bounded_note_batches() -> None:
    result = [tuning.convert(Tuning(source=Computed()), n) for n in range(128)]
    for bank in [None, 0]:
        message = tuning.bulk_dump(
            result, device_id=127, program=127, name="equal", bank=bank
        )
        assert len(message.table.values) == 128
        assert not any(isinstance(p, tuning.NoChange) for p in message.table.values)
    for bank, realtime in [(None, True), (0, False), (0, True)]:
        outgoing = tuning.note_changes(
            result, device_id=1, program=0, bank=bank, realtime=realtime
        )
        assert [len(m.changes) for m in outgoing] == [127, 1]
        assert [p.note for m in outgoing for p in m.changes] == list(range(128))
        for message in outgoing:
            encoded = codecs.encode(message)
            decoded = tuning.decode(messages.SysEx(data=encoded[1:-1]))
            assert decoded is not None and decoded.message == message


def test_note_changes_preserve_only_explicit_unmapped_keys() -> None:
    result = [
        tuning.convert(Tuning(source=RatioTable(values=["1"])), 69),
        tuning.NoteTuning(note=70, pitch=tuning.NoChange()),
    ]
    with pytest.raises(ValueError):
        tuning.note_changes(result, device_id=1, program=0, realtime=True)
    outgoing = tuning.note_changes(
        result, device_id=1, program=0, realtime=False, bank=1, preserve_unmapped=True
    )
    assert isinstance(outgoing[0].changes[1].pitch, tuning.NoChange)
    with pytest.raises(ValueError, match="real-time"):
        tuning.note_changes(
            result, device_id=1, program=0, realtime=False, preserve_unmapped=True
        )


@pytest.mark.parametrize(
    "note, exception",
    [(-1, ValueError), (128, ValueError), (True, TypeError), (69.0, TypeError)],
)
def test_conversion_requires_a_midi_note(note: int, exception: type[Exception]) -> None:
    with pytest.raises(exception):
        tuning.convert(Tuning(source=Computed()), note)


def test_message_builders_keep_native_validation_and_require_explicit_settings() -> (
    None
):
    result = [tuning.convert(Tuning(source=Computed()), 69)]
    with pytest.raises(TypeError):
        tuning.note_changes(result, device_id=True, program=0, realtime=True)
    with pytest.raises(ValueError):
        tuning.note_changes(result, device_id=1, program=128, realtime=True)
    with pytest.raises(ValueError):
        tuning.bulk_dump(
            result, device_id=1, program=0, name="x" * 17, preserve_unmapped=True
        )


def test_detuning_uses_ufor_and_converted_notes_do_not_change_with_source_edits() -> (
    None
):
    source = Tuning(source=RatioTable(values=["1"]), detune_cents=100)
    result = [tuning.convert(source, 69)]
    assert result[0].pitch == tuning.Pitch(semitone=70, fraction=0)
    assert isinstance(source.source, RatioTable)
    source.source.values[0] = "2"
    assert result[0].pitch == tuning.Pitch(semitone=70, fraction=0)
    result.append(result[0])
    with pytest.raises(ValueError, match="distinct"):
        tuning.bulk_dump(
            result, device_id=1, program=0, name="duplicate", preserve_unmapped=True
        )


@pytest.mark.parametrize("detune", [1e308, -1e308])
def test_ufor_numeric_overflow_and_underflow_cannot_generate_mts(detune: float) -> None:
    source = Tuning(source=Computed(), detune_cents=detune)
    with pytest.raises((ArithmeticError, ValueError)):
        tuning.convert(source, 69)


@pytest.mark.parametrize("bank", [None, 3])
def test_bulk_dump_can_be_sent_as_sysex_with_library_broadcast(
    bank: int | None,
) -> None:
    from middy.testing import Output

    result = [tuning.convert(Tuning(source=Computed()), n) for n in range(128)]
    dump = tuning.bulk_dump(result, device_id=127, program=2, name="dump", bank=bank)
    wire = codecs.encode(dump)
    message = messages.SysEx(data=wire[1:-1])
    output = Output(encoding="bytes")
    output.send(message)
    assert output.take_packet() == wire
    output.close()
