use meedee::{
    events::Clock,
    live::Item,
    testing::{Input, ManualClock, Output},
    transport::{Error, Packet, Protocol},
};
fn clock() -> Clock {
    Clock::new("simulation".into(), 1, 1_000_000_000, "manual".into(), None).unwrap()
}
#[test]
fn dropped_new_packets_advance_sequence_and_silence_still_reports_loss() {
    let mut i = Input::new("s".into(), Protocol::Midi1, 1, 8, 1024, 64).unwrap();
    assert!(i
        .inject(Packet::Bytes(vec![0x90, 60]), clock(), Some(-10))
        .unwrap());
    assert!(!i
        .inject(Packet::Bytes(vec![100]), clock(), Some(0))
        .unwrap());
    let Item::Diagnostic(loss) = i.receive().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(loss.lost, 1);
    assert_eq!(i.snapshot().next_sequence, 2);
    assert!(i
        .inject(Packet::Bytes(vec![0x90, 61, 100]), clock(), Some(5))
        .unwrap());
    let Item::Event(event) = i.receive().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(event.span.as_ref().unwrap().first_packet, 2);
    assert!(i.receive().unwrap().is_none());
}
#[test]
fn framing_and_overflow_are_shared_and_each_retained_stage_is_bounded() {
    let mut i = Input::new("s".into(), Protocol::Midi1, 8, 1, 128, 64).unwrap();
    i.inject(
        Packet::Bytes(vec![0x90, 60, 100, 61, 101]),
        clock(),
        Some(12),
    )
    .unwrap();
    let Item::Event(event) = i.receive().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(event.timestamp.as_ref().unwrap().ticks, 12);
    let Item::Diagnostic(d) = i.receive().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(d.lost, 0);
    assert_eq!(i.snapshot().dropped_events, 1);
    assert!(i.snapshot().queued_bytes <= 128);
    assert!(i.receive().unwrap().is_none());
    i.lose(2).unwrap();
    let Item::Diagnostic(d) = i.receive().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(d.lost, 2);
}
#[test]
fn output_failure_can_occur_after_acceptance_and_close_preserves_evidence() {
    let mut o = Output::new(Protocol::Midi1, 1, 3).unwrap();
    o.fail_next_send(true, "uncertain").unwrap();
    assert!(matches!(
        o.send(Packet::Bytes(vec![0x90, 60, 100])),
        Err(Error::Native(_))
    ));
    assert_eq!(o.snapshot().attempts, 1);
    assert_eq!(o.snapshot().accepted, 1);
    assert!(o.send(Packet::Bytes(vec![0x80, 60, 0])).is_err());
    assert_eq!(o.snapshot().attempts, 1);
    o.close();
    assert_eq!(o.take_packet(), Some(Packet::Bytes(vec![0x90, 60, 100])));
    assert!(o.take_packet().is_none());
}
#[test]
fn failure_close_capacity_and_manual_time_are_explicit() {
    let mut c = ManualClock::new(u64::MAX - 1);
    c.advance(1).unwrap();
    assert!(c.advance(1).is_err());
    assert_eq!(c.now_ns(), u64::MAX);
    let mut i = Input::new("s".into(), Protocol::Midi1, 1, 1, 2, 64).unwrap();
    assert!(!i
        .inject(Packet::Bytes(vec![0x90, 60, 100]), clock(), None)
        .unwrap());
    i.fail("disconnected");
    assert!(matches!(i.receive().unwrap(), Some(Item::Diagnostic(_))));
    assert!(matches!(i.receive(), Err(Error::Worker(_))));
    i.close();
    assert!(matches!(i.receive(), Err(Error::Closed)));
    let mut o = Output::new(Protocol::Midi1, 1, 3).unwrap();
    o.send(Packet::Bytes(vec![0x90, 60, 100])).unwrap();
    assert!(matches!(
        o.send(Packet::Bytes(vec![0x80, 60, 0])),
        Err(Error::Resource(_))
    ));
    assert_eq!(o.snapshot().accepted, 1);
}

#[test]
fn an_oversized_diagnostic_cannot_let_later_events_overtake_the_loss_boundary() {
    let mut i = Input::new("s".into(), Protocol::Midi1, 4, 8, 4, 64).unwrap();
    i.inject(Packet::Bytes(vec![0xf8]), clock(), None).unwrap();
    i.lose(1).unwrap();
    assert!(matches!(i.receive().unwrap(), Some(Item::Event(_))));
    i.inject(Packet::Bytes(vec![0xf8]), clock(), None).unwrap();
    assert!(matches!(i.receive().unwrap(), Some(Item::Diagnostic(_))));
    assert_eq!(i.snapshot().dropped_events, 2);
    assert_eq!(i.snapshot().dropped_packets, 1);
    assert!(i.receive().unwrap().is_none());
}
