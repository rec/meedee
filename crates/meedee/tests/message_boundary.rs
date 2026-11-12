use meedee::events::{Clock, Payload, SourceSpan, Timestamp};
use meedee::messages::{
    AnyMessage, Command, ControlChange, Message, Metadata, NoteOn, PrimitiveMessage, SysEx,
    TimingClock,
};
use meedee::{archive, translation, ump};
use std::io::Cursor;

#[test]
fn midi1_ump_uses_shared_values_and_explicit_group_removal() {
    let caps = ump::Capabilities::new(false, true, 0).unwrap();
    assert_eq!(
        caps.supports(&Payload::from(NoteOn::new(60).unwrap())),
        Some(false)
    );
    assert_eq!(
        caps.supports(&Payload::from(TimingClock::new().unwrap())),
        Some(true)
    );
    let value = NoteOn::new(60)
        .unwrap()
        .with_velocity(127)
        .unwrap()
        .with_channel(16)
        .unwrap()
        .with_group(16)
        .unwrap()
        .with_stream("keys")
        .unwrap();
    assert_eq!(value.version(), 1);
    assert_eq!(ump::encode(&value).unwrap(), [0x2f9f3c7f]);
    assert_eq!(
        ump::decode(&[0x2f9f3c7f], false).unwrap(),
        Payload::from(value.clone())
    );
    assert!(value.encode().is_err());
    let shifted = value.with_channel(2).unwrap().transpose(1);
    assert_eq!(shifted.stream(), Some("keys"));
    assert_eq!(shifted.group(), Some(16));
    assert_eq!(
        shifted.without_group().unwrap().encode().unwrap(),
        [0x91, 61, 127]
    );
    assert_eq!(
        ump::encode(&TimingClock::new().unwrap().with_group(2).unwrap()).unwrap(),
        [0x11f80000]
    );
    assert!(ump::encode(&TimingClock::new().unwrap()).is_err());
}

#[test]
fn grouped_complete_sysex_commands_and_native_values_keep_archive_categories() {
    let sysex = SysEx::new((0..20).collect())
        .unwrap()
        .with_group(3)
        .unwrap();
    let packets = ump::packetize_sysex7(&sysex).unwrap();
    assert_eq!(packets.len(), 4);
    assert_eq!(ump::encode(&sysex).unwrap().len(), 8);
    let command = ControlChange::new_with_resolution(7, 10000, 14)
        .unwrap()
        .with_channel(2)
        .unwrap()
        .with_group(3)
        .unwrap();
    let expanded = command.expand().unwrap();
    assert!(expanded
        .iter()
        .all(|m| m.group() == Some(3) && m.version() == 1));
    assert_eq!(
        ump::encode(&Payload::from(command.clone())).unwrap(),
        [0x22b1074e, 0x22b12710]
    );
    assert!(Payload::Message(AnyMessage::ControlChange(command.clone()))
        .validate()
        .is_err());
    let native = meedee::messages::PerNotePitchBend::new(3, 2, 60, i32::MIN.into()).unwrap();
    let values = vec![
        Payload::from(sysex),
        Payload::from(command),
        Payload::from(native),
        Payload::Ump(ump::PacketValue::SysEx7(packets[0].clone())),
    ];
    let records = values
        .into_iter()
        .map(|v| archive::Record::Event(Box::new(v.with_stream("keys").unwrap())))
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    let mut writer = archive::Writer::new(&mut bytes, 4096).unwrap();
    writer
        .write(&archive::Record::Stream {
            id: "keys".into(),
            label: String::new(),
            clock_identity: None,
        })
        .unwrap();
    for record in &records {
        writer.write(record).unwrap();
    }
    writer.finish(true, "").unwrap();
    drop(writer);
    let mut reader = archive::Reader::new(Cursor::new(bytes), 4096, false, false).unwrap();
    let restored = reader.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(&restored[1..], records.as_slice());
    assert!(reader.complete);
}

#[test]
fn translation_preserves_metadata_and_never_scales_during_packaging() {
    let value = NoteOn::new(60)
        .unwrap()
        .with_channel(2)
        .unwrap()
        .with_velocity(90)
        .unwrap()
        .with_stream("keys")
        .unwrap();
    let packaged = value.clone().with_group(3).unwrap();
    assert_eq!(ump::encode(&packaged).unwrap(), [0x22913c5a]);
    let converted = translation::to_midi2(&AnyMessage::NoteOn(value.clone()), 3).unwrap();
    assert_eq!(converted.messages[0].stream(), Some("keys"));
    let restored = translation::to_midi1(&converted.messages[0].clone().into(), false).unwrap();
    assert_eq!(restored.messages, [AnyMessage::NoteOn(value)]);
    assert_eq!(restored.messages[0].stream(), Some("keys"));
}

