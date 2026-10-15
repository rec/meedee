use meedee::{
    event_views::{self, Value, View},
    events::{Clock, Event, Payload, Timestamp},
    live::{Diagnostic, DiagnosticKind},
    messages::{AnyMessage, ControlChange, NoteOn},
    ump,
};
fn event(message: AnyMessage, ticks: i64) -> Event {
    Event {
        stream: "s".into(),
        timestamp: Some(Timestamp {
            ticks,
            clock: Clock::new("c".into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
        }),
        span: None,
        message: Payload::Midi1(message),
    }
}
#[test]
fn timed_controls_retain_only_active_contributors_and_reset_on_loss() {
    let mut view = event_views::Controls::new(None).unwrap();
    view.feed(
        &event(
            AnyMessage::ControlChange(ControlChange::new(1, 7, 64).unwrap()),
            10,
        ),
        None,
    )
    .unwrap();
    for i in 0..2000 {
        view.feed(
            &event(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap()), i + 11),
            None,
        )
        .unwrap();
    }
    let out = view
        .feed(
            &event(
                AnyMessage::ControlChange(ControlChange::new(1, 39, 1).unwrap()),
                3000,
            ),
            None,
        )
        .unwrap();
    assert_eq!(out[0].first_index, Some(0));
    assert_eq!(out[0].first_timestamp.as_ref().unwrap().ticks, 10);
    assert_eq!(out[0].last_timestamp.as_ref().unwrap().ticks, 3000);
    assert!(
        matches!(&out[0].value,Value::Control(meedee::controls::Observation::Control(v))if v.value==8193)
    );
    view.boundary(&Diagnostic {
        stream: "s".into(),
        packet: 2002,
        kind: DiagnosticKind::Loss,
        lost: 1,
        reason: "loss".into(),
    })
    .unwrap();
    let out = view
        .feed(
            &event(
                AnyMessage::ControlChange(ControlChange::new(1, 39, 2).unwrap()),
                4000,
            ),
            None,
        )
        .unwrap();
    assert!(matches!(
        &out[0].value,
        Value::Control(meedee::controls::Observation::Diagnostic(_))
    ));
    let mut wrong = event(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap()), 5000);
    wrong.stream = "another".into();
    assert!(view.feed(&wrong, None).is_err());
}
#[test]
fn velocity_and_ump_assembly_keep_exact_event_ranges() {
    let mut v = event_views::Velocity::new(None).unwrap();
    v.feed(
        &event(
            AnyMessage::ControlChange(ControlChange::new(1, 88, 127).unwrap()),
            1,
        ),
        None,
    )
    .unwrap();
    let out = v
        .feed(
            &event(AnyMessage::NoteOn(NoteOn::new(1, 60, 127).unwrap()), 2),
            None,
        )
        .unwrap();
    assert_eq!((out[0].first_index, out[0].last_index), (Some(0), Some(1)));
    assert!(matches!(&out[0].value,Value::Velocity(v)if v.velocity==16383));
    let mut assembler = event_views::Ump::new(1024, 8192, 64).unwrap();
    let packets = ump::packetize_sysex7(2, &[125, 1, 2, 3, 4, 5, 6, 7]).unwrap();
    let mut a = event(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap()), 10);
    a.message = Payload::Ump(ump::UmpMessage::SysEx7(packets[0].clone()));
    assert!(assembler.feed(&a, None).unwrap().is_empty());
    a.timestamp.as_mut().unwrap().ticks = 20;
    a.message = Payload::Ump(ump::UmpMessage::SysEx7(packets[1].clone()));
    let out = assembler.feed(&a, None).unwrap();
    assert_eq!(
        (out[0].group, out[0].first_index, out[0].last_index),
        (Some(2), Some(0), Some(1))
    );
    assert_eq!(out[0].first_timestamp.as_ref().unwrap().ticks, 10);
    assert_eq!(out[0].last_timestamp.as_ref().unwrap().ticks, 20);
}
