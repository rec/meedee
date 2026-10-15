"""Independent UMP vectors, immutable values and bounded source-local engines."""

import inspect
from pathlib import Path

import pytest

from meedee import gm, messages, ump


def vectors() -> list[tuple[str, tuple[int, ...]]]:
    fixture = (
        Path(__file__).resolve().parents[1] / "crates/meedee/tests/fixtures/ump.txt"
    )
    return [
        (n, tuple(int(v, 16) for v in w.split()))
        for x in fixture.read_text().splitlines()
        if "|" in x
        for n, w in [x.split("|")]
    ]


def test_shared_vectors_preserve_every_family_and_frozen_native_values() -> None:
    for name, words in vectors():
        value = ump.decode(words)
        assert isinstance(value, messages.Message)
        assert ump.encode(value) == words
        assert type(value).__module__ == "meedee.ump"
        constructor = getattr(ump, type(value).__name__)
        fields = constructor.__match_args__
        clone = constructor(**{f: getattr(value, f) for f in fields})
        assert clone == value and clone.replace() == value
        assert hash(clone) == hash(value)
        assert repr(value).startswith(type(value).__name__ + "(")
        for f in fields:
            assert (
                inspect.signature(type(value)).parameters[f].kind
                == inspect.Parameter.KEYWORD_ONLY
            )
            with pytest.raises(AttributeError):
                setattr(value, f, None)
        with pytest.raises(TypeError):
            clone.replace(unknown=1)
        if name not in {
            "System",
            "Abort",
            "TimeSignature",
            "EndpointDiscovery",
            "EndpointInfo",
            "StartOfClip",
        }:
            assert type(value).__name__ == name
    on = ump.Midi2NoteOn(
        group=16,
        channel=16,
        note=127,
        velocity=65535,
        attribute_type=3,
        attribute_data=65535,
    )
    assert ump.encode(on) == (0x4F9F7F03, 0xFFFFFFFF)
    assert on.replace(group=1).group == 1
    assert not hasattr(ump.JrClock(ticks=0), "group")
    assert not hasattr(ump.start_of_clip(), "group")
    for field in inspect.signature(type(on)).parameters:
        for v in (True, 1.0, "1", None):
            with pytest.raises(TypeError):
                on.replace(**{field: v})
    for changes in (
        {"group": 0},
        {"channel": 17},
        {"note": 128},
        {"velocity": 65536},
        {"attribute_type": 4},
        {"attribute_type": 0, "attribute_data": 1},
    ):
        with pytest.raises(ValueError):
            on.replace(**changes)


def test_word_framing_recovers_without_confusing_byte_order_or_transport_buffers() -> (
    None
):
    words = tuple(w for _, v in vectors() for w in v)
    for n in range(1, 10):
        decoder = ump.Decoder()
        result = [
            v for i in range(0, len(words), n) for v in decoder.feed(words[i : i + n])
        ]
        assert len(result) == len(vectors()) and all(
            isinstance(v, messages.Message) for v in result
        )
        assert decoder.finish() == []
    decoder = ump.Decoder()
    result = decoder.feed((0x40903C00, 0xFFFF0000, 0x60000000, 0x10F80000))
    assert isinstance(result[1], ump.DecodeDiagnostic) and result[1].word_offset == 2
    assert isinstance(result[2], ump.Midi1Packet)
    assert decoder.feed((0x40903C00,)) == []
    assert len(decoder.finish()) == 1 and decoder.finish() == []
    with pytest.raises(ValueError):
        ump.decode((0xB0000000, 1, 2))
    opaque = ump.decode((0xB0000000, 1, 2), preserve_unknown=True)
    assert isinstance(opaque, ump.OpaquePacket) and opaque.words == (0xB0000000, 1, 2)
    for order in ("little", "big"):
        raw = ump.words_to_bytes(words, byteorder=order)
        assert ump.bytes_to_words(memoryview(bytearray(raw)), byteorder=order) == words
    assert ump.words_to_bytes((0x40903C00,), byteorder="big") == b"\x40\x90\x3c\x00"
    assert ump.words_to_bytes((0x40903C00,), byteorder="little") == b"\x00\x3c\x90\x40"
    with pytest.raises(ValueError):
        ump.bytes_to_words(b"abc", byteorder="big")
    with pytest.raises(ValueError):
        ump.words_to_bytes((0,), byteorder="native")
    for bad in ((True,), (1.0,), ("1",)):
        with pytest.raises(TypeError):
            ump.decode(bad)  # ty: ignore[invalid-argument-type]
    with pytest.raises(ValueError):
        ump.decode((1 << 32,))
    with pytest.raises(TypeError):
        ump.Decoder(preserve_unknown=1)  # ty: ignore[invalid-argument-type]


