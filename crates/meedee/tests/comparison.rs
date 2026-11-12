use meedee::{
    inspection::Format,
    inspection::{compare, ComparisonOptions},
};

#[test]
fn streaming_comparison_counts_differences_and_never_equates_invalid_prefixes() {
    let o = ComparisonOptions {
        detail_limit: 1,
        ..ComparisonOptions::default()
    };
    let r = compare(
        &b"\xf0\x01\xf7\xf0\x01\xf7"[..],
        &b"\xf0\x02\xf7"[..],
        Format::Syx,
        &o,
    )
    .unwrap();
    assert_eq!(r.differences, 2);
    assert_eq!(r.examples.len(), 1);
    assert_eq!(r.examples[0].record, Some(0));
    assert!(!r.equal);
    assert!(r.left_complete && r.right_complete);
    let r = compare(&b"\xf0\x01"[..], &b"\xf0\x01"[..], Format::Syx, &o).unwrap();
    assert!(!r.equal && !r.left_complete && !r.right_complete);
}

#[test]
fn archive_comparison_retains_occurrence_metadata_while_message_equality_ignores_it() {
    use meedee::archive::{Reader, Record, Writer};
    use meedee::events::{Clock, Payload, SourceSpan, Timestamp};
    use meedee::messages::{AnyMessage, Message, Metadata, NoteOn};
    let clock = Clock::new("clock".into(), 1, 1000, "test".into(), None).unwrap();
    let first = Payload::Message(AnyMessage::NoteOn(
        NoteOn::new(60).unwrap().with_channel(1).unwrap(),
    ))
    .with_metadata(Metadata {
        timestamp: Some(Timestamp {
            ticks: 1,
            clock: clock.clone(),
        }),
        stream: Some("keys".into()),
        span: Some(SourceSpan {
            first_packet: 1,
            last_packet: 2,
            first_timestamp: None,
            last_timestamp: None,
        }),
    })
    .unwrap();
    let later = first
        .clone()
        .with_timestamp(Timestamp { ticks: 2, clock })
        .unwrap();
    assert_eq!(first, later);
    assert_ne!(
        Record::Event(Box::new(first.clone())),
        Record::Event(Box::new(later.clone()))
    );

    fn capture(message: &Payload) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes, 1024).unwrap();
        writer
            .write(&Record::Clock(message.timestamp().unwrap().clock.clone()))
            .unwrap();
        writer
            .write(&Record::Stream {
                id: message.stream().unwrap().into(),
                label: "keys".into(),
                clock_identity: Some("clock".into()),
            })
            .unwrap();
        writer
            .write(&Record::Event(Box::new(message.clone())))
            .unwrap();
        writer.finish(true, "").unwrap();
        bytes
    }
    let bytes = capture(&first);
    let decoded: Vec<_> = Reader::new(bytes.as_slice(), 1024, false, false)
        .unwrap()
        .collect::<std::io::Result<_>>()
        .unwrap();
    let Record::Event(decoded) = &decoded[2] else {
        panic!("expected message record")
    };
    assert_eq!(decoded.metadata(), first.metadata());
    let later_bytes = capture(&later);
    assert!(
        !compare(
            bytes.as_slice(),
            later_bytes.as_slice(),
            Format::Archive,
            &ComparisonOptions::default()
        )
        .unwrap()
        .equal
    );
    let options = ComparisonOptions {
        tick_tolerance: 1,
        ..ComparisonOptions::default()
    };
    assert!(
        compare(
            bytes.as_slice(),
            later_bytes.as_slice(),
            Format::Archive,
            &options
        )
        .unwrap()
        .equal
    );
    let changed_source = capture(&first.clone().with_stream("other").unwrap());
    let options = ComparisonOptions {
        source_map: [("keys".into(), "other".into())].into(),
        ..ComparisonOptions::default()
    };
    assert!(
        compare(
            bytes.as_slice(),
            changed_source.as_slice(),
            Format::Archive,
            &options
        )
        .unwrap()
        .equal
    );
    let changed_span = capture(
        &first
            .clone()
            .with_span(SourceSpan {
                last_packet: 3,
                ..first.span().unwrap().clone()
            })
            .unwrap(),
    );
    let options = ComparisonOptions {
        ignore_timestamps: true,
        ..ComparisonOptions::default()
    };
    assert!(
        !compare(
            bytes.as_slice(),
            changed_span.as_slice(),
            Format::Archive,
            &options
        )
        .unwrap()
        .equal
    );
}
