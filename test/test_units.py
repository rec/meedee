"""Authored quantities normalize before reaching MIDI and scheduler code."""

import pytest
import tyro
from reccy.configuration import units

from meedee import (
    codecs,
    controls,
    events,
    messages,
    smf,
    testing,
    timecode,
    transfer,
    tuning,
    ump,
)
from meedee._cli import WatchPorts


def test_configuration_retains_authored_units_and_normalizes_cli() -> None:
    config = tyro.cli(
        WatchPorts,
        args=["--protocol", "midi1", "--interval", "250 ms", "--duration", "1:30"],
    )
    assert config.interval == 0.25
    assert config.duration == 90
    assert units.authored_dump(config)["interval"] == "250 ms"
    assert units.runtime_dump(config)["interval"] == 0.25
    assert WatchPorts.model_validate(units.revalidation_dump(config)) == config


def test_time_strings_and_numbers_produce_identical_transfer_plans() -> None:
    message = messages.SysEx(data=b"\x7d\x01")
    authored = transfer.Plan([message], gap_ns="2 ms", byte_limit="1 KiB")
    numeric = transfer.Plan([message], gap_ns=2_000_000, byte_limit=1024)
    assert authored.gap_ns == numeric.gap_ns == 2_000_000


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


@pytest.mark.parametrize("tempo", ["120 bpm", "500 ms / beat", "50000000"])
def test_ump_tempo_strings_encode_the_same_midi_values(tempo: str) -> None:
    assert ump.tempo(group=1, ten_nanoseconds_per_quarter=tempo) == ump.tempo(
        group=1, ten_nanoseconds_per_quarter=50_000_000
    )


def test_smf_tempo_conversion_preserves_event_ticks() -> None:
    clock = events.Clock(identity="test")
    origin = events.Timestamp(ticks=0, clock=clock)
    event = events.Event(
        stream="test",
        message=messages.NoteOn(channel=1, note=60, velocity=90),
        timestamp=events.Timestamp(ticks="1e9 tick", clock=clock),
    )
    header = smf.Header(format=0, tracks=1, division=480)
    assert (
        smf.export_event(event, header=header, origin=origin, tempo="120 bpm").events
        == smf.export_event(event, header=header, origin=origin, tempo=500_000).events
    )


def test_simulated_receive_accepts_seconds_and_packet_timestamps() -> None:
    source = testing.Input(
        stream="test", protocol="midi1", byte_limit="1 KiB", sysex_limit="512 byte"
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
        ump.tempo(group=1, ten_nanoseconds_per_quarter="1 ns / beat")
    with pytest.raises(ValueError):
        ump.tempo(group=1, ten_nanoseconds_per_quarter="0 bpm")


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
