use meedee::{archive, events, messages, routing, transport::Protocol, ump};

fn event(source: &str, note: i64) -> events::Event {
    events::Event {
        stream: source.into(),
        timestamp: None,
        span: None,
        message: events::Payload::Midi1(messages::AnyMessage::NoteOn(
            messages::NoteOn::new(1, note, 100).unwrap(),
        )),
    }
}
fn route(name: &str, source: &str) -> routing::Route {
    routing::Route::new(
        name.into(),
        source.into(),
        name.into(),
        Protocol::Midi1,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap()
}

#[test]
fn merging_is_arrival_order_then_route_order_with_explicit_confirmation() {
    let mut router = routing::Router::new(
        vec![route("a", "s"), route("b", "s"), route("c", "t")],
        4,
        12,
    )
    .unwrap();
    router.feed(&event("t", 61)).unwrap();
    router.feed(&event("s", 60)).unwrap();
    for name in ["c", "a", "b"] {
        let delivery = router.peek().unwrap();
        assert_eq!(delivery.destination, name);
        let id = delivery.id;
        router.confirm(id).unwrap();
    }
    let report = router.snapshot();
    assert_eq!(report.confirmed, 3);
    assert_eq!(report.queued_bytes, 0);
    assert_eq!(report.routes["a"].confirmed, 1);
}

#[test]
fn capacity_failure_never_admits_a_partial_fanout_or_discards_progress() {
    let mut router = routing::Router::new(vec![route("a", "s"), route("b", "s")], 3, 9).unwrap();
    router.feed(&event("s", 60)).unwrap();
    assert!(router.feed(&event("s", 61)).is_err());
    let report = router.snapshot();
    assert!(report.stopped);
    assert_eq!(report.admitted, 2);
    assert_eq!(report.pending, 2);
    assert_eq!(report.capacity_rejections, 2);
    assert_eq!(report.routes["a"].capacity_rejections, 1);
    assert_eq!(report.queued_bytes, 6);
    assert!(router.peek().is_none());
    assert!(router.confirm(0).is_err());
    let mut bytes = routing::Router::new(vec![route("a", "s"), route("b", "s")], 128, 5).unwrap();
    assert!(bytes.feed(&event("s", 60)).is_err());
    assert_eq!(bytes.snapshot().admitted, 0);
}

#[test]
fn ump_remapping_preserves_native_value_and_source_provenance() {
    let route = routing::Route::new(
        "r".into(),
        "s".into(),
        "out".into(),
        Protocol::Ump,
        Some(16),
        Some(16),
        Some(1),
        Some(1),
        false,
    )
    .unwrap();
    let mut router = routing::Router::new(vec![route], 128, 1024).unwrap();
    let clock = events::Clock::new("c".into(), 1, 30000, "test".into(), None).unwrap();
    let mut e = event("s", 60);
    e.timestamp = Some(events::Timestamp { ticks: -5, clock });
    e.message = events::Payload::Ump(ump::UmpMessage::Midi2ControlChange(
        ump::Midi2ControlChange::new(16, 16, 74, 0xfedcba98).unwrap(),
    ));
    router.feed(&e).unwrap();
    let d = router.peek().unwrap();
    assert_eq!(d.event.stream, "s");
    assert_eq!(d.event.timestamp, e.timestamp);
    let events::Payload::Ump(v) = &d.event.message else {
        panic!("expected UMP")
    };
    assert_eq!(ump::encode(v), [0x40b04a00, 0xfedcba98]);
    let events::Payload::Ump(v) = &e.message else {
        panic!("expected UMP")
    };
    assert_eq!(ump::encode(v), [0x4fbf4a00, 0xfedcba98]);
}

#[test]
fn preview_verifies_completion_after_routing_stops_at_loss() {
    let mut data = Vec::new();
    let mut writer = archive::Writer::new(&mut data, 1024).unwrap();
    writer
        .write(&archive::Record::Stream {
            id: "s".into(),
            label: String::new(),
            clock_identity: None,
        })
        .unwrap();
    writer
        .write(&archive::Record::Event(Box::new(event("s", 60))))
        .unwrap();
    writer
        .write(&archive::Record::Gap {
            stream: "s".into(),
            lost: 1,
            reason: "loss".into(),
        })
        .unwrap();
    writer
        .write(&archive::Record::Event(Box::new(event("s", 61))))
        .unwrap();
    writer.finish(true, "").unwrap();
    let preview =
        routing::preview(data.as_slice(), vec![route("out", "s")], 128, 1024, 1024, 1).unwrap();
    assert!(preview.complete);
    assert!(preview.routing.stopped);
    assert_eq!(preview.records, 4);
    assert_eq!(preview.routing.confirmed, 1);
    assert_eq!(preview.examples.len(), 1);
    let prefix = routing::preview(
        &data[..data.len() - 1],
        vec![route("out", "s")],
        128,
        1024,
        1024,
        1,
    )
    .unwrap();
    assert!(!prefix.complete);
    assert!(prefix.problem.is_some());
}
