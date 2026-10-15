use meedee::{codecs, gm, messages::*, mpe, timecode, velocity};

#[test]
fn extended_sysex_limits_unknown_ids_and_byte_order_are_preserved() {
    let value =
        gm::ControllerDestination::new(127, 7, 1, None, vec![(0, 66), (1, 96), (5, 32)], 1024)
            .unwrap();
    assert_eq!(value.encode(), wire("PressureDestination"));
    assert_eq!(
        gm::decode_extended(&SysEx::new(value.encode()[1..12].to_vec()).unwrap(), 1024).unwrap(),
        Some(gm::ExtendedMessage::ControllerDestination(value))
    );
    let value =
        gm::GlobalParameterControl::new(1, vec![257], 2, 2, vec![(vec![1, 2], vec![3, 4])], 1024)
            .unwrap();
    assert_eq!(value.encode(), wire("GlobalParameterControl"));
    let raw = SysEx::new(value.encode()[1..14].to_vec()).unwrap();
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
    assert_eq!(codecs::encode(&v.note_on()), [145, 60, 100]);
    a.allocate(61, 100).unwrap();
    assert!(a.allocate(62, 100).is_err());
    a.set_sustain(true);
    a.release(v.id(), 64).unwrap();
    assert!(a.allocate(62, 100).is_err());
    a.set_sustain(false);
    assert_eq!(a.allocate(62, 100).unwrap().channel(), 2);
    assert!(a.release(v.id(), 64).is_err());
    let mut t = mpe::Tracker::new(zone);
    assert_eq!(t.snapshot(2).unwrap().pitch_bend, None);
    assert_eq!(t.snapshot(2).unwrap().sustain, None);
    for note in [60, 60, 64] {
        t.feed(&AnyMessage::NoteOn(NoteOn::new(2, note, 100).unwrap()))
            .unwrap();
    }
    assert!(t.snapshot(2).unwrap().ambiguous);
    t.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 64, 127).unwrap(),
    ))
    .unwrap();
    t.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 123, 0).unwrap(),
    ))
    .unwrap();
    assert_eq!(t.snapshot(2).unwrap().sustain, Some(true));
    assert_eq!(t.snapshot(2).unwrap().notes[0].sustained, 2);
    t.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 64, 0).unwrap(),
    ))
    .unwrap();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
    t.feed(&AnyMessage::NoteOn(NoteOn::new(2, 60, 100).unwrap()))
        .unwrap();
    t.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 120, 0).unwrap(),
    ))
    .unwrap();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
    t.reset();
    assert!(t.snapshot(2).unwrap().notes.is_empty());
}
#[test]
fn velocity_prefix_is_channel_scoped_one_shot_and_cleared_on_reset() {
    let mut d = velocity::VelocityInterpreter::new();
    d.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 88, 127).unwrap(),
    ));
    let coarse = d
        .feed(&AnyMessage::NoteOn(NoteOn::new(2, 60, 1).unwrap()))
        .unwrap();
    assert_eq!((coarse.velocity, coarse.resolution), (1, 7));
    let fine = d
        .feed(&AnyMessage::NoteOn(NoteOn::new(1, 60, 1).unwrap()))
        .unwrap();
    assert_eq!((fine.velocity, fine.resolution), (255, 14));
    d.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 88, 127).unwrap(),
    ));
    let off = d
        .feed(&AnyMessage::NoteOn(NoteOn::new(1, 60, 0).unwrap()))
        .unwrap();
    assert!(!off.note_on);
    assert_eq!(off.velocity, 0);
    d.feed(&AnyMessage::ControlChange(
        ControlChange::new(1, 88, 127).unwrap(),
    ));
    d.reset();
    assert_eq!(
        d.feed(&AnyMessage::NoteOff(NoteOff::new(1, 60, 64).unwrap()))
            .unwrap()
            .resolution,
        7
    );
}
#[test]
fn timecode_sequence_lock_requires_continuity_and_expires_without_extrapolation() {
    use timecode::*;
    let mut a = Assembler::new(100).unwrap();
    let mut now = 0;
    for frame in [0, 2] {
        let n = [frame, 0, 0, 0, 0, 0, 0, 6];
        for (i, v) in n.into_iter().enumerate() {
            let r = a
                .feed(&QuarterFrame::new(i as i64, v).unwrap(), now)
                .unwrap();
            now += 1;
            if i == 7 {
                let o = r.observation.unwrap();
                assert_eq!(o.encoded.frames(), frame as u8);
                assert_eq!(o.compensated.frames(), frame as u8 + 2);
                assert_eq!(o.locked, frame == 2);
            }
        }
    }
    assert!(a.locked());
    assert_eq!(a.poll(115).unwrap(), Some(AssemblyDiagnostic::Timeout));
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
