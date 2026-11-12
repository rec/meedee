use meedee::events::Payload;
use meedee::messages::{
    AnyMessage, Command, ControlChange, Message, NoteOn, PrimitiveMessage, SysEx, TimingClock,
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
