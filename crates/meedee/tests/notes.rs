use meedee::{events::Payload, messages, notes::Tracker};

#[test]
fn sources_are_independent_and_loss_before_notes_remains_visible() {
    let mut tracker = Tracker::default();
    let on = Payload::Midi1(messages::AnyMessage::NoteOn(
        messages::NoteOn::new(1, 60, 1).unwrap(),
    ));
    tracker.feed("left", &on, Some("c"), 3);
    tracker.boundary("right", 3);
    tracker.feed("right", &on, Some("c"), 3);
    tracker.boundary("left", 3);
    let report = tracker.snapshot();
    assert!(report.channels[0].notes.is_empty());
    assert!(report.channels[0].uncertain);
    assert_eq!(report.channels[1].notes[0].held, 1);
    assert!(report.channels[1].uncertain);
    assert_eq!(report.boundaries, 2);
}

#[test]
fn source_and_channel_storage_are_bounded_without_eviction() {
    let mut tracker = Tracker::default();
    let on = Payload::Midi1(messages::AnyMessage::NoteOn(
        messages::NoteOn::new(1, 60, 1).unwrap(),
    ));
    tracker.feed("first", &on, None, 1);
    tracker.feed("second", &on, None, 1);
    tracker.feed("first", &on, None, 1);
    let report = tracker.snapshot();
    assert_eq!(report.channels.len(), 1);
    assert_eq!(report.channels[0].notes[0].held, 2);
    assert_eq!(report.omitted_events, 1);
}
