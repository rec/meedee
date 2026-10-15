use meedee::{messages::*, ump::*};
fn vectors() -> Vec<(&'static str, Vec<u32>)> {
    include_str!("fixtures/ump.txt")
        .lines()
        .filter_map(|v| v.split_once('|'))
        .map(|(n, w)| {
            (
                n,
                w.split_whitespace()
                    .map(|v| u32::from_str_radix(v, 16).unwrap())
                    .collect(),
            )
        })
        .collect()
}
#[test]
fn independent_packets_cover_all_families_and_preserve_native_widths() {
    for (name, words) in vectors() {
        let value = decode(&words, false).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(encode(&value), words, "{name}");
        for n in 0..words.len() {
            assert!(decode(&words[..n], false).is_err());
        }
    }
    assert_eq!(
        encode(&Midi2NoteOn::new(16, 16, 127, 65535, 3, 65535).unwrap()),
        [0x4f9f7f03, 0xffffffff]
    );
    assert_eq!(
        encode(&Midi2ProgramChange::new(16, 16, 128, 1, 121, 3).unwrap()),
        [0x4fcf0001, 0x7f007903]
    );
    assert_eq!(
        encode(&Midi2RelativeRegisteredController::new(1, 1, 127, 127, -1).unwrap()),
        [0x40407f7f, 0xffffffff]
    );
    assert_eq!(
        encode(&Midi2PitchBend::new(1, 1, 0).unwrap()),
        [0x40e00000, 0x80000000]
    );
    assert!(Midi2NoteOn::new(0, 1, 0, 0, 0, 0).is_err());
    assert!(Midi2NoteOn::new(1, 17, 0, 0, 0, 0).is_err());
    assert!(Midi2NoteOn::new(1, 1, 128, 0, 0, 0).is_err());
    assert!(Midi2NoteOn::new(1, 1, 0, 65536, 0, 0).is_err());
    assert!(Midi2NoteOn::new(1, 1, 0, 0, 0, 1).is_err());
}
#[test]
fn framing_recovers_by_declared_lengths_and_opaque_preservation_is_explicit() {
    let all = vectors()
        .into_iter()
        .flat_map(|(_, w)| w)
        .collect::<Vec<_>>();
    for size in 1..=9 {
        let mut decoder = Decoder::new(false);
        let result = all
            .chunks(size)
            .flat_map(|c| decoder.feed(c))
            .collect::<Vec<_>>();
        assert!(decoder.finish().is_empty());
        assert_eq!(result.len(), 31);
        assert!(result.iter().all(|v| matches!(v, DecodeOutput::Message(_))));
    }
    let mut d = Decoder::new(false);
    let mut out = d.feed(&[0x40903c00, 0xffff0000, 0x60800000, 0x10f80000]);
    assert_eq!(out.len(), 3);
    assert!(matches!(out.remove(1), DecodeOutput::Diagnostic(_)));
    assert!(d.finish().is_empty());
    assert!(decode(&[0xb0000000, 1, 2], false).is_err());
    let opaque = decode(&[0xb0000000, 1, 2], true).unwrap();
    assert!(matches!(opaque, UmpMessage::Opaque(_)));
    assert_eq!(encode(&opaque), [0xb0000000, 1, 2]);
    d.feed(&[0x40903c00]);
    assert_eq!(d.finish().len(), 1);
    assert!(d.finish().is_empty());
    for little in [true, false] {
        assert_eq!(
            bytes_to_words(&words_to_bytes(&all, little), little).unwrap(),
            all
        );
    }
    assert!(bytes_to_words(&[0, 0, 0], false).is_err());
}
#[test]
fn sysex_reassembly_handles_groups_streams_interruptions_abort_and_resource_limits() {
    let data = (0..127).collect::<Vec<u8>>();
    let packets = packetize_sysex7(1, &data).unwrap();
    let mut a = Assembler::new(1024, 2048, 4).unwrap();
    let mut out = vec![];
    for p in packets {
        out.extend(a.feed(&UmpMessage::SysEx7(p)));
    }
    let [AssemblyItem::Data(v)] = out.as_slice() else {
        panic!("expected one payload")
    };
    assert_eq!(v.data, data);
    assert_eq!(a.retained_bytes(), 0);
    let s1 = packetize_sysex8(1, 1, &[255; 20]).unwrap();
    let s2 = packetize_sysex8(1, 2, &[128; 14]).unwrap();
    assert!(a.feed(&UmpMessage::SysEx8(s1[0].clone())).is_empty());
    assert!(a.feed(&UmpMessage::SysEx8(s2[0].clone())).is_empty());
    assert_eq!(a.active_streams(), 2);
    assert_eq!(
        a.feed(&UmpMessage::SysEx8(
            SysEx8Packet::new(1, 3, 1, vec![], true).unwrap()
        )),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::Aborted)]
    );
    let r = a.feed(&UmpMessage::SysEx8(s2[1].clone()));
    assert!(matches!(&r[0],AssemblyItem::Data(v) if v.data==[128;14]));
    assert_eq!(a.active_streams(), 0);
    a.feed(&UmpMessage::SysEx7(
        SysEx7Packet::new(1, 1, vec![1]).unwrap(),
    ));
    let clock = UmpMessage::Midi1(
        Midi1Packet::new(1, AnyMessage::TimingClock(TimingClock::new().unwrap())).unwrap(),
    );
    assert!(a.feed(&clock).is_empty());
    assert_eq!(a.active_streams(), 1);
    let on = UmpMessage::Midi2NoteOn(Midi2NoteOn::new(1, 1, 60, 1, 0, 0).unwrap());
    assert_eq!(
        a.feed(&on),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::Interrupted)]
    );
    let mut bounded = Assembler::new(4, 4, 1).unwrap();
    let start = UmpMessage::SysEx7(SysEx7Packet::new(1, 1, vec![1; 4]).unwrap());
    bounded.feed(&start);
    assert_eq!(bounded.retained_bytes(), 4);
    assert_eq!(
        bounded.feed(&UmpMessage::SysEx8(
            SysEx8Packet::new(2, 1, 0, vec![1], false).unwrap()
        )),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::StreamLimit)]
    );
    assert_eq!(
        bounded.feed(&UmpMessage::SysEx7(
            SysEx7Packet::new(1, 2, vec![1]).unwrap()
        )),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::PayloadLimit)]
    );
    assert_eq!(bounded.retained_bytes(), 0);
    assert!(bounded
        .feed(&UmpMessage::SysEx7(
            SysEx7Packet::new(1, 3, vec![]).unwrap()
        ))
        .is_empty());
    assert!(bounded.finish().is_empty());
}
#[test]
fn utf8_text_spans_packets_and_preserves_addressing_without_inventing_a_group() {
    let text = "A longer text with a split: 🎹🎹🎹";
    let mut a = Assembler::new(1024, 2048, 8).unwrap();
    let packets = packetize_flex_text(16, Some(16), 1, 0, text).unwrap();
    let mut result = vec![];
    for p in packets {
        result.extend(a.feed(&UmpMessage::FlexData(p)));
    }
    let [AssemblyItem::Data(v)] = result.as_slice() else {
        panic!("expected text")
    };
    assert_eq!(v.text(), Some(text));
    assert_eq!(v.channel, Some(16));
    let mut result = vec![];
    for p in packetize_stream_text(18, Some(31), text).unwrap() {
        result.extend(a.feed(&UmpMessage::Stream(p)));
    }
    let [AssemblyItem::Data(v)] = result.as_slice() else {
        panic!("expected stream text")
    };
    assert_eq!(v.text(), Some(text));
    assert_eq!(v.group, None);
    assert_eq!(v.function_block, Some(31));
    assert_eq!(v.stream_id, None);
    assert!(packetize_stream_text(4, None, "🎹").is_err());
    assert!(packetize_flex_text(1, None, 1, 0, &"x".repeat(385)).is_err());
    let bad = FlexData::new(1, 0, None, 1, 0, vec![0xff000000, 0, 0]).unwrap();
    assert_eq!(
        a.feed(&UmpMessage::FlexData(bad)),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::MalformedText)]
    );
}
#[test]
fn translation_preserves_centers_and_requires_permission_for_precision_loss() {
    for v in 0..=127 {
        let original = AnyMessage::NoteOn(NoteOn::new(1, 60, v).unwrap());
        let t = to_midi2(&original, 16).unwrap();
        assert_eq!(t.status, TranslationStatus::Exact);
        let r = to_midi1(&t.messages[0], false).unwrap();
        assert_eq!(r.status, TranslationStatus::Exact);
        if v != 0 {
            assert_eq!(r.messages, [original]);
        } else {
            assert!(matches!(&r.messages[0],AnyMessage::NoteOff(v) if v.velocity()==64));
        }
    }
    assert_eq!(scale_up(64, 7, 32).unwrap(), 0x80000000);
    assert_eq!(scale_up(127, 7, 32).unwrap(), 0xffffffff);
    assert_eq!(scale_up(1, 7, 16).unwrap(), 0x200);
    let v = UmpMessage::Midi2ControlChange(Midi2ControlChange::new(1, 1, 7, 0x80000001).unwrap());
    let denied = to_midi1(&v, false).unwrap();
    assert_eq!(denied.status, TranslationStatus::Quantized);
    assert!(!denied.converted());
    let allowed = to_midi1(&v, true).unwrap();
    assert!(allowed.converted());
    assert!(matches!(&allowed.messages[0],AnyMessage::ControlChange(v) if v.value()==64));
    let attributed = UmpMessage::Midi2NoteOn(Midi2NoteOn::new(1, 1, 60, 65535, 3, 30720).unwrap());
    assert_eq!(
        to_midi1(&attributed, true).unwrap().status,
        TranslationStatus::ProfileDependent
    );
    let bank =
        UmpMessage::Midi2ProgramChange(Midi2ProgramChange::new(1, 16, 128, 1, 121, 3).unwrap());
    assert_eq!(
        to_midi1(&bank, false)
            .unwrap()
            .messages
            .iter()
            .flat_map(|v| v.encode())
            .collect::<Vec<_>>(),
        [0xbf, 0, 121, 0xbf, 32, 3, 0xcf, 127]
    );
    let select = AnyMessage::ControlChange(ControlChange::new(1, 101, 0).unwrap());
    assert_eq!(
        to_midi2(&select, 1).unwrap().status,
        TranslationStatus::ContextRequired
    );
}

