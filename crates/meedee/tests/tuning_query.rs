use meedee::{
    events::{Event, Payload},
    live::{Diagnostic, DiagnosticKind, Item},
    messages::SysEx,
    tuning::{BankedDumpRequest, BulkDumpRequest, ChecksumStatus},
    tuning_query::Query,
};

fn dump(kind: &str) -> SysEx {
    let text = include_str!("fixtures/tuning.txt")
        .lines()
        .find_map(|v| v.strip_prefix(&format!("{kind}|")))
        .unwrap();
    let mut wire = Vec::new();
    for token in text.split_whitespace() {
        match token {
            "NAME" => wire.extend([32; 16]),
            "TABLE" => wire.extend([127; 384]),
            "Z12" => wire.extend([64; 12]),
            "Z24" => wire.extend([64, 0].repeat(12)),
            _ => wire.push(u8::from_str_radix(token, 16).unwrap()),
        }
    }
    SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap()
}
fn event(raw: SysEx) -> Item {
    Item::Event(Box::new(Event {
        stream: "s".into(),
        timestamp: None,
        span: None,
        message: Payload::Midi1(meedee::messages::AnyMessage::SysEx(raw)),
    }))
}
fn query() -> Query {
    Query::new(BulkDumpRequest::new(1, 2).unwrap().into(), "s".into(), 10).unwrap()
}
#[test]
fn matched_legacy_bytes_are_retained_without_checksum_normalization() {
    let mut q = query();
    assert!(q.write(&mut Vec::new()).is_err());
    assert_eq!(q.prepare(100).unwrap().data(), &[126, 1, 8, 0, 2]);
    assert!(q.prepare(100).is_err());
    q.confirm(101).unwrap();
    q.feed(&event(SysEx::new(vec![125, 1]).unwrap()), 102)
        .unwrap();
    let saved = q.snapshot();
    let mut bytes = dump("BulkDump").data().to_vec();
    *bytes.last_mut().unwrap() ^= 1;
    q.feed(&event(SysEx::new(bytes.clone()).unwrap()), 109)
        .unwrap();
    let report = q.snapshot();
    assert_eq!(saved.status, "waiting");
    assert_eq!(report.status, "completed");
    assert_eq!(report.ignored_events, 1);
    assert!(report.request_sent && !report.uncertain_send);
    assert_eq!(
        report.decoded.unwrap().checksum,
        ChecksumStatus::LegacyMismatch
    );
    let mut exported = Vec::new();
    q.write(&mut exported).unwrap();
    assert_eq!(&exported[1..exported.len() - 1], bytes);
    q.fail("late failure");
    assert_eq!(q.snapshot().status, "completed");
}
#[test]
fn existing_banked_matching_accepts_both_dump_kinds_and_ignores_wrong_programs() {
    for kind in ["KeyBasedDump", "ScaleOctaveDump7", "ScaleOctaveDump14"] {
        let mut q = Query::new(
            BankedDumpRequest::new(1, 3, 2).unwrap().into(),
            "s".into(),
            10,
        )
        .unwrap();
        q.prepare(0).unwrap();
        assert!(q.feed(&event(dump(kind)), 0).is_err());
        q.confirm(1).unwrap();
        q.feed(&event(dump("BulkDump")), 2).unwrap();
        assert_eq!(q.snapshot().status, "waiting");
        q.feed(&event(dump(kind)), 3).unwrap();
        assert_eq!(q.snapshot().status, "completed");
        assert_eq!(
            q.snapshot().decoded.unwrap().checksum,
            ChecksumStatus::Matches
        );
    }
    assert!(Query::new(BulkDumpRequest::new(127, 2).unwrap().into(), "s".into(), 1).is_err());
}
#[test]
fn deadlines_and_failure_boundaries_preserve_known_send_progress() {
    let mut q = query();
    assert!(q.prepare(u64::MAX).is_err());
    assert_eq!(q.snapshot().status, "ready");
    q.prepare(0).unwrap();
    q.fail("uncertain send");
    assert!(q.snapshot().uncertain_send && !q.snapshot().request_sent);
    assert!(q.prepare(1).is_err());
    let mut q = query();
    q.prepare(10).unwrap();
    assert!(q.confirm(9).is_err());
    q.confirm(20).unwrap();
    assert_eq!(q.snapshot().status, "timed_out");
    assert!(q.snapshot().request_sent && !q.snapshot().uncertain_send);
    let mut q = query();
    q.prepare(0).unwrap();
    q.confirm(1).unwrap();
    q.feed(&event(dump("BulkDump")), 10).unwrap();
    assert_eq!(q.snapshot().status, "timed_out");
    assert!(q.snapshot().reply.is_none());
    let mut q = query();
    q.prepare(0).unwrap();
    q.confirm(1).unwrap();
    q.feed(
        &Item::Diagnostic(Diagnostic {
            stream: "s".into(),
            packet: 0,
            kind: DiagnosticKind::Loss,
            lost: 1,
            reason: "lost".into(),
        }),
        2,
    )
    .unwrap();
    assert_eq!(q.snapshot().status, "failed");
    assert!(q.snapshot().request_sent && !q.snapshot().uncertain_send);
    let mut q = query();
    q.cancel("before send");
    assert!(!q.snapshot().uncertain_send && !q.snapshot().request_sent);
}
