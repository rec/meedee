use meedee::inspection_timing::Timing;

#[test]
fn live_observer_reuses_timing_without_double_counting_loss_counters() {
    use meedee::{
        events::{Clock, Event, Payload},
        inspection::Observer,
        live::{Diagnostic, DiagnosticKind, Item},
        messages::{AnyMessage, TimingClock},
    };
    let c = Clock::new("c".into(), 1, 1000, "native".into(), None).unwrap();
    let mut o = Observer::new("s".into(), c, 2).unwrap();
    let e = Item::Event(Box::new(Event {
        stream: "s".into(),
        message: Payload::Midi1(AnyMessage::TimingClock(TimingClock::new().unwrap())),
        timestamp: None,
        span: None,
    }));
    o.feed(&e).unwrap();
    o.feed(&Item::Diagnostic(Diagnostic {
        stream: "s".into(),
        packet: 1,
        kind: DiagnosticKind::Loss,
        lost: 3,
        reason: "overflow".into(),
    }))
    .unwrap();
    o.loss_counter_changed();
    let report = o.snapshot();
    assert_eq!(report.lost_events, 3);
    assert_eq!(report.records, 2);
    assert!(!report.complete);
}

#[test]
fn clocks_and_loss_segment_observations_without_cross_source_regressions() {
    let mut t = Timing::default();
    t.observe("a", Some("one"), Some(10), 0, 128);
    t.observe("b", Some("two"), Some(-100), 1, 128);
    t.observe("a", Some("one"), Some(9), 2, 128);
    t.observe("a", Some("two"), Some(-200), 3, 128);
    t.boundary("a", 4, "gap", 128);
    t.observe("a", Some("two"), Some(-300), 5, 128);
    assert_eq!(t.regressions, 1);
    assert_eq!(t.segments.len(), 4);
    assert_eq!(t.segments[0].minimum, Some(9));
    assert_eq!(t.diagnostics[1].kind, "clock_change");
}