#[test]
fn opaque_archives_preserve_raw_identity_for_known_and_unknown_packets() {
    let words = [
        vec![0x2f9f3c7f],
        vec![0x4f9f3c00, 0xffff0000],
        vec![0],
        vec![0x3f037d01, 0x02000000],
        vec![0xb0000000, 1, 2],
    ];
    let mut records = vec![archive::Record::Stream {
        id: "keys".into(),
        label: String::new(),
        clock_identity: None,
    }];
    records.extend(words.iter().map(|w| {
        archive::Record::Event(Box::new(
            Payload::from(ump::OpaquePacket::new(w.clone()).unwrap())
                .with_stream("keys")
                .unwrap(),
        ))
    }));
    for preserve in [false, true] {
        let restored = round_trip(&records, preserve);
        assert_eq!(restored, records);
        for (record, words) in restored[1..].iter().zip(&words) {
            let archive::Record::Event(value) = record else {
                panic!("expected an event");
            };
            let Payload::Ump(ump::PacketValue::Opaque(value)) = value.as_ref() else {
                panic!("lost opaque identity");
            };
            assert_eq!(value.words(), words);
            assert_eq!(value.stream(), Some("keys"));
        }
    }
}

#[test]
fn midi2_group_removal_is_rejected_for_all_shared_messages() {
    let values = [
        AnyMessage::NoteOn(NoteOn::new_midi2(60).unwrap()),
        AnyMessage::NoteOff(meedee::messages::NoteOff::new_midi2(60).unwrap()),
        AnyMessage::PolyPressure(meedee::messages::PolyPressure::new_midi2(60).unwrap()),
        AnyMessage::ControlChange(ControlChange::new_midi2(7, 123456).unwrap()),
        AnyMessage::ProgramChange(meedee::messages::ProgramChange::new_midi2(3).unwrap()),
        AnyMessage::ChannelPressure(meedee::messages::ChannelPressure::new_midi2().unwrap()),
        AnyMessage::PitchBend(meedee::messages::PitchBend::new_midi2().unwrap()),
    ];
    for value in values {
        let value = value.with_group(9).unwrap();
        assert!(value
            .clone()
            .without_group()
            .unwrap_err()
            .to_string()
            .contains("MIDI 2 requires a group"));
        assert_eq!(value.group(), Some(9));
        assert_eq!(value.version(), 2);
        assert_eq!(value.with_group(3).unwrap().group(), Some(3));
    }
}

#[test]
fn direct_ump_controller_encoding_preserves_native_resolution() {
    let cases = [
        (ControlChange::new(7, 90).unwrap(), vec![0x22b1075a]),
        (
            ControlChange::new_with_resolution(7, 10000, 14).unwrap(),
            vec![0x22b1074e, 0x22b12710],
        ),
        (
            ControlChange::new_midi2(7, 0xfedcba98).unwrap(),
            vec![0x42b10700, 0xfedcba98],
        ),
    ];
    for (value, words) in cases {
        let value = value.with_channel(2).unwrap().with_group(3).unwrap();
        assert_eq!(ump::encode(&value).unwrap(), words);
        assert_eq!(
            ump::encode(&AnyMessage::ControlChange(value.clone())).unwrap(),
            words
        );
        assert_eq!(ump::encode(&Payload::from(value.clone())).unwrap(), words);
        assert_eq!(
            value.version(),
            if value.resolution() == 32 { 2 } else { 1 }
        );
        if value.resolution() == 14 {
            assert!(value.without_group().unwrap().encode().is_err());
        }
    }
}

#[test]
fn complete_sysex7_translation_keeps_archive_provenance() {
    let clock = Clock::new("clock".into(), 1, 1_000_000_000, "test".into(), None).unwrap();
    let timestamp = Timestamp {
        ticks: 9,
        clock: clock.clone(),
    };
    let metadata = Metadata {
        stream: Some("keys".into()),
        timestamp: Some(timestamp.clone()),
        span: Some(SourceSpan {
            first_packet: 2,
            last_packet: 3,
            first_timestamp: Some(timestamp.clone()),
            last_timestamp: Some(timestamp),
        }),
    };
    let packet = ump::SysEx7Packet::new(9, 0, b"abc".to_vec())
        .unwrap()
        .with_metadata(metadata.clone())
        .unwrap();
    let report = translation::to_midi1(&Payload::from(packet), false).unwrap();
    assert_eq!(report.status, translation::TranslationStatus::Exact);
    assert_eq!(report.group, Some(9));
    assert_eq!(
        report.messages,
        [AnyMessage::SysEx(SysEx::new(b"abc".to_vec()).unwrap())]
    );
    assert_eq!(report.messages[0].metadata(), Some(&metadata));
    let records = vec![
        archive::Record::Clock(clock),
        archive::Record::Stream {
            id: "keys".into(),
            label: String::new(),
            clock_identity: Some("clock".into()),
        },
        archive::Record::Event(Box::new(Payload::from(report.messages[0].clone()))),
    ];
    let restored = round_trip(&records, false);
    assert_eq!(restored, records);
    let archive::Record::Event(value) = &restored[2] else {
        panic!("expected an event");
    };
    assert_eq!(value.metadata(), Some(&metadata));
}

fn round_trip(records: &[archive::Record], preserve: bool) -> Vec<archive::Record> {
    let mut bytes = Vec::new();
    let mut writer = archive::Writer::new(&mut bytes, 4096).unwrap();
    for record in records {
        writer.write(record).unwrap();
    }
    writer.finish(true, "").unwrap();
    drop(writer);
    let mut reader = archive::Reader::new(Cursor::new(bytes), 4096, false, preserve).unwrap();
    let restored = reader.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(reader.complete);
    restored
}
