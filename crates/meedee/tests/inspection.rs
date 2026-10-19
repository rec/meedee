use meedee::events::{Clock, Event, Payload, Timestamp};
use meedee::inspection::{inspect, Format};
use meedee::messages::{AnyMessage, NoteOn};
use meedee::{archive, file_io};
use std::io::{self, Read};

#[test]
fn archive_completion_is_separate_from_capture_loss() {
    let clock = Clock::new("c".into(), 1001, 30000, "SMPTE".into(), None).unwrap();
    let mut data = vec![];
    let mut writer = archive::Writer::new(&mut data, file_io::DEFAULT_LIMIT).unwrap();
    for r in [
        archive::Record::Clock(clock.clone()),
        archive::Record::Stream {
            id: "keys".into(),
            label: "Keyboard".into(),
            clock_identity: Some("c".into()),
        },
        archive::Record::Event(Box::new(Event {
            stream: "keys".into(),
            timestamp: Some(Timestamp {
                ticks: -3,
                clock: clock.clone(),
            }),
            span: None,
            message: Payload::Midi1(AnyMessage::NoteOn(NoteOn::new(16, 60, 100).unwrap())),
        })),
        archive::Record::Gap {
            stream: "keys".into(),
            lost: u64::MAX,
            reason: "overflow".into(),
        },
        archive::Record::Gap {
            stream: "keys".into(),
            lost: 1,
            reason: "overflow".into(),
        },
        archive::Record::Discontinuity {
            stream: "keys".into(),
            reason: "reset".into(),
        },
    ] {
        writer.write(&r).unwrap();
    }
    writer.finish(true, "").unwrap();
    let r = inspect(
        data.as_slice(),
        Format::Archive,
        file_io::DEFAULT_LIMIT,
        false,
        128,
    )
    .unwrap();
    assert!(r.complete);
    assert!(r.problem.is_none());
    assert_eq!(r.records, 6);
    assert_eq!(r.message_counts["NoteOn"], 1);
    assert_eq!(r.clocks, [clock]);
    assert_eq!(r.streams.len(), 1);
    assert_eq!(r.gap_records, 2);
    assert_eq!(r.lost_events, u128::from(u64::MAX) + 1);
    assert_eq!(r.discontinuities, 1);
    assert_eq!(r.untimed_events, 0);
    // A valid prefix cannot establish a complete file, even after all events.
    data.truncate(data.len() - 1);
    let r = inspect(
        data.as_slice(),
        Format::Archive,
        file_io::DEFAULT_LIMIT,
        false,
        128,
    )
    .unwrap();
    assert!(!r.complete);
    assert!(r.problem.unwrap().contains("truncated"));
    assert_eq!(r.records, 6);
}

#[test]
fn malformed_input_returns_only_verified_prefix_counts() {
    let r = inspect(&b"\xf0\x7d\xf7\xf0\x01"[..], Format::Syx, 10, false, 128).unwrap();
    assert!(!r.complete);
    assert_eq!(r.records, 1);
    assert_eq!(r.message_counts["SysEx"], 1);
    assert!(r.problem.unwrap().contains("byte 5"));
    let r = inspect(&b"invalid"[..], Format::Smf, 10, false, 128).unwrap();
    assert!(!r.complete);
    assert!(r.smf_header.is_none());
    assert_eq!(r.records, 0);
    assert!(r.problem.unwrap().contains("MThd"));
}

#[test]
fn input_failures_and_invalid_limits_are_not_format_reports() {
    struct FailedDisk;
    impl Read for FailedDisk {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "read denied",
            ))
        }
    }
    for format in [Format::Archive, Format::Smf, Format::Syx, Format::Clip] {
        assert_eq!(
            inspect(FailedDisk, format, 1024, false, 128)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            inspect(io::empty(), format, 0, false, 128)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
