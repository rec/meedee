use meedee::ci::{Address, Body, DiscoveryInfo, Header, Identity, Management};
use meedee::ci_browser::Browser;
use meedee::ci_session::{Route, Session};

fn identity() -> Identity {
    Identity::new(vec![125, 0, 0], 1, 2, vec![0; 4]).unwrap()
}
fn browser(limit: usize) -> Browser {
    let session = Session::new(
        1,
        identity(),
        Route::new("in".into(), "out".into(), None).unwrap(),
        0,
        None,
        1024,
        2,
        4,
        4096,
    )
    .unwrap();
    Browser::new(session, 3_000_000_000, 100, 4_000_000_000, limit).unwrap()
}
fn reply(source: i64) -> Management {
    Management::new(
        Header::new(source, 1, 2, Address::FUNCTION_BLOCK).unwrap(),
        Body::DiscoveryReply {
            info: DiscoveryInfo::new(identity(), 0, 1024, Some(0)).unwrap(),
            function_block: None,
        },
        vec![],
    )
    .unwrap()
}
#[test]
fn a_scan_queries_each_discovered_format_two_peer_once_and_retains_snapshots() {
    let mut b = browser(16);
    let a = b.start(0).unwrap().remove(0);
    b.report_send(a.token, true, 1).unwrap();
    b.feed(&reply(2).sysex(), "in", None, 2).unwrap();
    let saved = b.snapshot();
    let a = b.poll(3_000_000_001).unwrap().remove(0);
    assert!(matches!(a.message.body(), Body::EndpointInquiry));
    b.report_send(a.token, true, 3_000_000_002).unwrap();
    let reply = Management::new(
        Header::new(2, 1, 2, Address::FUNCTION_BLOCK).unwrap(),
        Body::EndpointReply {
            product_instance_id: "peer-2".into(),
        },
        vec![],
    )
    .unwrap();
    assert!(b
        .feed(&reply.sysex(), "in", None, 3_000_000_003)
        .unwrap()
        .is_empty());
    let r = b.snapshot();
    assert_eq!(r.status, "completed");
    assert_eq!(r.confirmed_actions, 2);
    assert_eq!(r.peers[0].product_instance_id.as_deref(), Some("peer-2"));
    assert_eq!(saved.peers[0].product_instance_id, None);
    assert!(b.start(4_000_000_000).is_err());
}
#[test]
fn silence_is_not_offline_evidence_and_diagnostics_are_bounded() {
    let mut b = browser(1);
    let a = b.start(0).unwrap().remove(0);
    b.report_send(a.token, true, 0).unwrap();
    b.feed(&reply(2).sysex(), "in", None, 1).unwrap();
    let a = b.poll(3_000_000_000).unwrap().remove(0);
    b.report_send(a.token, true, 3_000_000_000).unwrap();
    b.poll(3_000_000_100).unwrap();
    let r = b.snapshot();
    assert_eq!(r.status, "completed");
    assert_eq!(r.problems, 1);
    assert_eq!(r.peers.len(), 1);
    assert_eq!(r.diagnostics.len(), 1);
    assert!(r.omitted_diagnostics > 0);
}
#[test]
fn collision_retires_stale_actions_and_sends_only_required_invalidation() {
    let mut b = browser(16);
    let a = b.start(0).unwrap().remove(0);
    let actions = b.feed(&reply(1).sysex(), "in", None, 1).unwrap();
    assert_eq!(actions.len(), 1);
    assert!(matches!(
        actions[0].message.body(),
        Body::InvalidateMuid { .. }
    ));
    assert_eq!(b.snapshot().status, "stopping");
    assert!(b.report_send(a.token, true, 2).is_err());
    b.report_send(actions[0].token, true, 2).unwrap();
    assert_eq!(b.snapshot().status, "stopped");
    assert!(b.snapshot().pending_tokens.is_empty());
}
#[test]
fn send_failure_and_overall_deadline_stop_without_retry_or_peer_eviction() {
    let mut b = browser(16);
    let a = b.start(0).unwrap().remove(0);
    b.report_send(a.token, true, 0).unwrap();
    b.feed(&reply(2).sysex(), "in", None, 1).unwrap();
    b.poll(4_000_000_000).unwrap();
    assert_eq!(b.snapshot().peers.len(), 1);
    assert_eq!(b.snapshot().status, "stopped");
    assert!(b.poll(4_000_000_001).is_err());
    let mut b = browser(16);
    let a = b.start(0).unwrap().remove(0);
    b.report_send(a.token, false, 1).unwrap();
    assert_eq!(b.snapshot().confirmed_actions, 0);
    assert_eq!(b.snapshot().uncertain_token, Some(a.token));
    assert_eq!(b.snapshot().status, "stopped");
}

#[test]
fn early_replies_wait_for_handoff_and_known_endpoint_requests_receive_a_nak() {
    let mut b = browser(16);
    let a = b.start(0).unwrap().remove(0);
    b.feed(&reply(2).sysex(), "in", None, 1).unwrap();
    assert!(b.snapshot().peers.is_empty());
    b.report_send(a.token, true, 2).unwrap();
    assert_eq!(b.snapshot().peers.len(), 1);
    let request = Management::new(
        Header::new(2, 1, 2, Address::FUNCTION_BLOCK).unwrap(),
        Body::EndpointInquiry,
        vec![],
    )
    .unwrap();
    let actions = b.feed(&request.sysex(), "in", None, 3).unwrap();
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0].message.body(), Body::Nak(_)));
    b.report_send(actions[0].token, true, 4).unwrap();
    assert_eq!(b.snapshot().status, "discovering");
}
