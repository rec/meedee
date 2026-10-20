use meedee::{
    events::{Event, Payload},
    live::{Diagnostic, DiagnosticKind, Item},
    messages::{AnyMessage, SysEx},
    transfer::Capture,
};

fn event(data: &[u8]) -> Item {
    Item::Event(Box::new(Event {
        stream: "s".into(),
        timestamp: None,
        span: None,
        message: Payload::Midi1(AnyMessage::SysEx(SysEx::new(data.to_vec()).unwrap())),
    }))
}
fn capture(count: usize, bytes: usize, payload: usize) -> Capture {
    Capture::new("s".into(), count, 10, bytes, payload).unwrap()
}
#[test]
fn count_completion_preserves_exact_bytes_and_independent_progress() {
    let mut c = capture(2, 7, 2);
    assert!(c.write(&mut Vec::new()).is_err());
    assert!(c.start(u64::MAX).is_err());
    assert_eq!(c.snapshot().status, "ready");
    c.start(100).unwrap();
    assert!(c.start(100).is_err());
    c.feed(&event(&[1]), 101).unwrap();
    let saved = c.snapshot();
    assert!(c.poll(100).is_err());
    c.feed(&event(&[2, 3]), 109).unwrap();
    assert_eq!(saved.received_messages, 1);
    assert_eq!(c.snapshot().status, "completed");
    assert_eq!(c.snapshot().wire_bytes, 7);
    let mut bytes = Vec::new();
    c.write(&mut bytes).unwrap();
    assert_eq!(bytes, [240, 1, 247, 240, 2, 3, 247]);
    c.cancel("late cancellation");
    assert_eq!(c.snapshot().status, "completed");
    assert!(c.feed(&event(&[]), 109).is_err());
}
#[test]
fn deadline_limits_and_diagnostics_stop_without_exporting_a_prefix() {
    let mut c = capture(2, 8, 2);
    c.start(0).unwrap();
    c.feed(&event(&[1]), 1).unwrap();
    c.feed(&event(&[2]), 10).unwrap();
    assert_eq!(c.snapshot().status, "timed_out");
    assert_eq!(c.snapshot().received_messages, 1);
    assert!(c.write(&mut Vec::new()).is_err());
    assert_eq!(c.message(0).unwrap().data(), &[1]);
    for (bytes, payload) in [(4, 2), (8, 1)] {
        let mut c = capture(2, bytes, payload);
        c.start(0).unwrap();
        c.feed(&event(&[1]), 1).unwrap();
        c.feed(&event(&[2, 3]), 2).unwrap();
        assert_eq!(c.snapshot().status, "failed");
        assert_eq!(c.snapshot().received_messages, 1);
        assert_eq!(c.snapshot().wire_bytes, 3);
    }
    for kind in [
        DiagnosticKind::Loss,
        DiagnosticKind::Malformed,
        DiagnosticKind::ClockChange,
        DiagnosticKind::Truncated,
    ] {
        let mut c = capture(1, 8, 2);
        c.start(0).unwrap();
        c.feed(
            &Item::Diagnostic(Diagnostic {
                stream: "s".into(),
                packet: 0,
                kind,
                lost: 0,
                reason: "x".repeat(2000),
            }),
            1,
        )
        .unwrap();
        let report = c.snapshot();
        assert_eq!(report.status, "failed");
        assert_eq!(report.reason.unwrap().len(), 1024);
    }
    assert!(Capture::new("s".into(), 3, 1, 4, 1).is_err());
    let mut c = capture(1, 2, 1);
    c.cancel("cancelled before starting");
    assert_eq!(c.snapshot().status, "cancelled");
    assert!(c.start(0).is_err());
}
