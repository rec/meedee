use meedee::{
    ci::{
        self, Address, Body, DiscoveryInfo, Header, Identity, Management, Notice, BROADCAST_MUID,
    },
    ci_session::{Route, Session, Update},
    messages::{PrimitiveMessage, SysEx},
};
fn identity() -> Identity {
    Identity::new(vec![125, 0, 0], 1, 2, vec![1, 2, 3, 4]).unwrap()
}
fn info(path: Option<i64>) -> DiscoveryInfo {
    DiscoveryInfo::new(identity(), 0, 512, path).unwrap()
}
fn header(source: i64, destination: i64, version: i64) -> Header {
    Header::new(source, destination, version, Address::FUNCTION_BLOCK).unwrap()
}
fn management(source: i64, destination: i64, body: Body) -> Management {
    Management::new(header(source, destination, 2), body, vec![]).unwrap()
}
fn fixtures() -> Vec<Management> {
    vec![
        management(1, BROADCAST_MUID.into(), Body::Discovery(info(Some(3)))),
        management(
            2,
            1,
            Body::DiscoveryReply {
                info: info(Some(3)),
                function_block: None,
            },
        ),
        management(1, 2, Body::EndpointInquiry),
        management(
            2,
            1,
            Body::EndpointReply {
                product_instance_id: "ABC".into(),
            },
        ),
        management(1, BROADCAST_MUID.into(), Body::InvalidateMuid { target: 2 }),
        Management::new(
            Header::new(2, 1, 2, Address::channel(1).unwrap()).unwrap(),
            Body::Ack(Notice::new(0x72, 0, 0, vec![0; 5], b"OK!".to_vec()).unwrap()),
            vec![],
        )
        .unwrap(),
        management(
            2,
            1,
            Body::Nak(Some(Notice::new(0x72, 1, 0, vec![0; 5], vec![]).unwrap())),
        ),
        Management::new(header(2, 1, 1), Body::Nak(None), vec![]).unwrap(),
        Management::new(
            header(1, BROADCAST_MUID.into(), 1),
            Body::Discovery(info(None)),
            vec![],
        )
        .unwrap(),
    ]
}
#[test]
fn management_vectors_legacy_versions_and_all_truncations() {
    let lines = include_str!("fixtures/ci.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    for (message, line) in fixtures().into_iter().zip(lines) {
        let (_, hex) = line.split_once('|').unwrap();
        let wire: Vec<u8> = hex
            .split_whitespace()
            .map(|n| u8::from_str_radix(n, 16).unwrap())
            .collect();
        assert_eq!(message.encode(), wire);
        assert_eq!(ci::decode(&message.sysex()).unwrap(), Some(message.clone()));
        let payload = message.sysex();
        for cut in 3..payload.data().len() {
            assert!(
                ci::decode(&SysEx::new(payload.data()[..cut].to_vec()).unwrap()).is_err(),
                "{} at {cut}",
                message.kind()
            );
        }
    }
}
#[test]
fn reserved_identifiers_addresses_versions_and_extensions_are_explicit() {
    for n in [0x0fff_ff00, 0x0fff_fffe, 0x0fff_ffff, -1] {
        assert!(Header::new(n, 1, 2, Address::FUNCTION_BLOCK).is_err());
    }
    assert!(Header::new(
        ci::MAX_MUID.into(),
        BROADCAST_MUID.into(),
        2,
        Address::channel(16).unwrap()
    )
    .is_ok());
    assert!(Address::channel(0).is_err());
    assert!(Address::channel(17).is_err());
    let mut b = fixtures()[0].sysex().data().to_vec();
    b[4] = 3;
    b.extend([1, 2, 127]);
    let value = ci::decode(&SysEx::new(b.clone()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(value.extensions(), [1, 2, 127]);
    assert_eq!(value.sysex().data(), b);
    b[4] = 2;
    assert!(ci::decode(&SysEx::new(b.clone()).unwrap()).is_err());
    b[4] = 32;
    assert!(ci::decode(&SysEx::new(b).unwrap()).is_err());
    assert!(ci::decode(&SysEx::new(vec![125, 1]).unwrap())
        .unwrap()
        .is_none());
    let mut b = fixtures()[0].sysex().data().to_vec();
    b[3] = 0x34;
    assert!(ci::decode(&SysEx::new(b).unwrap()).unwrap().is_none());
    assert!(Management::new(
        header(1, BROADCAST_MUID.into(), 3),
        Body::Discovery(info(Some(3))),
        vec![0; 481]
    )
    .is_err());
    assert_eq!(
        Management::new(
            header(1, BROADCAST_MUID.into(), 3),
            Body::Discovery(info(Some(3))),
            vec![0; 480]
        )
        .unwrap()
        .encode()
        .len(),
        512
    );
    assert!(management_text(vec![255]).is_err());
    assert!(management_text(vec![0; 16384]).is_err());
}
fn management_text(text: Vec<u8>) -> Result<Notice, meedee::messages::ValueError> {
    Notice::new(0x72, 0, 0, vec![0; 5], text)
}
fn session(local: i64, input: &str, path: i64, peers: usize, exchanges: usize) -> Session {
    Session::new(
        local,
        identity(),
        Route::new(input.into(), format!("{input}-out"), None).unwrap(),
        path,
        None,
        512,
        peers,
        exchanges,
        8_388_608,
    )
    .unwrap()
}
fn action(updates: &[Update]) -> Management {
    updates
        .iter()
        .find_map(|u| {
            if let Update::Send(a) = u {
                Some(a.message.clone())
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn discovery_endpoint_requests_and_send_results_use_explicit_routes_and_time() {
    let mut a = session(1, "a", 3, 16, 16);
    let mut b = session(2, "b", 0, 16, 16);
    assert!(a.discover(0, 2_999_999_999).is_err());
    let request = a.discover(0, 3_000_000_000).unwrap();
    a.report_send(request.token, true, 1).unwrap();
    let updates = b.feed(&request.message.sysex(), "b", None, 2).unwrap();
    let reply = action(&updates);
    assert!(matches!(reply.body(),Body::DiscoveryReply{info,..}if info.output_path()==Some(3)));
    let updates = a.feed(&reply.sysex(), "a", None, 3).unwrap();
    assert!(updates
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="discovery_reply")));
    assert_eq!(a.peers().len(), 1);
    let q = a.inquire_endpoint(2, 4, 100).unwrap();
    a.report_send(q.token, true, 4).unwrap();
    let reply = management(
        2,
        1,
        Body::EndpointReply {
            product_instance_id: "serial".into(),
        },
    );
    a.feed(&reply.sysex(), "a", None, 5).unwrap();
    assert_eq!(a.peers()[0].product_instance_id.as_deref(), Some("serial"));
    assert!(a.feed(&reply.sysex(), "wrong", None, 5).is_err());
    assert!(a.feed(&reply.sysex(), "a", Some(1), 5).is_err());
    assert!(a.poll(4).is_err());
    assert!(a
        .poll(3_000_000_001)
        .unwrap()
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="discovery_finished")));
}
#[test]
fn loss_collisions_deadlines_and_admission_do_not_reuse_transaction_state() {
    let mut a = session(1, "a", 0, 1, 1);
    let d = a.discover(0, 3_000_000_000).unwrap();
    assert!(a.discover(0, 3_000_000_000).is_err());
    a.report_send(d.token, false, 0).unwrap();
    assert_eq!(a.pending_count(), 0);
    let d = a.discover(0, 3_000_000_000).unwrap();
    a.report_send(d.token, true, 0).unwrap();
    let reply = management(
        2,
        1,
        Body::DiscoveryReply {
            info: info(Some(0)),
            function_block: None,
        },
    );
    a.feed(&reply.sysex(), "a", None, 1).unwrap();
    assert_eq!(a.peers().len(), 1);
    let other = management(
        3,
        1,
        Body::DiscoveryReply {
            info: info(Some(0)),
            function_block: None,
        },
    );
    let u = a.feed(&other.sysex(), "a", None, 2).unwrap();
    assert!(u
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="resource_limit")));
    assert_eq!(a.peers().len(), 1);
    let collision = management(1, BROADCAST_MUID.into(), Body::Discovery(info(Some(0))));
    let u = a.feed(&collision.sysex(), "a", None, 3).unwrap();
    assert!(!a.active());
    assert!(matches!(
        action(&u).body(),
        Body::InvalidateMuid { target: 1 }
    ));
    assert!(a.peers().is_empty());
    assert!(a.restart(1, 4).is_err());
    a.restart(4, 4).unwrap();
    assert_eq!(a.pending_count(), 0);
    let d = a.discover(4, 3_000_000_000).unwrap();
    let u = a.poll(3_000_000_004).unwrap();
    assert!(u
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="send_confirmation_timeout")));
    assert!(a.report_send(d.token, true, 3_000_000_004).is_err());
    a.reset("input loss".into());
    assert!(a.inquire_endpoint(2, 3_000_000_004, 100).is_err());
}
#[test]
fn malformed_self_messages_do_not_invalidate_and_peer_collisions_can_emit_recovery_at_capacity() {
    let mut a = session(1, "a", 0, 1, 1);
    let mut malformed = fixtures()[0].sysex().data().to_vec();
    malformed.truncate(13);
    let u = a
        .feed(&SysEx::new(malformed).unwrap(), "a", None, 0)
        .unwrap();
    assert!(a.active());
    assert!(u
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="malformed")));
    // Retire the explicit NAK action before starting the discovery.
    for u in u {
        if let Update::Send(action) = u {
            a.report_send(action.token, true, 0).unwrap();
        }
    }
    let d = a.discover(0, 3_000_000_000).unwrap();
    a.report_send(d.token, true, 0).unwrap();
    let reply = management(
        2,
        1,
        Body::DiscoveryReply {
            info: info(Some(0)),
            function_block: None,
        },
    );
    a.feed(&reply.sysex(), "a", None, 1).unwrap();
    let changed = Identity::new(vec![125, 0, 0], 9, 2, vec![1, 2, 3, 4]).unwrap();
    let info = DiscoveryInfo::new(changed, 0, 512, Some(0)).unwrap();
    let collision = management(
        2,
        1,
        Body::DiscoveryReply {
            info,
            function_block: None,
        },
    );
    let u = a.feed(&collision.sysex(), "a", None, 2).unwrap();
    assert!(a.peers().is_empty());
    assert!(matches!(
        action(&u).body(),
        Body::InvalidateMuid { target: 2 }
    ));
    assert_eq!(a.pending_count(), 1);
}
#[test]
fn replies_before_send_confirmation_are_bounded_and_cannot_restore_invalidated_peers() {
    let mut a = session(1, "a", 3, 16, 16);
    let d = a.discover(0, 3_000_000_000).unwrap();
    let reply = management(
        2,
        1,
        Body::DiscoveryReply {
            info: info(Some(3)),
            function_block: None,
        },
    );
    let u = a.feed(&reply.sysex(), "a", None, 1).unwrap();
    assert!(a.peers().is_empty());
    assert!(u
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="reply_pending_send_result")));
    a.report_send(d.token, true, 2).unwrap();
    assert_eq!(a.peers().len(), 1);
    let q = a.inquire_endpoint(2, 3, 100).unwrap();
    let response = management(
        2,
        1,
        Body::EndpointReply {
            product_instance_id: "ABC".into(),
        },
    );
    a.feed(&response.sysex(), "a", None, 4).unwrap();
    assert!(a.peers()[0].product_instance_id.is_none());
    a.report_send(q.token, false, 5).unwrap();
    assert!(a.peers()[0].product_instance_id.is_none());
    let invalidate = management(2, BROADCAST_MUID.into(), Body::InvalidateMuid { target: 2 });
    a.feed(&invalidate.sysex(), "a", None, 6).unwrap();
    assert!(a.peers().is_empty());
    a.feed(&reply.sysex(), "a", None, 7).unwrap();
    assert!(a.peers().is_empty());
    assert_eq!(a.pending_count(), 0);
    let mut small = Session::new(
        1,
        identity(),
        Route::new("s".into(), "out".into(), None).unwrap(),
        3,
        None,
        512,
        16,
        16,
        1,
    )
    .unwrap();
    small.discover(0, 3_000_000_000).unwrap();
    let u = small.feed(&reply.sysex(), "s", None, 1).unwrap();
    assert!(u
        .iter()
        .any(|u| matches!(u,Update::Observe(o)if o.kind=="resource_limit")));
    assert!(small.peers().is_empty());
}
