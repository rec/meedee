use meedee::events::{Clock, SourceSpan, Timestamp};
use meedee::messages::{
    AnyCommand, AnyMessage, Command, Message, Metadata, NoteOn, PrimitiveMessage,
};
use meedee::{gm, mpe, tuning, ump, universal};
use std::collections::HashSet;
use std::hash::Hash;

fn timestamp() -> Timestamp {
    Timestamp {
        ticks: 123,
        clock: Clock::new("clock".into(), 1, 1000, "test".into(), None).unwrap(),
    }
}
fn metadata() -> Metadata {
    Metadata {
        timestamp: Some(timestamp()),
        stream: Some("keyboard".into()),
        span: Some(SourceSpan {
            first_packet: 1,
            last_packet: 3,
            first_timestamp: Some(timestamp()),
            last_timestamp: Some(timestamp()),
        }),
    }
}
fn check_value<M: Message + Clone + Eq + Hash>(value: M) {
    let attached = value.clone().with_metadata(metadata()).unwrap();
    assert_eq!(attached.metadata(), Some(&metadata()));
    assert_eq!(attached, value);
    assert_eq!(HashSet::from([attached.clone(), value.clone()]).len(), 1);
    assert_eq!(attached.without_metadata().metadata(), None);
    assert_eq!(value.metadata(), None);
}

#[test]
fn provenance_survives_note_builders_and_clamped_transposition() {
    let note = NoteOn::new(60)
        .unwrap()
        .with_metadata(metadata())
        .unwrap()
        .with_channel(2)
        .unwrap()
        .with_velocity(70)
        .unwrap()
        .transpose(100);
    assert_eq!(note.metadata(), Some(&metadata()));
    assert_eq!(note.encode().unwrap(), [0x91, 127, 70]);
    assert_eq!(note.clone().without_channel().metadata(), note.metadata());
    let changed = note.clone().with_stream("second").unwrap();
    assert_eq!(changed.timestamp(), note.timestamp());
    assert_eq!(changed.span(), note.span());
    assert_eq!(changed.stream(), Some("second"));
    assert_eq!(note.stream(), Some("keyboard"));
    check_value(note.without_metadata());
}

#[test]
fn metadata_is_ignored_by_equality_and_hash_across_message_families() {
    check_value(AnyMessage::NoteOn(NoteOn::new(60).unwrap()));
    check_value(meedee::messages::TimingClock::new().unwrap());
    check_value(meedee::messages::SysEx::new(vec![125, 1, 2]).unwrap());
    check_value(gm::Gm2On::new(1).unwrap());
    check_value(universal::MasterVolume::new(1, 123).unwrap());
    check_value(tuning::BulkDumpRequest::new(1, 2).unwrap());
    check_value(tuning::TuningMessage::SingleNoteChange(
        tuning::SingleNoteChange::new(
            1,
            2,
            vec![tuning::NoteTuning::new(60, tuning::TuningValue::NoChange).unwrap()],
        )
        .unwrap(),
    ));
    check_value(ump::UmpMessage::NoteOn(
        meedee::messages::NoteOn::new_midi2(60)
            .and_then(|m| m.with_group(1))
            .and_then(|m| m.with_channel(2))
            .and_then(|m| m.with_velocity(1234))
            .and_then(|m| m.with_attribute_type(0))
            .and_then(|m| m.with_attribute_data(0))
            .unwrap(),
    ));
    check_value(ump::SysEx7Packet::new(1, 0, vec![125, 1]).unwrap());
    assert_ne!(NoteOn::new(60).unwrap(), NoteOn::new(61).unwrap());
    assert_ne!(metadata(), Metadata::default());
}

#[test]
fn expanded_commands_retain_the_original_provenance() {
    let commands = [
        AnyCommand::ControlChange14(
            meedee::messages::ControlChange14::new(1, 1234)
                .unwrap()
                .with_channel(2)
                .unwrap(),
        ),
        AnyCommand::RpnSet(
            meedee::messages::RpnSet::new(0, 1234)
                .unwrap()
                .with_channel(2)
                .unwrap(),
        ),
        AnyCommand::ParameterStep(
            meedee::messages::ParameterStep::new(1)
                .and_then(|m| m.with_family(meedee::controls::ParameterFamily::Nrpn))
                .unwrap()
                .with_channel(2)
                .unwrap(),
        ),
        AnyCommand::BankProgramSelect(
            gm::BankProgramSelect::new(34)
                .unwrap()
                .with_channel(2)
                .unwrap(),
        ),
        AnyCommand::ZoneConfiguration(mpe::ZoneConfiguration::new(1, 3).unwrap()),
    ];
    for command in commands {
        check_value(command.clone());
        let messages = command.with_metadata(metadata()).unwrap().expand().unwrap();
        assert!(!messages.is_empty());
        assert!(messages.iter().all(|m| m.metadata() == Some(&metadata())));
    }
}

#[test]
fn metadata_rejects_inconsistent_clocks_spans_and_empty_sources() {
    let note = NoteOn::new(60).unwrap();
    assert!(note.clone().with_stream("").is_err());
    let mut reversed = metadata();
    reversed.span.as_mut().unwrap().last_packet = 0;
    assert!(note.clone().with_metadata(reversed).is_err());
    let mut different = timestamp();
    different.clock = Clock::new("other".into(), 1, 1000, "test".into(), None).unwrap();
    assert!(note
        .clone()
        .with_metadata(metadata())
        .unwrap()
        .with_timestamp(different)
        .is_err());
    let mut disabled = timestamp();
    disabled.clock = disabled.clock.with_transport(1, false);
    assert!(note.with_timestamp(disabled).is_err());
}
