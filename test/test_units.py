"""Authored quantities normalize before reaching MIDI and scheduler code."""

import inspect

import pytest
import tyro
from reccy.configuration import units

from middy import (
    ci,
    codecs,
    controls,
    events,
    messages,
    midi,
    smf,
    testing,
    timecode,
    tuning,
    ump,
    universe,
)
from middy._cli import WatchPorts


def test_configuration_retains_authored_units_and_normalizes_cli() -> None:
    config = tyro.cli(
        WatchPorts,
        args=["--encoding", "bytes", "--interval", "250 ms", "--duration", "1:30"],
    )
    assert config.interval == 0.25
    assert config.duration == 90
    assert units.authored_dump(config)["interval"] == "250 ms"
    assert units.runtime_dump(config)["interval"] == 0.25
    assert WatchPorts.model_validate(units.revalidation_dump(config)) == config


def test_public_defaults_are_seconds_and_bpm() -> None:
    assert inspect.signature(ci.Session.discover).parameters["timeout"].default == 3
    parameters = inspect.signature(ci.Browser).parameters
    assert parameters["discovery_timeout"].default == 3
    assert parameters["endpoint_timeout"].default == 1
    assert parameters["timeout"].default == 30
    assert inspect.signature(smf.export_event).parameters["tempo"].default == 120


def test_exact_time_conversion_and_wrong_dimensions() -> None:
    clock = testing.ManualClock(now_ns="1 ms")
    clock.advance("2 us")
    assert clock.now_ns == 1_002_000
    for value in ("1 ns / 3", "1 beat", "440 Hz"):
        with pytest.raises(ValueError):
            clock.advance(value)
    assert clock.now_ns == 1_002_000


def test_frequency_and_pitch_intervals() -> None:
    assert (
        tuning.Pitch.from_frequency(frequency_hz="0.44 kHz").pitch
        == tuning.Pitch.from_frequency(frequency_hz=440).pitch
    )
    assert (
        tuning.Pitch.from_cents(note=60, cents="1 semitone").pitch
        == tuning.Pitch.from_cents(note=60, cents=100).pitch
    )


@pytest.mark.parametrize("tempo", ["120 bpm", "500 ms / beat", "120"])
def test_ump_tempo_strings_encode_the_same_midi_values(tempo: str) -> None:
    assert ump.tempo(group=1, bpm=tempo) == ump.tempo(group=1, bpm=120.0)


def test_smf_tempo_conversion_preserves_event_ticks() -> None:
    clock = events.Clock(identity="test")
    origin = events.Timestamp(ticks=0, clock=clock)
    event = (messages.NoteOn(channel=1, note=60, velocity=90)).replace(
        stream="test", timestamp=events.Timestamp(ticks="1e9 tick", clock=clock)
    )
    header = smf.Header(format=0, tracks=1, division=480)
    assert (
        smf.export_event(event, header=header, origin=origin, tempo="120 bpm").events
        == smf.export_event(event, header=header, origin=origin, tempo=120).events
    )


def test_simulated_receive_accepts_seconds_and_packet_timestamps() -> None:
    source = testing.Input(
        stream="test", encoding="bytes", byte_limit="1 KiB", sysex_limit="512 byte"
    )
    clock = events.Clock(identity="test")
    source.inject(b"\x90\x3c\x40", clock=clock, timestamp_ns="2 ms")
    assert source.receive(timeout="0 s") is not None


def test_encoded_units_retain_integral_ranges() -> None:
    assert codecs.Decoder(sysex_limit="1 KiB").sysex_limit == 1024
    assert ump.JrTimestamp(ticks="2 tick").ticks == 2
    assert controls.channel_coarse_tuning(
        channel=1, semitones="100 cents"
    ) == controls.channel_coarse_tuning(channel=1, semitones=1)
    assert (
        tuning.ScaleOffsets.from_cents(
            resolution=14, cents=["0.01 semitone"] * 12
        ).offsets
        == tuning.ScaleOffsets.from_cents(resolution=14, cents=[1.0] * 12).offsets
    )
    with pytest.raises(ValueError):
        codecs.Decoder(sysex_limit="1 bit")
    with pytest.raises(ValueError):
        ump.tempo(group=1, bpm="1 ns / beat")
    with pytest.raises(ValueError):
        ump.tempo(group=1, bpm="0 bpm")


