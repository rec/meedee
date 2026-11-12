use meedee::messages::Message;
use meedee::{
    archive::{self, Record},
    events::{Clock, Payload, Rational, Timestamp},
    messages::{AnyMessage, NoteOn},
};
use std::io::{self, Cursor};
fn records() -> Vec<Record> {
    let c = Clock::new(
        "clock".into(),
        1,
        1_000_000_000,
        "monotonic".into(),
        Some(1),
    )
    .unwrap();
    vec![
        Record::Clock(c.clone()),
        Record::Stream {
            id: "source".into(),
            label: "keys".into(),
            clock_identity: None,
        },
        Record::Event(Box::new(
            (Payload::Midi1(AnyMessage::NoteOn(
                NoteOn::new(127)
                    .and_then(|m| m.with_velocity(127))
                    .and_then(|m| m.with_channel(16))
                    .unwrap(),
            )))
            .with_metadata(meedee::messages::Metadata {
                stream: Some("source".into()),
                timestamp: Some(Timestamp {
                    ticks: i64::MIN,
                    clock: c,
                }),
                span: None,
            })
            .unwrap(),
        )),
        Record::Opaque {
            kind: 100,
            data: vec![0, 255],
        },
        Record::Gap {
            stream: "source".into(),
            lost: 2,
            reason: "overflow".into(),
        },
    ]
}
fn saved() -> Vec<u8> {
    let mut bytes = vec![];
    {
        let mut w = archive::Writer::new(&mut bytes, 1024).unwrap();
        for r in records() {
            assert!(archive::payload_size(&r).unwrap() >= archive::encode(&r).unwrap().len());
            w.write(&r).unwrap();
        }
        w.finish(true, "").unwrap();
    }
    bytes
}

