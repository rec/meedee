use meedee::{
    archive::{self, Record},
    events::{Clock, Event, Payload, Rational, Timestamp},
    live::{Decoder, DiagnosticKind, Item},
    messages::{AnyMessage, NoteOn},
    playback::{self, Cancellation, PlaybackClock},
    recording,
    transport::{Packet, Protocol},
};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
fn clock() -> Clock {
    Clock::new(
        "c".into(),
        1,
        1_000_000_000,
        "backend_absolute".into(),
        None,
    )
    .unwrap()
    .with_transport(1, true)
}
fn event(ticks: i64) -> Event {
    Event {
        stream: "s".into(),
        timestamp: Some(Timestamp {
            ticks,
            clock: clock(),
        }),
        span: None,
        message: Payload::Midi1(AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap())),
    }
}
#[test]
fn completion_timestamps_and_spans_survive_realtime_and_running_status() {
    let mut d = Decoder::new("s".into(), Protocol::Midi1, 1024).unwrap();
    assert!(d
        .feed(&Packet::Bytes(vec![0x90, 60]), &clock(), Some(10), Some(0))
        .unwrap()
        .is_empty());
    let out = d
        .feed(
            &Packet::Bytes(vec![0xf8, 100, 61, 101]),
            &clock(),
            Some(20),
            Some(1),
        )
        .unwrap();
    assert_eq!(out.len(), 3);
    let Item::Event(e) = &out[1] else { panic!() };
    assert_eq!(e.timestamp.as_ref().unwrap().ticks, 20);
    let span = e.span.as_ref().unwrap();
    assert_eq!((span.first_packet, span.last_packet), (0, 1));
    assert_eq!(span.first_timestamp.as_ref().unwrap().ticks, 10);
    let Item::Event(rt) = &out[0] else { panic!() };
    assert_eq!(rt.span.as_ref().unwrap().first_packet, 1);
    let Item::Event(running) = &out[2] else {
        panic!()
    };
    assert_eq!(running.span.as_ref().unwrap().first_packet, 1);
    assert!(d.finish().is_empty());
}
#[test]
fn gaps_protocol_errors_and_clock_changes_cannot_join_old_fragments() {
    let mut d = Decoder::new("s".into(), Protocol::Midi1, 1024).unwrap();
    d.feed(&Packet::Bytes(vec![0x90, 60]), &clock(), Some(1), Some(0))
        .unwrap();
    let out = d
        .feed(&Packet::Bytes(vec![100]), &clock(), Some(2), Some(2))
        .unwrap();
    assert!(matches!(&out[0],Item::Diagnostic(v)if v.kind==DiagnosticKind::Loss&&v.lost==1));
    assert!(out.iter().all(|v| !matches!(v, Item::Event(_))));
    let c = Clock::new(
        "new".into(),
        1,
        1_000_000_000,
        "backend_absolute".into(),
        None,
    )
    .unwrap()
    .with_transport(1, true);
    let out = d
        .feed(&Packet::Bytes(vec![0x90, 60, 100]), &c, None, Some(3))
        .unwrap();
    assert!(matches!(&out[0],Item::Diagnostic(v)if v.kind==DiagnosticKind::ClockChange));
    let Item::Event(e) = &out[1] else { panic!() };
    assert!(e.timestamp.is_none());
    let mut d = Decoder::new("u".into(), Protocol::Ump, 1024).unwrap();
    assert!(d
        .feed(&Packet::Ump(vec![0x40903c00]), &clock(), Some(4), Some(0))
        .unwrap()
        .is_empty());
    let out = d
        .feed(&Packet::Ump(vec![0xffff0000]), &clock(), Some(5), Some(1))
        .unwrap();
    let Item::Event(e) = &out[0] else { panic!() };
    assert_eq!(e.span.as_ref().unwrap().first_packet, 0);
}
#[derive(Default)]
struct FakeClock {
    now: u64,
    waits: Vec<u64>,
}
impl PlaybackClock for FakeClock {
    fn now_ns(&mut self) -> u64 {
        self.now
    }
    fn wait_until(&mut self, n: u64, c: &Cancellation) -> io::Result<bool> {
        self.waits.push(n);
        self.now = self.now.max(n);
        Ok(!c.cancelled())
    }
}
#[test]
fn scheduler_keeps_absolute_order_reports_lateness_and_stops_at_failure() {
    let mut clock = FakeClock {
        now: 7,
        waits: vec![],
    };
    let mut sent = 0;
    let report = playback::play(
        vec![Ok(event(100)), Ok(event(100)), Ok(event(120))].into_iter(),
        playback::Planner::new(
            Timestamp {
                ticks: 100,
                clock: self::clock(),
            },
            vec![],
            false,
        )
        .unwrap(),
        &mut clock,
        &Cancellation::default(),
        |_| {
            sent += 1;
            Ok::<_, io::Error>(())
        },
    )
    .unwrap();
    assert_eq!(clock.waits, [0, 0, 20]);
    assert_eq!(report.sent_events, 3);
    assert_eq!(report.max_lateness_ns, 7);
    assert_eq!(sent, 3);
    let mut sent = 0;
    let failure = playback::play(
        vec![Ok(event(0)), Ok(event(1)), Ok(event(2))].into_iter(),
        playback::Planner::new(
            Timestamp {
                ticks: 0,
                clock: self::clock(),
            },
            vec![],
            false,
        )
        .unwrap(),
        &mut FakeClock::default(),
        &Cancellation::default(),
        |_| {
            sent += 1;
            if sent == 2 {
                Err(io::Error::other("device failed"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(failure.report.sent_events, 1);
    assert_eq!(sent, 2);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        playback::play(
            std::iter::once(Ok(event(0))),
            playback::Planner::new(
                Timestamp {
                    ticks: 0,
                    clock: self::clock()
                },
                vec![],
                false
            )
            .unwrap(),
            &mut FakeClock::default(),
            &cancel,
            |_| Ok::<_, io::Error>(())
        )
        .unwrap()
        .cancelled
    );
}
#[test]
fn explicit_mappings_and_quantization_preserve_clock_boundaries() {
    let c = Clock::new("fraction".into(), 1, 3, "test".into(), None).unwrap();
    let mapping = playback::Mapping::new(
        c.clone(),
        clock(),
        Rational::new(1, 1).unwrap(),
        Rational::new(0, 1).unwrap(),
        None,
    )
    .unwrap();
    let mut p = playback::Planner::new(
        Timestamp {
            ticks: 0,
            clock: clock(),
        },
        vec![mapping],
        false,
    )
    .unwrap();
    let mut a = event(0);
    a.timestamp = Some(Timestamp {
        ticks: 0,
        clock: c.clone(),
    });
    assert_eq!(p.deadline(&a).unwrap().delay_ns, 0);
    a.timestamp.as_mut().unwrap().ticks = 1;
    assert!(p.deadline(&a).is_err());
    let mut p = playback::Planner::new(
        Timestamp {
            ticks: 0,
            clock: c.clone(),
        },
        vec![],
        true,
    )
    .unwrap();
    a.timestamp.as_mut().unwrap().ticks = 0;
    p.deadline(&a).unwrap();
    a.timestamp.as_mut().unwrap().ticks = 1;
    let planned = p.deadline(&a).unwrap();
    assert_eq!(planned.delay_ns, 333_333_333);
    assert_eq!(planned.rounding_error_ns, Rational::new(-1, 3).unwrap());
    assert!(playback::Planner::new(
        Timestamp {
            ticks: 0,
            clock: clock()
        },
        vec![],
        false
    )
    .unwrap()
    .deadline(&a)
    .is_err());
}
#[derive(Clone)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl Write for Buffer {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn recording_finishes_verified_data_and_marks_loss_without_blocking_admission() {
    let saved = Arc::new(Mutex::new(vec![]));
    let r = recording::Recorder::new(
        Buffer(Arc::clone(&saved)),
        vec![
            Record::Clock(clock()),
            Record::Stream {
                id: "s".into(),
                label: "keys".into(),
                clock_identity: None,
            },
        ],
        1024,
        128,
        8192,
        false,
    )
    .unwrap();
    assert!(r.record(Record::Event(Box::new(event(1)))));
    let report = r.stop(Duration::from_secs(2)).unwrap();
    assert!(report.complete);
    let data = saved.lock().unwrap().clone();
    let mut reader = archive::Reader::new(&data[..], 1024, false, false).unwrap();
    assert_eq!(reader.by_ref().count(), 3);
    assert!(reader.complete);
    let saved = Arc::new(Mutex::new(vec![]));
    let r = recording::Recorder::new(
        Buffer(Arc::clone(&saved)),
        vec![
            Record::Clock(clock()),
            Record::Stream {
                id: "s".into(),
                label: "".into(),
                clock_identity: None,
            },
        ],
        1024,
        128,
        8192,
        false,
    )
    .unwrap();
    assert!(!r.record(Record::Gap {
        stream: "s".into(),
        lost: 1,
        reason: "input loss".into()
    }));
    assert!(!r.stop(Duration::from_secs(2)).unwrap().complete);
    assert!(
        archive::Reader::new(&saved.lock().unwrap()[..], 1024, false, false)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .is_err()
    );
}

struct GatedWriter {
    buffer: Buffer,
    gate: Arc<(Mutex<(bool, bool)>, std::sync::Condvar)>,
}
impl Write for GatedWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let (mut state, changed) = (self.gate.0.lock().unwrap(), &self.gate.1);
        if state.0 {
            state.1 = true;
            changed.notify_all();
            while state.0 {
                state = changed.wait(state).unwrap();
            }
        }
        drop(state);
        self.buffer.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn a_blocked_file_cannot_block_record_admission_or_exceed_stop_deadline() {
    let gate = Arc::new((Mutex::new((false, false)), std::sync::Condvar::new()));
    let bytes = Arc::new(Mutex::new(vec![]));
    let r = recording::Recorder::new(
        GatedWriter {
            buffer: Buffer(Arc::clone(&bytes)),
            gate: Arc::clone(&gate),
        },
        vec![
            Record::Clock(clock()),
            Record::Stream {
                id: "s".into(),
                label: "".into(),
                clock_identity: None,
            },
        ],
        1024,
        2,
        8192,
        false,
    )
    .unwrap();
    gate.0.lock().unwrap().0 = true;
    assert!(r.record(Record::Event(Box::new(event(0)))));
    let (s, wait) = gate
        .1
        .wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(2), |s| !s.1)
        .unwrap();
    assert!(!wait.timed_out());
    drop(s);
    assert!(r.record(Record::Event(Box::new(event(1)))));
    assert!(!r.record(Record::Event(Box::new(event(2)))));
    assert_eq!(
        r.stop(Duration::from_millis(10)).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    gate.0.lock().unwrap().0 = false;
    gate.1.notify_all();
    let report = r.stop(Duration::from_secs(2)).unwrap();
    assert!(!report.complete && report.failure.is_some());
}
