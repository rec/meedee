use middi::events::Rational;
use middi::messages::{Beats, Note, NoteDuration, Rest};
use std::time::Duration;

#[test]
fn beats_keep_exact_fractions_and_time_is_independent_of_tempo() {
    let tempo = Rational::new(120, 1).unwrap();
    let third = NoteDuration::from(Beats::new(1, 3).unwrap());
    let interval = third.seconds(Some(tempo)).unwrap();
    let total = interval
        .checked_add(interval)
        .unwrap()
        .checked_add(interval)
        .unwrap();
    assert_eq!(total, Rational::new(1, 2).unwrap());
    let elapsed = NoteDuration::from(Duration::from_millis(500));
    assert_eq!(elapsed.seconds(None).unwrap(), total);
    assert_eq!(
        elapsed
            .seconds(Some(Rational::new(73, 1).unwrap()))
            .unwrap(),
        total
    );
    assert!(third.seconds(None).is_err());
    assert!(third.seconds(Some(Rational::new(0, 1).unwrap())).is_err());
    assert!(Beats::new(1, 0).is_err());
}

#[test]
fn note_defaults_and_builders_preserve_duration_and_unassigned_channel() {
    let note = Note::new(Duration::from_millis(500), 60).unwrap();
    assert_eq!(note.channel(), None);
    assert_eq!(note.velocity(), 90);
    let changed = note
        .clone()
        .with_velocity(70)
        .unwrap()
        .with_channel(3)
        .unwrap();
    assert_eq!(changed.note(), 60);
    assert_eq!(changed.velocity(), 70);
    assert_eq!(changed.channel(), Some(3));
    assert_eq!(changed.duration(), note.duration());
    assert!(note.clone().with_velocity(128).is_err());
    assert!(note.with_channel(17).is_err());
    assert!(Note::new(Beats::from(1), 128).is_err());
    let rest = Rest::new(Beats::new(1, 2).unwrap());
    assert_eq!(
        rest.duration(),
        NoteDuration::Beats(Beats::new(1, 2).unwrap())
    );
}
