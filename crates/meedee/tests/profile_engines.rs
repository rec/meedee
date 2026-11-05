use meedee::{codecs, controls as velocity, gm, messages::*, mpe, timecode};

#[test]
fn extended_sysex_limits_unknown_ids_and_byte_order_are_preserved() {
    let value =
        gm::ControllerDestination::new(127, 7, 1, None, vec![(0, 66), (1, 96), (5, 32)], 1024)
            .unwrap();
    assert_eq!(value.encode().unwrap(), wire("PressureDestination"));
    assert_eq!(
        gm::decode_extended(
            &SysEx::new(value.encode().unwrap()[1..12].to_vec()).unwrap(),
            1024
        )
        .unwrap(),
        Some(gm::ExtendedMessage::ControllerDestination(value))
    );
    let value =
        gm::GlobalParameterControl::new(1, vec![257], 2, 2, vec![(vec![1, 2], vec![3, 4])], 1024)
            .unwrap();
    assert_eq!(value.encode().unwrap(), wire("GlobalParameterControl"));
    let raw = SysEx::new(value.encode().unwrap()[1..14].to_vec()).unwrap();
    assert_eq!(
        gm::decode_extended(&raw, 1024).unwrap(),
        Some(gm::ExtendedMessage::GlobalParameterControl(value))
    );
    assert!(gm::GlobalParameterControl::new(1, vec![0], 0, 1, vec![], 1024).is_err());
    assert!(gm::KeyBasedControl::new(1, 1, 60, vec![(0, 0)], 1024).is_err());
    assert!(gm::KeyBasedControl::new(1, 1, 60, vec![(7, 0); 1025], 1024).is_err());
    assert!(gm::ControllerDestination::new(1, 1, 3, Some(32), vec![], 1024).is_err());
    assert!(
        gm::decode_extended(&SysEx::new(vec![127, 1, 4, 5, 127, 1, 1]).unwrap(), 1024).is_err()
    );
}
#[test]
fn mpe_exhaustion_sustain_and_ambiguous_notes_do_not_invent_voice_attribution() {
    let zone = mpe::ZoneConfiguration::new(1, 2).unwrap();
    let mut a = mpe::Allocator::new(zone.clone());
    let v = a.allocate(60, 100).unwrap();
    assert_eq!(v.channel(), 2);
    assert_eq!(codecs::encode(&v.note_on()).unwrap(), [145, 60, 100]);
    a.allocate(61, 100).unwrap();
    assert!(a.allocate(62, 100).is_err());
    a.set_sustain(true);
    a.release(v.id(), 64).unwrap();
    assert!(a.allocate(62, 100).is_err());
    a.set_sustain(false);
    assert_eq!(a.allocate(62, 100).unwrap().channel(), 2);
    assert!(a.release(v.id(), 64).is_err());
    let mut t = mpe::Tracker::new(zone, None).unwrap();
    assert_eq!(t.snapshot(2).unwrap().pitch_bend, None);
    assert_eq!(t.snapshot(2).unwrap().sustain, None);
    for note in [60, 60, 64] {
        t.feed(&meedee::events::Payload::Midi1(
            (AnyMessage::NoteOn(
                NoteOn::new(note)
                    .and_then(|m| m.with_velocity(100))
                    .and_then(|m| m.with_channel(2))
                    .unwrap(),
            ))
            .clone(),
        ))
        .unwrap();
    }
    assert!(t.snapshot(2).unwrap().ambiguous);
    t.feed(&meedee::events::Payload::Midi1(
        (AnyMessage::ControlChange(
            ControlChange::new(64, 127)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ))
        .clone(),
    ))
    .unwrap();
    t.feed(&meedee::events::Payload::Midi1(
        (AnyMessage::ControlChange(
            ControlChange::new(123, 0)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ))
        .clone(),
    ))
    .unwrap();
    assert_eq!(t.snapshot(2).unwrap().sustain, Some(true));
    assert_eq!(t.snapshot(2).unwrap().notes[0].sustained, 2);
    t.feed(&meedee::events::Payload::Midi1(
        (AnyMessage::ControlChange(
            ControlChange::new(64, 0)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ))
        .clone(),
    ))
    .unwrap();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
    t.feed(&meedee::events::Payload::Midi1(
        (AnyMessage::NoteOn(
            NoteOn::new(60)
                .and_then(|m| m.with_velocity(100))
                .and_then(|m| m.with_channel(2))
                .unwrap(),
        ))
        .clone(),
    ))
    .unwrap();
    t.feed(&meedee::events::Payload::Midi1(
        (AnyMessage::ControlChange(
            ControlChange::new(120, 0)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ))
        .clone(),
    ))
    .unwrap();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
    t.reset();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
}
#[test]
fn velocity_prefix_is_channel_scoped_one_shot_and_cleared_on_reset() {
    use meedee::events::{ObservationValue, Payload};
    let mut d = velocity::VelocityInterpreter::new(None).unwrap();
    let prefix = Payload::Midi1(AnyMessage::ControlChange(
        ControlChange::new(88, 127)
            .unwrap()
            .with_channel(1)
            .unwrap(),
    ));
    let note = NoteOn::new(60).unwrap().with_velocity(1).unwrap();
    d.feed(&prefix).unwrap();
    let coarse = d
        .feed(&Payload::Midi1(AnyMessage::NoteOn(
            note.clone().with_channel(2).unwrap(),
        )))
        .unwrap();
    assert!(
        matches!(&coarse[0].value, ObservationValue::Velocity(v) if v.velocity==1 && v.resolution==7)
    );
    let fine = d
        .feed(&Payload::Midi1(AnyMessage::NoteOn(
            note.clone().with_channel(1).unwrap(),
        )))
        .unwrap();
    assert!(
        matches!(&fine[0].value, ObservationValue::Velocity(v) if v.velocity==255 && v.resolution==14)
    );
    d.feed(&prefix).unwrap();
    let off = d
        .feed(&Payload::Midi1(AnyMessage::NoteOn(
            note.with_channel(1).unwrap().with_velocity(0).unwrap(),
        )))
        .unwrap();
    assert!(matches!(&off[0].value, ObservationValue::Velocity(v) if !v.note_on && v.velocity==0));
    d.feed(&prefix).unwrap();
    d.reset();
    let coarse = d
        .feed(&Payload::Midi1(AnyMessage::NoteOff(
            NoteOff::new(60)
                .unwrap()
                .with_velocity(64)
                .unwrap()
                .with_channel(1)
                .unwrap(),
        )))
        .unwrap();
    assert!(
        matches!(&coarse[0].value, ObservationValue::Velocity(v) if v.velocity==64 && v.resolution==7)
    );
}

