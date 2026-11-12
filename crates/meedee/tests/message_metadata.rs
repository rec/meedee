use meedee::events::{Clock, SourceSpan, Timestamp};
use meedee::messages::{
    AnyCommand, AnyMessage, Command, Message, Metadata, NoteOn, PrimitiveMessage,
};
use meedee::{gm, messages, mpe, tuning, ump, universal};
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
    check_value(tuning::TuningMessage::NoteChange(
        tuning::NoteChange::new(
            1,
            2,
            vec![tuning::NoteTuning::new(60, tuning::TuningValue::NoChange).unwrap()],
        )
        .unwrap(),
    ));
    check_value(meedee::messages::AnyMessage::NoteOn(
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
        AnyCommand::ControlChange(
            meedee::messages::ControlChange::new_with_resolution(1, 1234, 14)
                .unwrap()
                .with_channel(2)
                .unwrap(),
        ),
        AnyCommand::ParameterSet(
            meedee::messages::ParameterSet::new(0, 1234)
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
        AnyCommand::ProgramChange(
            meedee::messages::ProgramChange::new(34)
                .and_then(|m| m.with_bank(0, 0))
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

#[test]
fn generic_transpose_preserves_default_values_and_dispatches_note_addresses() {
    fn shift<M: Message>(value: M, semitones: i64) -> M {
        value.transpose(semitones)
    }
    let control = meedee::messages::ControlChange::new(7, 90)
        .unwrap()
        .with_metadata(metadata())
        .unwrap();
    let unchanged = shift(control.clone(), 12);
    assert_eq!(unchanged, control);
    assert_eq!(unchanged.metadata(), control.metadata());
    let pressure = meedee::messages::PolyPressure::new_midi2(60)
        .unwrap()
        .with_pressure(0xFEDCBA98)
        .unwrap()
        .with_channel(2)
        .unwrap()
        .with_group(3)
        .unwrap()
        .with_metadata(metadata())
        .unwrap();
    let shifted = shift(pressure.clone(), 12);
    assert_eq!(shifted.note(), 72);
    assert_eq!(shifted.pressure(), pressure.pressure());
    assert_eq!(shifted.channel(), pressure.channel());
    assert_eq!(shifted.group(), pressure.group());
    assert_eq!(shifted.metadata(), pressure.metadata());
    assert_eq!(shift(pressure.clone(), i64::MAX).note(), 127);
    assert_eq!(shift(pressure.clone(), i64::MIN).note(), 0);
    let payload =
        meedee::events::Payload::Message(meedee::messages::AnyMessage::PolyPressure(pressure));
    let shifted = shift(payload, 12);
    let meedee::events::Payload::Message(meedee::messages::AnyMessage::PolyPressure(shifted)) =
        shifted
    else {
        panic!("changed message kind")
    };
    assert_eq!(shifted.note(), 72);
    assert_eq!(shifted.metadata(), Some(&metadata()));
}

#[test]
fn transpose_keeps_per_note_controls_and_ump_wrappers_addressed_to_the_shifted_key() {
    let controls = [
        meedee::messages::AnyMessage::RegisteredPerNoteController(
            messages::RegisteredPerNoteController::new(3, 2, 60, 1, 1234).unwrap(),
        ),
        meedee::messages::AnyMessage::AssignablePerNoteController(
            messages::AssignablePerNoteController::new(3, 2, 60, 2, 5678).unwrap(),
        ),
        meedee::messages::AnyMessage::PerNotePitchBend(
            messages::PerNotePitchBend::new(3, 2, 60, -1234).unwrap(),
        ),
        meedee::messages::AnyMessage::PerNoteManagement(
            messages::PerNoteManagement::new(3, 2, 60, 3).unwrap(),
        ),
    ];
    for value in controls {
        let before = ump::encode(&value).unwrap();
        let shifted = value.with_metadata(metadata()).unwrap().transpose(12);
        let after = ump::encode(&shifted).unwrap();
        assert_eq!(after[0], (before[0] & !0x7F00) | (72 << 8));
        assert_eq!(after[1], before[1]);
        assert_eq!(shifted.metadata(), Some(&metadata()));
    }
    let packet = AnyMessage::PolyPressure(
        meedee::messages::PolyPressure::new(60)
            .unwrap()
            .with_pressure(50)
            .unwrap()
            .with_channel(2)
            .unwrap(),
    )
    .with_group(3)
    .unwrap()
    .with_metadata(metadata())
    .unwrap();
    let shifted = packet.transpose(12);
    assert_eq!(shifted.group(), Some(3));
    assert_eq!(
        shifted.clone().without_group().unwrap().encode().unwrap(),
        [0xA1, 72, 50]
    );
    assert_eq!(shifted.metadata(), Some(&metadata()));
}
