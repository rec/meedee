use meedee::{
    events::{Payload, Rational, Timestamp},
    file_timing,
    live::Item,
    messages::{AnyMessage, NoteOn},
    smf::{self, Event, Header, Record},
    ump,
};
use std::io::{self, Cursor};
fn file() -> Vec<u8> {
    let mut b = Cursor::new(vec![]);
    {
        let mut w =
            smf::Writer::new(&mut b, Header::new(1, 2, 480, vec![]).unwrap(), 1024).unwrap();
        for e in [
            Event {
                track: 0,
                tick: 0,
                record: Record::Meta {
                    kind: 81,
                    data: vec![7, 161, 32],
                },
            },
            Event {
                track: 0,
                tick: 240,
                record: Record::Meta {
                    kind: 81,
                    data: vec![3, 208, 144],
                },
            },
            Event {
                track: 0,
                tick: 480,
                record: Record::Meta {
                    kind: 47,
                    data: vec![],
                },
            },
            Event {
                track: 1,
                tick: 240,
                record: Record::Message(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap())),
            },
            Event {
                track: 1,
                tick: 480,
                record: Record::Message(AnyMessage::NoteOn(NoteOn::new(1, 61, 100).unwrap())),
            },
            Event {
                track: 1,
                tick: 480,
                record: Record::Meta {
                    kind: 47,
                    data: vec![],
                },
            },
        ] {
            w.write(&e).unwrap();
        }
        w.finish().unwrap();
    }
    b.into_inner()
}
#[test]
fn merged_tracks_apply_tempo_in_stable_order_and_preserve_leading_silence() {
    let bytes = file();
    let merge = smf::MergedReader::new(Cursor::new(&bytes), 1024, 128, 8192, None).unwrap();
    let merged = merge.collect::<io::Result<Vec<_>>>().unwrap();
    assert_eq!(
        merged.iter().map(|e| (e.tick, e.track)).collect::<Vec<_>>(),
        [(0, 0), (240, 0), (240, 1), (480, 0), (480, 1), (480, 1)]
    );
    let merge = smf::MergedReader::new(Cursor::new(&bytes), 1024, 128, 8192, None).unwrap();
    let mut timed = file_timing::SmfReader::new(merge, "song".into(), None, 1024).unwrap();
    let origin = timed.origin();
    assert_eq!(origin.ticks, 0);
    let result = timed.by_ref().collect::<io::Result<Vec<_>>>().unwrap();
    let events = result
        .into_iter()
        .map(|v| if let Item::Event(v) = v { *v } else { panic!() })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].timestamp.as_ref().unwrap().ticks, 120_000_000);
    assert_eq!(
        events[0].timestamp.as_ref().unwrap().clock.denominator(),
        480_000_000
    );
    assert_eq!(
        meedee::events::timestamp_seconds(events[0].timestamp.as_ref().unwrap()).unwrap(),
        Rational::new(1, 4).unwrap()
    );
    assert_eq!(
        meedee::events::timestamp_seconds(events[1].timestamp.as_ref().unwrap()).unwrap(),
        Rational::new(3, 8).unwrap()
    );
    let mut plan = meedee::playback::Planner::new(origin, vec![], false).unwrap();
    assert_eq!(plan.deadline(&events[0]).unwrap().delay_ns, 250_000_000);
    assert!(smf::MergedReader::new(Cursor::new(&bytes), 1024, 1, 8192, None).is_err());
}
#[test]
fn independent_sequences_smpte_and_export_rounding_are_explicit() {
    let header = Header::new(2, 2, 0xe328, vec![]).unwrap();
    let mut timeline =
        file_timing::SmfTimeline::new(header.clone(), "frames".into(), Some(1), 1024).unwrap();
    assert!(file_timing::SmfTimeline::new(header, "bad".into(), None, 1024).is_err());
    let result = timeline
        .feed(&Event {
            track: 1,
            tick: 1200,
            record: Record::Message(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap())),
        })
        .unwrap();
    let Item::Event(e) = &result[0] else { panic!() };
    assert_eq!(
        meedee::events::timestamp_seconds(e.timestamp.as_ref().unwrap()).unwrap(),
        Rational::new(1001, 1000).unwrap()
    );
    let target = Header::new(0, 1, 480, vec![]).unwrap();
    let report =
        file_timing::export_smf(e, &target, &timeline.origin(), 0, 500000, None, false).unwrap();
    assert_eq!(report.status, ump::TranslationStatus::Quantized);
    assert!(report.events.is_empty());
    let report =
        file_timing::export_smf(e, &target, &timeline.origin(), 0, 500000, None, true).unwrap();
    assert_eq!(report.events[0].tick, 961);
    assert_eq!(report.rounding_error_ticks, Rational::new(1, 25).unwrap());
    let mut missing = (**e).clone();
    missing.timestamp = None;
    assert_eq!(
        file_timing::export_smf(
            &missing,
            &target,
            &timeline.origin(),
            0,
            500000,
            None,
            false
        )
        .unwrap()
        .status,
        ump::TranslationStatus::ContextRequired
    );
}
#[test]
fn large_escape_records_stream_and_group_routing_is_not_silently_discarded() {
    let mut b = Cursor::new(vec![]);
    {
        let mut w =
            smf::Writer::new(&mut b, Header::new(0, 1, 480, vec![]).unwrap(), 1_048_576).unwrap();
        w.write(&Event {
            track: 0,
            tick: 0,
            record: Record::SysEx {
                status: 247,
                data: vec![248; 4096],
            },
        })
        .unwrap();
        w.write(&Event {
            track: 0,
            tick: 0,
            record: Record::Meta {
                kind: 47,
                data: vec![],
            },
        })
        .unwrap();
        w.finish().unwrap();
    }
    let merged =
        smf::MergedReader::new(Cursor::new(b.into_inner()), 1_048_576, 128, 8192, None).unwrap();
    let mut reader = file_timing::SmfReader::new(merged, "s".into(), None, 1_048_576).unwrap();
    assert_eq!(reader.by_ref().count(), 4096);
    let clock = reader.clock().clone();
    let origin = Timestamp {
        ticks: 0,
        clock: clock.clone(),
    };
    let event = meedee::events::Event {
        stream: "s".into(),
        timestamp: Some(origin.clone()),
        span: None,
        message: Payload::Ump(ump::UmpMessage::Midi2NoteOn(
            ump::Midi2NoteOn::new(2, 1, 60, 65535, 0, 0).unwrap(),
        )),
    };
    let report = file_timing::export_smf(
        &event,
        &Header::new(0, 1, 480, vec![]).unwrap(),
        &origin,
        0,
        500000,
        None,
        true,
    )
    .unwrap();
    assert_eq!(report.status, ump::TranslationStatus::ContextRequired);
    assert!(report.reason.contains("group"));
}