#[test]
fn timecode_sequence_lock_requires_continuity_and_expires_without_extrapolation() {
    use timecode::*;
    let mut a = Assembler::new(100, None).unwrap();
    let mut now = 0;
    for frame in [0, 2] {
        let n = [frame, 0, 0, 0, 0, 0, 0, 6];
        for (i, v) in n.into_iter().enumerate() {
            let r = a
                .feed(
                    &meedee::events::Payload::Midi1(AnyMessage::QuarterFrame(
                        QuarterFrame::new(i as i64, v).unwrap(),
                    )),
                    now,
                )
                .unwrap();
            now += 1;
            if i == 7 {
                let meedee::events::ObservationValue::Timecode(v) = &r[0].value else {
                    panic!("timecode expected")
                };
                let o = v.observation.as_ref().unwrap();
                assert_eq!(o.encoded.frames(), frame as u8);
                assert_eq!(o.compensated.frames(), frame as u8 + 2);
                assert_eq!(o.locked, frame == 2);
            }
        }
    }
    assert!(a.locked());
    assert!(
        matches!(&a.poll(115).unwrap()[0].value, meedee::events::ObservationValue::Timecode(v) if v.diagnostic == Some(AssemblyDiagnostic::Timeout))
    );
    assert!(!a.locked());
    assert!(a.poll(114).is_err());
    let p = Position::new(FrameRate::DropFrame30, 0, 0, 59, 29).unwrap();
    let q = p.advance(1);
    assert_eq!((q.minutes(), q.seconds(), q.frames()), (1, 0, 2));
    assert_eq!(q.advance(-1), p);
    assert!(Position::new(FrameRate::DropFrame30, 0, 1, 0, 1).is_err());
    let start = Position::new(FrameRate::Fps24, 0, 0, 0, 0).unwrap();
    assert_eq!(start.advance(-1).advance(1), start);
}

fn wire(name: &str) -> Vec<u8> {
    include_str!("fixtures/profile-engines.txt")
        .lines()
        .filter_map(|l| l.split_once('|'))
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
        .split_whitespace()
        .map(|v| u8::from_str_radix(v, 16).unwrap())
        .collect()
}