#[test]
fn fourteen_bit_controller_retains_the_existing_command_encoding_and_budget() {
    use meedee::messages::{AnyCommand, ControlChange, Metadata};
    let value = ControlChange::new_with_resolution(7, 10000, 14)
        .unwrap()
        .with_channel(2)
        .unwrap();
    let metadata = Metadata {
        stream: Some("source".into()),
        ..Metadata::default()
    };
    let message = Record::Event(Box::new(
        Payload::Midi1(AnyMessage::ControlChange(value.clone()))
            .with_metadata(metadata.clone())
            .unwrap(),
    ));
    let command = Record::Event(Box::new(
        Payload::Command(AnyCommand::ControlChange(value))
            .with_metadata(metadata)
            .unwrap(),
    ));
    let bytes = archive::encode(&message).unwrap();
    assert_eq!(bytes, archive::encode(&command).unwrap());
    assert!(archive::payload_size(&message).unwrap() >= bytes.len());
}
#[test]
fn archive_integrity_order_exact_time_and_verified_recovery() {
    assert_eq!(archive::crc32(b"123456789"), 0xcbf43926);
    let b = saved();
    let mut r = archive::Reader::new(Cursor::new(&b), 1024, false, true).unwrap();
    assert_eq!(
        r.by_ref().collect::<io::Result<Vec<_>>>().unwrap(),
        records()
    );
    assert!(r.complete);
    assert!(r.problem.is_none());
    for cut in 12..b.len() {
        let mut r = archive::Reader::new(Cursor::new(&b[..cut]), 1024, true, true).unwrap();
        let prefix = r.by_ref().collect::<io::Result<Vec<_>>>().unwrap();
        assert!(prefix.len() <= records().len());
        assert!(!r.complete && r.problem.is_some());
        assert!(
            archive::Reader::new(Cursor::new(&b[..cut]), 1024, false, true)
                .unwrap()
                .collect::<io::Result<Vec<_>>>()
                .is_err()
        );
    }
    let mut corrupt = b.clone();
    corrupt[20] ^= 1;
    let mut r = archive::Reader::new(Cursor::new(corrupt), 1024, true, true).unwrap();
    assert!(r.next().is_none());
    assert!(r.problem.as_ref().unwrap().contains("CRC"));
    assert!(archive::Reader::new(Cursor::new(&b), 2, false, true)
        .unwrap()
        .next()
        .unwrap()
        .is_err());
    let mut oversized = archive::MAGIC.to_vec();
    oversized.extend(u32::MAX.to_be_bytes());
    assert!(
        archive::Reader::new(Cursor::new(oversized), 1024, false, true)
            .unwrap()
            .next()
            .unwrap()
            .is_err()
    );
}
#[test]
fn archive_declared_identity_completion_and_future_records_are_explicit() {
    let mut b = vec![];
    {
        let mut w = archive::Writer::new(&mut b, 1024).unwrap();
        for r in records().into_iter().take(3) {
            w.write(&r).unwrap();
        }
        w.finish(false, "lost input").unwrap();
    }
    let mut r = archive::Reader::new(Cursor::new(&b), 1024, true, false).unwrap();
    assert_eq!(r.by_ref().count(), 3);
    assert!(!r.complete);
    assert!(r.problem.as_ref().unwrap().contains("lost input"));
    let mut w = archive::Writer::new(vec![], 1024).unwrap();
    assert!(w.write(&records().remove(2)).is_err());
    assert!(w.finish(true, "").is_err());
    let b = saved();
    assert!(archive::Reader::new(Cursor::new(b), 1024, false, false)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .is_err());
}
#[test]
fn rational_arithmetic_and_rounding_never_use_floating_seconds() {
    let r = Rational::new(3, 2).unwrap();
    assert!(r.round(false).is_err());
    assert_eq!(r.round(true).unwrap(), (2, Rational::new(1, 2).unwrap()));
    assert_eq!(Rational::new(-3, 2).unwrap().round(true).unwrap().0, -2);
    assert_eq!(Rational::new(5, 2).unwrap().round(true).unwrap().0, 2);
    let c = Clock::new("c".into(), 1001, 30000, "SMPTE".into(), None).unwrap();
    assert_eq!(
        (Timestamp {
            ticks: 30000,
            clock: c
        })
        .seconds()
        .unwrap(),
        Rational::new(1001, 1).unwrap()
    );
    let fractional = Timestamp {
        ticks: -1,
        clock: Clock::new("fractional".into(), 1001, 30000, "SMPTE".into(), None).unwrap(),
    };
    assert_eq!(
        fractional.seconds().unwrap(),
        Rational::new(-1001, 30000).unwrap()
    );
    let disabled = Timestamp {
        ticks: 1,
        clock: fractional.clock.with_transport(1, false),
    };
    assert_eq!(
        disabled.seconds().unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert!(Rational::new(i128::MAX, 1).unwrap().multiply(2, 1).is_err());
}

#[test]
fn independent_v1_vectors_validate_future_headers_and_preserve_exact_bytes() {
    for line in include_str!("fixtures/archive-v1.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('|').collect();
        let (name, expected, encoded) = (fields[0], fields[1], fields[2]);
        let bytes: Vec<_> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect();
        let mut reader = archive::Reader::new(Cursor::new(&bytes), 4096, false, true).unwrap();
        let result = reader.by_ref().collect::<io::Result<Vec<_>>>();
        if expected != "valid" {
            assert!(result.unwrap_err().to_string().contains(expected), "{name}");
            assert!(!reader.complete);
            let mut recovery = archive::Reader::new(Cursor::new(&bytes), 4096, true, true).unwrap();
            assert!(recovery.by_ref().collect::<io::Result<Vec<_>>>().is_ok());
            assert!(!recovery.complete);
            assert!(recovery.problem.unwrap().contains(expected), "{name}");
            continue;
        }
        let records = result.unwrap();
        assert!(reader.complete && reader.problem.is_none(), "{name}");
        let mut rewritten = vec![];
        let mut writer = archive::Writer::new(&mut rewritten, 4096).unwrap();
        for record in &records {
            writer.write(record).unwrap();
        }
        writer.finish(true, "").unwrap();
        assert_eq!(rewritten, bytes, "{name}");
        if name.starts_with("future-") {
            assert!(matches!(
                records.last(),
                Some(Record::Opaque { kind: 3, .. })
            ));
            assert!(
                archive::Reader::new(Cursor::new(&bytes), 4096, false, false)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .is_err()
            );
        }
    }
}

#[test]
fn parameter_steps_keep_the_existing_archive_ids_and_fields() {
    use meedee::{
        controls::{ParameterFamily, StepDirection},
        messages::{AnyCommand, ParameterStep},
    };
    for (id, family, direction) in [
        (4u16, ParameterFamily::Rpn, StepDirection::Increment),
        (5, ParameterFamily::Rpn, StepDirection::Decrement),
        (6, ParameterFamily::Nrpn, StepDirection::Increment),
        (7, ParameterFamily::Nrpn, StepDirection::Decrement),
    ] {
        let step = ParameterStep::new(7)
            .unwrap()
            .with_family(family)
            .unwrap()
            .with_direction(direction)
            .with_channel(2)
            .unwrap();
        let event = Payload::Command(AnyCommand::ParameterStep(step))
            .with_stream("s")
            .unwrap();
        let mut expected = vec![0, 3, 0, 0, 0, 1, b's', 0, 0, 3];
        expected.extend(id.to_be_bytes());
        expected.extend(2i64.to_be_bytes());
        expected.extend(7i64.to_be_bytes());
        assert_eq!(
            archive::encode(&Record::Event(Box::new(event))).unwrap(),
            expected
        );
    }
}