#[test]
fn rejected_layouts_and_long_lived_streams_remain_bounded_and_recoverable() {
    let mut seed = 0x12345678u32;
    let mut assembler = Assembler::new(32, 64, 2).unwrap();
    for _ in 0..4096 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut words = vec![seed];
        for _ in 1..word_count(seed) {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            words.push(seed);
        }
        let packet = decode(&words, true).unwrap();
        assert_eq!(encode(&packet), words);
        assembler.feed(&packet);
        assert!(assembler.retained_bytes() <= 64);
        assert!(assembler.active_streams() <= 2);
    }
    assembler.finish();
    assert_eq!(assembler.retained_bytes(), 0);
    assert_eq!(assembler.active_streams(), 0);
    let mut text = Assembler::new(1024, 2048, 4).unwrap();
    let start = FlexData::new(1, 1, None, 1, 0, vec![0x61616161; 3]).unwrap();
    text.feed(&UmpMessage::FlexData(start));
    let cont = FlexData::new(1, 2, None, 1, 0, vec![0x61616161; 3]).unwrap();
    for _ in 0..31 {
        text.feed(&UmpMessage::FlexData(cont.clone()));
    }
    let end = FlexData::new(1, 3, None, 1, 0, vec![0; 3]).unwrap();
    assert_eq!(
        text.feed(&UmpMessage::FlexData(end)),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::TooManyPackets)]
    );
    for bend in -8192..=8191 {
        let original = AnyMessage::PitchBend(PitchBend::new(16, bend).unwrap());
        let up = to_midi2(&original, 16).unwrap();
        assert_eq!(
            to_midi1(&up.messages[0], false).unwrap().messages,
            [original]
        );
    }
    let null = UmpMessage::Midi2RegisteredController(
        Midi2RegisteredController::new(1, 1, 127, 127, 0).unwrap(),
    );
    assert_eq!(
        to_midi1(&null, false).unwrap().status,
        TranslationStatus::Unsupported
    );
    let zero = UmpMessage::Midi2NoteOn(Midi2NoteOn::new(1, 1, 60, 0, 0, 0).unwrap());
    assert!(!to_midi1(&zero, false).unwrap().converted());
    assert!(
        matches!(&to_midi1(&zero,true).unwrap().messages[0],AnyMessage::NoteOn(v) if v.velocity()==1)
    );
}
