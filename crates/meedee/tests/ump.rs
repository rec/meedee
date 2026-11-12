use meedee::{messages::*, translation::*, ump::*};

#[test]
fn shared_channel_values_keep_versions_defaults_and_encoding_boundaries() {
    let registered = ParameterController::new(1, 1, 1, 2, u32::MAX.into()).unwrap();
    let assignable = registered
        .clone()
        .with_family(ControllerFamily::Assignable)
        .unwrap();
    assert_ne!(registered, assignable);
    assert_eq!(
        encode(&registered).unwrap()[0] ^ encode(&assignable).unwrap()[0],
        0x00100000
    );
    assert_eq!(
        assignable
            .with_family(ControllerFamily::Registered)
            .unwrap(),
        registered
    );
    let per_note =
        PerNoteController::new_with_family(1, 1, 60, 255, 1, ControllerFamily::Assignable).unwrap();
    assert!(per_note.with_family(ControllerFamily::Registered).is_err());
    let byte = NoteOn::new(60).unwrap().with_channel(1).unwrap();
    let wide = NoteOn::new_midi2(60).unwrap();
    assert_eq!(byte.version(), 1);
    assert_eq!(byte.group(), None);
    assert_eq!(wide.version(), 2);
    assert_eq!(wide.group(), Some(1));
    assert_eq!(wide.channel(), None);
    assert_eq!(u32::from(wide.velocity()), scale_up(90, 7, 16).unwrap());
    assert!(encode(&wide).is_err());
    assert!(encode(&byte).is_err());
    let wide = wide.with_channel(1).unwrap();
    assert!(wide.encode().is_err());
    assert_ne!(byte, wide);
    assert!(to_midi2(&AnyMessage::NoteOn(wide.clone()), 1).is_err());
    assert_eq!(byte.clone().with_group(1).unwrap().group(), Some(1));
    assert!(byte.with_attribute_type(0).is_err());
    assert_eq!(wide.clone().with_velocity(90).unwrap().velocity(), 90);
    let attributed = wide
        .with_attribute_type(3)
        .unwrap()
        .with_attribute_data(123)
        .unwrap();
    let shifted = attributed.transpose(12);
    assert_eq!(shifted.note(), 72);
    assert_eq!(shifted.attribute_data(), Some(123));
    assert_eq!(shifted.version(), 2);
    let bend = PitchBend::new_midi2()
        .unwrap()
        .with_value(i32::MAX.into())
        .unwrap()
        .with_channel(1)
        .unwrap();
    assert!(bend.encode().is_err());
    assert_eq!(encode(&bend).unwrap(), [0x40e00000, u32::MAX]);
}

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
        assert_eq!(encode(&value).unwrap(), words, "{name}");
        for n in 0..words.len() {
            assert!(decode(&words[..n], false).is_err());
        }
    }
    assert_eq!(
        encode(
            &meedee::messages::NoteOn::new_midi2(127)
                .and_then(|m| m.with_group(16))
                .and_then(|m| m.with_channel(16))
                .and_then(|m| m.with_velocity(65535))
                .and_then(|m| m.with_attribute_type(3))
                .and_then(|m| m.with_attribute_data(65535))
                .unwrap()
        )
        .unwrap(),
        [0x4f9f7f03, 0xffffffff]
    );
    assert_eq!(
        encode(
            &meedee::messages::ProgramChange::new_midi2(128)
                .and_then(|m| m.with_group(16))
                .and_then(|m| m.with_channel(16))
                .and_then(|m| m.with_bank(121, 3))
                .unwrap()
        )
        .unwrap(),
        [0x4fcf0001, 0x7f007903]
    );
    assert_eq!(
        encode(&RelativeParameterController::new(1, 1, 127, 127, -1).unwrap()).unwrap(),
        [0x40407f7f, 0xffffffff]
    );
    assert_eq!(
        encode(
            &meedee::messages::PitchBend::new_midi2()
                .and_then(|m| m.with_group(1))
                .and_then(|m| m.with_channel(1))
                .and_then(|m| m.with_value(0))
                .unwrap()
        )
        .unwrap(),
        [0x40e00000, 0x80000000]
    );
    assert!(meedee::messages::NoteOn::new_midi2(0)
        .and_then(|m| m.with_group(0))
        .and_then(|m| m.with_channel(1))
        .and_then(|m| m.with_velocity(0))
        .and_then(|m| m.with_attribute_type(0))
        .and_then(|m| m.with_attribute_data(0))
        .is_err());
    assert!(meedee::messages::NoteOn::new_midi2(0)
        .and_then(|m| m.with_group(1))
        .and_then(|m| m.with_channel(17))
        .and_then(|m| m.with_velocity(0))
        .and_then(|m| m.with_attribute_type(0))
        .and_then(|m| m.with_attribute_data(0))
        .is_err());
    assert!(meedee::messages::NoteOn::new_midi2(128)
        .and_then(|m| m.with_group(1))
        .and_then(|m| m.with_channel(1))
        .and_then(|m| m.with_velocity(0))
        .and_then(|m| m.with_attribute_type(0))
        .and_then(|m| m.with_attribute_data(0))
        .is_err());
    assert!(meedee::messages::NoteOn::new_midi2(0)
        .and_then(|m| m.with_group(1))
        .and_then(|m| m.with_channel(1))
        .and_then(|m| m.with_velocity(65536))
        .and_then(|m| m.with_attribute_type(0))
        .and_then(|m| m.with_attribute_data(0))
        .is_err());
    assert!(meedee::messages::NoteOn::new_midi2(0)
        .and_then(|m| m.with_group(1))
        .and_then(|m| m.with_channel(1))
        .and_then(|m| m.with_velocity(0))
        .and_then(|m| m.with_attribute_type(0))
        .and_then(|m| m.with_attribute_data(1))
        .is_err());
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
    assert!(matches!(
        opaque,
        meedee::events::Payload::Ump(PacketValue::Opaque(_))
    ));
    assert_eq!(encode(&opaque).unwrap(), [0xb0000000, 1, 2]);
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
    let packets =
        packetize_sysex7(&SysEx::new(data.clone()).unwrap().with_group(1).unwrap()).unwrap();
    let mut a = Assembler::new(1024, 2048, 4).unwrap();
    let mut out = vec![];
    for p in packets {
        out.extend(assembly_values(
            a.feed(&meedee::events::Payload::Ump(
                (PacketValue::SysEx7(p)).clone(),
            ))
            .unwrap(),
        ));
    }
    let [AssemblyItem::Data(v)] = out.as_slice() else {
        panic!("expected one payload")
    };
    assert_eq!(v.data, data);
    assert_eq!(a.retained_bytes(), 0);
    let s1 = packetize_sysex8(1, 1, &[255; 20]).unwrap();
    let s2 = packetize_sysex8(1, 2, &[128; 14]).unwrap();
    assert!(assembly_values(
        a.feed(&meedee::events::Payload::Ump(
            (PacketValue::SysEx8(s1[0].clone())).clone()
        ))
        .unwrap()
    )
    .is_empty());
    assert!(assembly_values(
        a.feed(&meedee::events::Payload::Ump(
            (PacketValue::SysEx8(s2[0].clone())).clone()
        ))
        .unwrap()
    )
    .is_empty());
    assert_eq!(a.active_streams(), 2);
    assert_eq!(
        assembly_values(
            a.feed(&meedee::events::Payload::Ump(
                (PacketValue::SysEx8(SysEx8Packet::new(1, 3, 1, vec![], true).unwrap())).clone()
            ))
            .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::Aborted)]
    );
    let r = assembly_values(
        a.feed(&meedee::events::Payload::Ump(
            (PacketValue::SysEx8(s2[1].clone())).clone(),
        ))
        .unwrap(),
    );
    assert!(matches!(&r[0],AssemblyItem::Data(v) if v.data==[128;14]));
    assert_eq!(a.active_streams(), 0);
    assembly_values(
        a.feed(&meedee::events::Payload::Ump(
            (PacketValue::SysEx7(SysEx7Packet::new(1, 1, vec![1]).unwrap())).clone(),
        ))
        .unwrap(),
    );
    let clock = meedee::events::Payload::from(TimingClock::new().unwrap().with_group(1).unwrap());
    assert!(assembly_values(a.feed(&clock).unwrap()).is_empty());
    assert_eq!(a.active_streams(), 1);
    let on = meedee::messages::AnyMessage::NoteOn(
        meedee::messages::NoteOn::new_midi2(60)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(1))
            .and_then(|m| m.with_velocity(1))
            .and_then(|m| m.with_attribute_type(0))
            .and_then(|m| m.with_attribute_data(0))
            .unwrap(),
    );
    assert_eq!(
        assembly_values(
            a.feed(&meedee::events::Payload::Message(on.clone()))
                .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::Interrupted)]
    );
    let mut bounded = Assembler::new(4, 4, 1).unwrap();
    let start = PacketValue::SysEx7(SysEx7Packet::new(1, 1, vec![1; 4]).unwrap());
    assembly_values(
        bounded
            .feed(&meedee::events::Payload::Ump((start).clone()))
            .unwrap(),
    );
    assert_eq!(bounded.retained_bytes(), 4);
    assert_eq!(
        assembly_values(
            bounded
                .feed(&meedee::events::Payload::Ump(
                    (PacketValue::SysEx8(SysEx8Packet::new(2, 1, 0, vec![1], false).unwrap()))
                        .clone()
                ))
                .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::StreamLimit)]
    );
    assert_eq!(
        assembly_values(
            bounded
                .feed(&meedee::events::Payload::Ump(
                    (PacketValue::SysEx7(SysEx7Packet::new(1, 2, vec![1]).unwrap())).clone()
                ))
                .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::PayloadLimit)]
    );
    assert_eq!(bounded.retained_bytes(), 0);
    assert!(assembly_values(
        bounded
            .feed(&meedee::events::Payload::Ump(
                (PacketValue::SysEx7(SysEx7Packet::new(1, 3, vec![]).unwrap())).clone()
            ))
            .unwrap()
    )
    .is_empty());
    assert!(assembly_values(bounded.finish()).is_empty());
}
#[test]
fn utf8_text_spans_packets_and_preserves_addressing_without_inventing_a_group() {
    let text = "A longer text with a split: 🎹🎹🎹";
    let mut a = Assembler::new(1024, 2048, 8).unwrap();
    let packets = packetize_flex_text(16, Some(16), 1, 0, text).unwrap();
    let mut result = vec![];
    for p in packets {
        result.extend(assembly_values(
            a.feed(&meedee::events::Payload::Ump(
                (PacketValue::FlexData(p)).clone(),
            ))
            .unwrap(),
        ));
    }
    let [AssemblyItem::Data(v)] = result.as_slice() else {
        panic!("expected text")
    };
    assert_eq!(v.text(), Some(text));
    assert_eq!(v.channel, Some(16));
    let mut result = vec![];
    for p in packetize_stream_text(18, Some(31), text).unwrap() {
        result.extend(assembly_values(
            a.feed(&meedee::events::Payload::Ump(
                (PacketValue::Stream(p)).clone(),
            ))
            .unwrap(),
        ));
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
        assembly_values(
            a.feed(&meedee::events::Payload::Ump(
                (PacketValue::FlexData(bad)).clone()
            ))
            .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::MalformedText)]
    );
}
#[test]
fn translation_preserves_centers_and_requires_permission_for_precision_loss() {
    for v in 0..=127 {
        let original = AnyMessage::NoteOn(
            NoteOn::new(60)
                .and_then(|m| m.with_velocity(v))
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        );
        let t = to_midi2(&original, 16).unwrap();
        assert_eq!(t.status, TranslationStatus::Exact);
        let r = to_midi1(&t.messages[0].clone().into(), false).unwrap();
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
    let v = meedee::messages::AnyMessage::ControlChange(
        meedee::messages::ControlChange::new_midi2(7, 0x80000001)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(1))
            .unwrap(),
    );
    let denied = to_midi1(&v.clone().into(), false).unwrap();
    assert_eq!(denied.status, TranslationStatus::Quantized);
    assert!(!denied.converted());
    let allowed = to_midi1(&v.clone().into(), true).unwrap();
    assert!(allowed.converted());
    assert!(matches!(&allowed.messages[0],AnyMessage::ControlChange(v) if v.value()==64));
    let attributed = meedee::messages::AnyMessage::NoteOn(
        meedee::messages::NoteOn::new_midi2(60)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(1))
            .and_then(|m| m.with_velocity(65535))
            .and_then(|m| m.with_attribute_type(3))
            .and_then(|m| m.with_attribute_data(30720))
            .unwrap(),
    );
    assert_eq!(
        to_midi1(&attributed.clone().into(), true).unwrap().status,
        TranslationStatus::ProfileDependent
    );
    let bank = meedee::messages::AnyMessage::ProgramChange(
        meedee::messages::ProgramChange::new_midi2(128)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(16))
            .and_then(|m| m.with_bank(121, 3))
            .unwrap(),
    );
    assert_eq!(
        to_midi1(&bank.clone().into(), false)
            .unwrap()
            .messages
            .iter()
            .flat_map(|v| v.encode().unwrap())
            .collect::<Vec<_>>(),
        [0xbf, 0, 121, 0xbf, 32, 3, 0xcf, 127]
    );
    let select = AnyMessage::ControlChange(
        ControlChange::new(101, 0)
            .and_then(|m| m.with_channel(1))
            .unwrap(),
    );
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
        assert_eq!(encode(&packet).unwrap(), words);
        assembly_values(assembler.feed(&packet).unwrap());
        assert!(assembler.retained_bytes() <= 64);
        assert!(assembler.active_streams() <= 2);
    }
    assembly_values(assembler.finish());
    assert_eq!(assembler.retained_bytes(), 0);
    assert_eq!(assembler.active_streams(), 0);
    let mut text = Assembler::new(1024, 2048, 4).unwrap();
    let start = FlexData::new(1, 1, None, 1, 0, vec![0x61616161; 3]).unwrap();
    assembly_values(
        text.feed(&meedee::events::Payload::Ump(
            (PacketValue::FlexData(start)).clone(),
        ))
        .unwrap(),
    );
    let cont = FlexData::new(1, 2, None, 1, 0, vec![0x61616161; 3]).unwrap();
    for _ in 0..31 {
        assembly_values(
            text.feed(&meedee::events::Payload::Ump(
                (PacketValue::FlexData(cont.clone())).clone(),
            ))
            .unwrap(),
        );
    }
    let end = FlexData::new(1, 3, None, 1, 0, vec![0; 3]).unwrap();
    assert_eq!(
        assembly_values(
            text.feed(&meedee::events::Payload::Ump(
                (PacketValue::FlexData(end)).clone()
            ))
            .unwrap()
        ),
        [AssemblyItem::Diagnostic(AssemblyDiagnostic::TooManyPackets)]
    );
    for bend in -8192..=8191 {
        let original = AnyMessage::PitchBend(
            PitchBend::new()
                .and_then(|m| m.with_value(bend))
                .and_then(|m| m.with_channel(16))
                .unwrap(),
        );
        let up = to_midi2(&original, 16).unwrap();
        assert_eq!(
            to_midi1(&up.messages[0].clone().into(), false)
                .unwrap()
                .messages,
            [original]
        );
    }
    let null = meedee::messages::AnyMessage::ParameterController(
        ParameterController::new(1, 1, 127, 127, 0).unwrap(),
    );
    assert_eq!(
        to_midi1(&null.clone().into(), false).unwrap().status,
        TranslationStatus::Unsupported
    );
    let zero = meedee::messages::AnyMessage::NoteOn(
        meedee::messages::NoteOn::new_midi2(60)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(1))
            .and_then(|m| m.with_velocity(0))
            .and_then(|m| m.with_attribute_type(0))
            .and_then(|m| m.with_attribute_data(0))
            .unwrap(),
    );
    assert!(!to_midi1(&zero.clone().into(), false).unwrap().converted());
    assert!(
        matches!(&to_midi1(&zero.clone().into(),true).unwrap().messages[0],AnyMessage::NoteOn(v) if v.velocity()==1)
    );
}

fn assembly_values(observations: Vec<meedee::events::Observation>) -> Vec<AssemblyItem> {
    observations
        .into_iter()
        .map(|v| match v.value {
            meedee::events::ObservationValue::Ump(v) => v,
            _ => panic!("UMP observation expected"),
        })
        .collect()
}