def test_sysex_assembly_is_scoped_bounded_and_discards_incomplete_payloads() -> None:
    data = bytes(range(128))
    assembler = ump.Assembler()
    result = [
        v for p in ump.packetize_sysex7(group=16, data=data) for v in assembler.feed(p)
    ]
    assert len(result) == 1 and isinstance(result[0], ump.AssembledData)
    assert result[0].data == data and result[0].group == 16
    assert assembler.active_streams == assembler.retained_bytes == 0
    one = ump.packetize_sysex8(group=1, stream_id=1, data=b"\xff" * 20)
    two = ump.packetize_sysex8(group=1, stream_id=2, data=b"\x80" * 14)
    assembler.feed(one[0])
    assembler.feed(two[0])
    assert assembler.active_streams == 2
    assert assembler.feed(
        ump.SysEx8Packet(group=1, format=3, stream_id=1, data=b"", aborted=True)
    ) == [ump.AssemblyDiagnostic.Aborted]
    result = assembler.feed(two[1])
    assert isinstance(result[0], ump.AssembledData) and result[0].data == b"\x80" * 14
    assert result[0].stream_id == 2
    assembler.feed(ump.SysEx7Packet(group=1, format=1, data=b"\x7d"))
    assert (
        assembler.feed(ump.Midi1Packet(group=1, message=messages.TimingClock())) == []
    )
    assert (
        assembler.feed(
            ump.Midi1Packet(
                group=2, message=messages.NoteOn(channel=1, note=60, velocity=100)
            )
        )
        == []
    )
    assert assembler.feed(
        ump.Midi1Packet(
            group=1, message=messages.NoteOn(channel=1, note=60, velocity=100)
        )
    ) == [ump.AssemblyDiagnostic.Interrupted]
    small = ump.Assembler(message_limit=4, total_limit=4, max_streams=1)
    small.feed(ump.SysEx7Packet(group=1, format=1, data=b"abcd"))
    assert small.feed(ump.SysEx8Packet(group=2, format=1, stream_id=0, data=b"x")) == [
        ump.AssemblyDiagnostic.StreamLimit
    ]
    assert small.feed(ump.SysEx7Packet(group=1, format=2, data=b"x")) == [
        ump.AssemblyDiagnostic.PayloadLimit
    ]
    assert small.retained_bytes == 0
    assert small.feed(ump.SysEx7Packet(group=1, format=3, data=b"")) == []
    assert small.finish() == []
    small.feed(ump.SysEx7Packet(group=1, format=1, data=b"x"))
    assert small.finish() == [ump.AssemblyDiagnostic.Truncated]
    with pytest.raises(ValueError):
        ump.packetize_sysex7(group=1, data=b"\x80")
    with pytest.raises(ValueError):
        ump.packetize_sysex8(group=1, stream_id=0, data=b"xx", message_limit=1)
    mutable = bytearray(b"abc")
    packet = ump.SysEx8Packet(group=1, format=0, stream_id=0, data=mutable)
    mutable.clear()
    assert packet.data == b"abc"


def test_flex_and_stream_text_validate_complete_utf8_and_protocol_lengths() -> None:
    text = "A split UTF-8 sequence: 🎹🎹🎹"
    assembler = ump.Assembler()
    packets = ump.packetize_flex_text(
        group=1, channel=16, status_bank=1, status=0, text=text
    )
    result = [v for p in packets for v in assembler.feed(p)]
    assert len(result) == 1 and isinstance(result[0], ump.AssembledData)
    assert (result[0].text, result[0].channel, result[0].status_bank) == (text, 16, 1)
    packets = ump.packetize_stream_text(status=18, function_block=31, text=text)
    result = [v for p in packets for v in assembler.feed(p)]
    assert isinstance(result[0], ump.AssembledData)
    assert (result[0].text, result[0].group, result[0].function_block) == (
        text,
        None,
        31,
    )
    for kwargs in (
        {"status": 3, "function_block": None, "text": "x" * 99},
        {"status": 4, "function_block": None, "text": "🎹"},
        {"status": 18, "function_block": None, "text": "x"},
    ):
        with pytest.raises(ValueError):
            ump.packetize_stream_text(**kwargs)  # ty: ignore[invalid-argument-type]
    for text in ("x" * 385, "\ufeffabc", "ab\0cd"):
        with pytest.raises(ValueError):
            ump.packetize_flex_text(
                group=1, channel=None, status_bank=1, status=0, text=text
            )
    invalid = ump.FlexData(
        group=1,
        format=0,
        channel=None,
        status_bank=1,
        status=0,
        data=(0xFF000000, 0, 0),
    )
    assert assembler.feed(invalid) == [ump.AssemblyDiagnostic.MalformedText]
    assert ump.encode(ump.tempo(group=1, ten_nanoseconds_per_quarter=50_000_000)) == (
        0xD0100000,
        0x02FAF080,
        0,
        0,
    )
    assert ump.encode(
        ump.time_signature(
            group=1, numerator=4, denominator_power=2, thirtyseconds_per_quarter=8
        )
    ) == (0xD0100001, 0x04020800, 0, 0)
    assert (
        ump.time_signature(
            group=1, numerator=256, denominator_power=0, thirtyseconds_per_quarter=8
        ).data[0]
        == 0x00000800
    )