def test_timecode_fields_and_clock_resolution_accept_units() -> None:
    position = timecode.Position(
        rate=timecode.FrameRate.Fps25,
        hours="1 hour",
        minutes=0,
        seconds="2000 ms",
        frames="3 frame",
    )
    assert position.hours == 1
    assert position.seconds == 2
    assert position.frames == 3
    assert (
        events.Clock(identity="test", resolution_ticks="2 tick").resolution_ticks == 2
    )


@pytest.mark.parametrize("tempo", ["120 bpm", "500 ms / beat", "120"])
def test_output_normalizes_tempo_without_opening_a_device(tempo: str) -> None:
    with midi.Output(bpm=tempo) as output:
        assert output.bpm == 120


@pytest.mark.parametrize("tempo", ["1 Hz", "0 bpm", "-1 ms / beat"])
def test_output_rejects_incompatible_and_nonpositive_tempos(tempo: str) -> None:
    with pytest.raises(ValueError):
        midi.Output(bpm=tempo)


@pytest.mark.parametrize(
    "message",
    [
        messages.NoteOn(60),
        messages.NoteOff(60),
        messages.PolyPressure(60, 10),
        messages.NoteOn(60, version=2),
        messages.PerNoteController(group=1, channel=1, note=60, controller=1, value=2),
        messages.PerNotePitchBend(group=1, channel=1, note=60, value=0),
    ],
)
def test_transposition_converts_intervals_and_preserves_metadata(
    message: messages.Message,
) -> None:
    value = message.replace(stream="keys")
    shifted = value.transpose("1 octave")
    assert shifted == value.transpose(12)
    assert shifted.stream == "keys"
    for interval in ("50 cents", "1 s"):
        with pytest.raises(ValueError):
            value.transpose(interval)


def test_non_note_and_file_record_transposition_accepts_intervals() -> None:
    message = messages.TimingClock()
    assert message.transpose("100 cents") is message
    event = smf.Event(track=0, tick="1 tick", record=messages.NoteOn(60, channel=1))
    assert event.transpose("-1 octave") == event.transpose(-12)
    assert event.transpose("-1 octave").tick == 1
    smf.validate_meta(81, b"\x07\xa1\x20", offset="1 KiB")
    with pytest.raises(ValueError):
        smf.validate_meta(81, b"\x07\xa1\x20", offset="1 bit")


def test_clock_mapping_retains_exact_time_and_dimensionless_rate_strings() -> None:
    source = events.Clock(identity="source", tick_numerator=1, tick_denominator=1)
    target = events.Clock(identity="target")
    mapping = events.ClockMapping(
        source=source,
        target=target,
        rate="1001 / 1000",
        offset="250 ms",
        uncertainty="1 ns / 3",
    )
    expected = events.ClockMapping(
        source=source,
        target=target,
        rate=events.Rational(numerator=1001, denominator=1000),
        offset=events.Rational(numerator=1, denominator=4),
        uncertainty=events.Rational(numerator=1, denominator=3_000_000_000),
    )
    assert mapping == expected
    assert mapping.map_time(events.Timestamp(ticks=1, clock=source)) == events.Rational(
        numerator=1251, denominator=1000
    )
    assert mapping.replace(offset="500 ms").offset == events.Rational(
        numerator=1, denominator=2
    )
    for changes in ({"rate": "2 Hz"}, {"offset": "1 tick"}, {"uncertainty": "-1 ns"}):
        with pytest.raises(ValueError):
            mapping.replace(**changes)


def test_universe_timestamps_convert_units_before_observation() -> None:
    cache = universe.Universe(timecode_timeout="1 s")
    cache.feed(messages.Start(), now_ns="1 ms")
    assert cache.poll(now_ns="2 ms") == []
    for value in ("1 beat", "1 ns / 3", "-1 ns"):
        with pytest.raises(ValueError):
            cache.poll(now_ns=value)


def test_raw_addresses_and_format_selectors_stay_numeric() -> None:
    with pytest.raises(TypeError):
        messages.NoteOn(60, channel="1")  # ty: ignore[invalid-argument-type]
    with pytest.raises(TypeError):
        messages.ControlChange(7, 10, resolution="14 bit")  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        events.Timestamp(ticks="1 s", clock=events.Clock(identity="test"))
