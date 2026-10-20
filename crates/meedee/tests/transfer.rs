use meedee::messages::SysEx;
use meedee::transfer::{Plan, Session};
use std::io::Cursor;

fn plan() -> Plan {
    Plan::read(Cursor::new([240, 1, 247, 240, 2, 3, 247]), 10, 2, 7, 2).unwrap()
}

#[test]
fn whole_messages_are_preserved_and_all_limits_reject_before_execution() {
    let p = plan();
    assert_eq!(p.message_count(), 2);
    assert_eq!(p.wire_bytes(), 7);
    assert_eq!(p.message(1).unwrap().data(), &[2, 3]);
    for (bytes, count, total, payload) in [
        (vec![240, 1], 2, 7, 2),
        (vec![240, 1, 247, 248], 2, 7, 2),
        (vec![240, 1, 247, 240, 2, 247], 1, 7, 2),
        (vec![240, 1, 2, 247], 2, 3, 2),
        (vec![240, 1, 2, 247], 2, 7, 1),
        (vec![], 2, 7, 2),
    ] {
        assert!(Plan::read(Cursor::new(bytes), 0, count, total, payload).is_err());
    }
    assert!(Plan::new([Ok(SysEx::new(vec![]).unwrap())], 0, 0, 2).is_err());
}

#[test]
fn pacing_begins_at_confirmation_and_does_not_wait_after_final_send() {
    let mut s = Session::new(plan());
    assert_eq!(s.prepare(100).unwrap().unwrap().index, 0);
    assert!(s.prepare(101).is_err());
    assert!(s.confirm(1, 105).is_err());
    s.confirm(0, 200).unwrap();
    let saved = s.snapshot();
    assert_eq!(saved.next_ready_ns, Some(210));
    assert!(s.prepare(209).unwrap().is_none());
    assert!(s.prepare(208).is_err());
    let step = s.prepare(210).unwrap().unwrap();
    assert_eq!(step.index, 1);
    s.confirm(1, 300).unwrap();
    let r = s.snapshot();
    assert_eq!(r.status, "completed");
    assert_eq!(r.confirmed_messages, 2);
    assert_eq!(r.confirmed_bytes, 7);
    assert_eq!(r.next_ready_ns, None);
    assert_eq!(r.remaining_messages, 0);
    assert_eq!(saved.confirmed_messages, 1);
    assert!(s.prepare(301).is_err());
}

#[test]
fn failure_and_cancellation_keep_known_progress_and_prevent_retries() {
    let mut s = Session::new(plan());
    s.prepare(0).unwrap();
    s.confirm(0, 1).unwrap();
    s.prepare(11).unwrap();
    s.fail(1, "uncertain native handoff").unwrap();
    s.cancel("later reason");
    let r = s.snapshot();
    assert_eq!(r.status, "failed");
    assert_eq!(r.confirmed_messages, 1);
    assert_eq!(r.confirmed_bytes, 3);
    assert_eq!(r.uncertain_message, Some(1));
    assert_eq!(r.remaining_messages, 1);
    assert_eq!(r.reason.as_deref(), Some("uncertain native handoff"));
    assert!(s.prepare(12).is_err());
    let mut s = Session::new(plan());
    s.cancel("before send");
    assert_eq!(s.snapshot().uncertain_message, None);
    let mut s = Session::new(plan());
    s.prepare(0).unwrap();
    s.cancel("during send");
    assert_eq!(s.snapshot().uncertain_message, Some(0));
}

#[test]
fn deadline_overflow_cannot_schedule_an_early_send() {
    let mut s = Session::new(plan());
    s.prepare(u64::MAX).unwrap();
    s.confirm(0, u64::MAX).unwrap();
    assert_eq!(s.snapshot().status, "failed");
    assert_eq!(s.snapshot().confirmed_messages, 1);
    assert_eq!(s.snapshot().in_flight, None);
    assert_eq!(s.snapshot().uncertain_message, None);
}