def test_endpoint_factories_and_capabilities_preserve_unknown_support() -> None:
    assert ump.encode(ump.endpoint_discovery(major=1, minor=1, filter=31)) == (
        0xF0000101,
        31,
        0,
        0,
    )
    info = ump.endpoint_info(
        major=1,
        minor=1,
        static_blocks=True,
        function_blocks=2,
        midi1=True,
        midi2=True,
        rx_jr=True,
        tx_jr=True,
    )
    assert ump.encode(info) == (0xF0010101, 0x82000303, 0, 0)
    identity = ump.device_identity(
        manufacturer=b"\x41\0\0", family=257, model=16383, revision=b"\0\1\2\x7f"
    )
    assert ump.encode(identity) == (0xF0020000, 0x00410000, 0x01027F7F, 0x0001027F)
    assert (
        ump.stream_configuration(
            request=True, protocol=2, rx_jr=True, tx_jr=False
        ).status
        == 5
    )
    assert ump.function_block_discovery(block=255, filter=3).data[:2] == b"\xff\x03"
    info = ump.function_block_info(
        active=True,
        block=31,
        ui_hint=0,
        midi1_port=0,
        direction=3,
        first_group=16,
        groups=1,
        midi_ci_version=1,
        sysex8_streams=2,
    )
    assert ump.decode(ump.encode(info)) == info
    with pytest.raises(ValueError):
        info.replace(data=bytes([159, 3, 15, 2, 1, 2]) + bytes(8))
    with pytest.raises(TypeError):
        ump.endpoint_info(
            major=1,
            minor=1,
            static_blocks=1,  # ty: ignore[invalid-argument-type]
            function_blocks=1,
            midi1=True,
            midi2=True,
            rx_jr=False,
            tx_jr=False,
        )
    caps = ump.Capabilities(midi1=False, midi2=True, sysex8_streams=1)
    note = ump.Midi2NoteOn(
        group=1, channel=1, note=60, velocity=65535, attribute_type=0, attribute_data=0
    )
    assert caps.supports(note) is True
    assert (
        caps.supports(
            ump.Midi1Packet(
                group=1, message=messages.NoteOn(channel=1, note=60, velocity=100)
            )
        )
        is False
    )
    assert (
        caps.supports(ump.Midi1Packet(group=1, message=messages.TimingClock())) is True
    )
    assert (
        caps.supports(ump.SysEx8Packet(group=1, format=0, stream_id=1, data=b""))
        is False
    )
    assert caps.supports(ump.start_of_clip()) is None


def test_translation_reports_precision_and_never_guesses_compound_context() -> None:
    for v in range(1, 128):
        original = messages.NoteOn(channel=16, note=60, velocity=v)
        upgraded = ump.to_midi2(original, group=16)
        assert upgraded.converted and upgraded.status == ump.TranslationStatus.Exact
        assert ump.to_midi1(upgraded.messages[0]).messages == [original]
    zero = ump.to_midi2(messages.NoteOn(channel=1, note=60, velocity=0), group=1)
    assert (
        isinstance(zero.messages[0], ump.Midi2NoteOff)
        and zero.messages[0].velocity == 32768
    )
    fine = ump.Midi2ControlChange(group=1, channel=1, controller=7, value=0x80000001)
    denied = ump.to_midi1(fine)
    assert (
        not denied.converted
        and denied.messages == []
        and denied.status == ump.TranslationStatus.Quantized
    )
    accepted = ump.to_midi1(fine, allow_loss=True)
    assert accepted.messages == [
        messages.ControlChange(channel=1, controller=7, value=64)
    ]
    bank = ump.Midi2ProgramChange(
        group=16, channel=16, program=128, options=1, bank_msb=121, bank_lsb=3
    )
    assert ump.to_midi1(bank).messages == [
        messages.ControlChange(channel=16, controller=0, value=121),
        messages.ControlChange(channel=16, controller=32, value=3),
        messages.ProgramChange(channel=16, program=128),
    ]
    attributed = ump.Midi2NoteOn(
        group=1,
        channel=1,
        note=60,
        velocity=65535,
        attribute_type=3,
        attribute_data=30720,
    )
    assert (
        ump.to_midi1(attributed, allow_loss=True).status
        == ump.TranslationStatus.ProfileDependent
    )
    context = ump.to_midi2(
        messages.ControlChange(channel=1, controller=101, value=0), group=1
    )
    assert (
        context.status == ump.TranslationStatus.ContextRequired
        and not context.converted
    )
    raw_mode = ump.to_midi2(gm.Gm1On(device_id=127), group=1)
    assert isinstance(raw_mode.messages[0], ump.SysEx7Packet)
    assert ump.scale_up(value=64, source_bits=7, destination_bits=32) == 0x80000000
    assert ump.scale_up(value=127, source_bits=7, destination_bits=32) == 0xFFFFFFFF
    with pytest.raises(TypeError):
        ump.to_midi1(fine, allow_loss=1)  # ty: ignore[invalid-argument-type]
