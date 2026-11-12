use middy::{Clock, Message, Note, NoteOn, Output, PrimitiveMessage, Timestamp};
use std::time::Duration;

#[test]
fn root_classes_support_message_transforms_and_device_free_output() {
    let clock = Clock::new("capture".into(), 1, 1000, "test".into(), None).unwrap();
    let timestamp = Timestamp { ticks: 12, clock };
    let note = NoteOn::new(60)
        .unwrap()
        .with_channel(2)
        .unwrap()
        .with_timestamp(timestamp.clone())
        .unwrap()
        .transpose(12);
    assert_eq!(note.encode().unwrap(), [0x91, 72, 90]);
    assert_eq!(note.timestamp(), Some(&timestamp));
    let full_note = Note::new(Duration::from_millis(500), 60).unwrap();
    assert_eq!(full_note.note(), 60);
    let mut output = Output::new();
    assert_eq!(output.send(Vec::<NoteOn>::new()).unwrap().sent_events, 0);
    assert!(!output.closed());
    output.close().unwrap();
    assert!(output.closed());
}
