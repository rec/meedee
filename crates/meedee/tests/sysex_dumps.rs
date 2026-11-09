//! Source-based vectors shared with the Python API tests. No hardware or sleeps.
use meedee::drivers::{Receiver, Sender};
use meedee::events::InputItem;
use meedee::events::{Cancellation, Payload, PlaybackClock};
use meedee::messages::{AnyMessage, Message, SysEx};
use meedee::{midi, sysex};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    rc::Rc,
    time::Duration,
};

#[derive(Clone, Default)]
struct Clock(Rc<Cell<u64>>);
impl PlaybackClock for Clock {
    fn now_ns(&mut self) -> u64 {
        self.0.get()
    }
    fn wait_until(&mut self, deadline: u64, cancel: &Cancellation) -> io::Result<bool> {
        self.0.set(deadline);
        Ok(!cancel.cancelled())
    }
}
struct Input {
    clock: Clock,
    queue: Rc<RefCell<VecDeque<InputItem>>>,
    scheduled: Vec<InputItem>,
}
impl Receiver for Input {
    fn receive(&mut self, timeout: Duration) -> midi::Result<Option<InputItem>> {
        if !timeout.is_zero() {
            self.queue
                .borrow_mut()
                .extend(std::mem::take(&mut self.scheduled));
        }
        if let Some(item) = self.queue.borrow_mut().pop_front() {
            return Ok(Some(item));
        }
        self.clock
            .0
            .set(self.clock.0.get() + timeout.as_nanos() as u64);
        Ok(None)
    }
    fn check(&self) -> midi::Result<()> {
        Ok(())
    }
}
struct Output {
    queue: Rc<RefCell<VecDeque<InputItem>>>,
    requests: VecDeque<Vec<u8>>,
    replies: VecDeque<Vec<u8>>,
    sent: usize,
}
impl Sender for Output {
    fn send(&mut self, message: Payload) -> midi::Result<()> {
        let Payload::Midi1(AnyMessage::SysEx(message)) = message else {
            panic!("request was not SysEx")
        };
        assert_eq!(message.data(), self.requests.pop_front().unwrap());
        self.queue
            .borrow_mut()
            .push_back(item(self.replies.pop_front().unwrap()));
        self.sent += 1;
        Ok(())
    }
}
fn item(data: Vec<u8>) -> InputItem {
    InputItem::Message(Box::new(
        Payload::Midi1(AnyMessage::SysEx(SysEx::new(data).unwrap()))
            .with_stream("s")
            .unwrap(),
    ))
}
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn cases() -> Vec<Vec<&'static str>> {
    include_str!("fixtures/sysex-dumps.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('|').collect())
        .collect()
}
#[test]
fn documented_requests_retain_exact_opaque_replies() {
    let vectors = cases();
    let keys: std::collections::BTreeSet<_> = vectors.iter().map(|v| (v[0], v[1])).collect();
    for (model, scope) in keys {
        let selected: Vec<_> = vectors
            .iter()
            .filter(|v| v[0] == model && v[1] == scope)
            .collect();
        let replies: Vec<_> = selected
            .iter()
            .map(|v| {
                let mut data = hex(v[3]);
                data.extend(vec![0; v[4].parse().unwrap()]);
                data.extend(hex(v[5]));
                data
            })
            .collect();
        let clock = Clock::default();
        let queue = Rc::new(RefCell::new(VecDeque::from([item(replies[0].clone())])));
        let mut receiver = Input {
            clock: clock.clone(),
            queue: queue.clone(),
            scheduled: vec![],
        };
        let mut output = Output {
            queue,
            requests: selected.iter().map(|v| hex(v[2])).collect(),
            replies: replies.clone().into(),
            sent: 0,
        };
        let report = sysex::dump_with(
            &mut receiver,
            Some(&mut output),
            "s",
            model,
            &sysex::DumpOptions {
                scope: scope.into(),
                ..Default::default()
            },
            &mut clock.clone(),
        )
        .unwrap();
        assert_eq!(report.status, selected[0][6], "{model} {scope}");
        assert_eq!(report.complete, report.status == "complete");
        assert_eq!(report.requests_sent, selected.len());
        assert!(!report.uncertain_send);
        assert_eq!(report.ignored_events, 1);
        assert_eq!(
            report
                .messages
                .iter()
                .map(|m| m.data().to_vec())
                .collect::<Vec<_>>(),
            replies
        );
        assert_eq!(output.sent, selected.len());
        let mut saved = vec![];
        for m in &report.messages {
            sysex::write(&mut saved, m, 1_048_576).unwrap();
        }
        assert_eq!(
            sysex::Reader::new(&saved[..], 1_048_576)
                .unwrap()
                .map(|m| m.unwrap().data().to_vec())
                .collect::<Vec<_>>(),
            replies
        );
    }
}
#[test]
fn manual_capture_preserves_repetitions_and_never_claims_completion() {
    for (model, prefix) in [
        ("novation/peak", vec![0, 0x20, 0x29]),
        ("elektron/digitakt", vec![0, 0x20, 0x3c]),
        ("moog/subsequent-37", vec![4]),
    ] {
        let clock = Clock::default();
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let mut bytes = prefix;
        bytes.extend([0x12, 0x34]);
        let mut receiver = Input {
            clock: clock.clone(),
            queue,
            scheduled: vec![item(vec![0x7d]), item(bytes.clone()), item(bytes.clone())],
        };
        let report = sysex::dump_with(
            &mut receiver,
            None::<&mut Output>,
            "s",
            model,
            &sysex::DumpOptions {
                silence: Duration::from_millis(10),
                ..Default::default()
            },
            &mut clock.clone(),
        )
        .unwrap();
        assert_eq!(report.status, "unverified");
        assert!(!report.complete);
        assert_eq!(report.requests_sent, 0);
        assert_eq!(report.ignored_events, 1);
        assert_eq!(
            report
                .messages
                .iter()
                .map(|m| m.data().to_vec())
                .collect::<Vec<_>>(),
            [bytes.clone(), bytes]
        );
        assert_eq!(clock.0.get(), 10_000_000);
    }
}
#[test]
fn corrupt_matching_reply_is_retained_and_limits_stop_admission() {
    let clock = Clock::default();
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let mut receiver = Input {
        clock: clock.clone(),
        queue: queue.clone(),
        scheduled: vec![],
    };
    let mut bad = hex("4300570018000000");
    bad.extend([0; 25]);
    let mut output = Output {
        queue,
        requests: [hex("432057000000")].into(),
        replies: [bad.clone()].into(),
        sent: 0,
    };
    let report = sysex::dump_with(
        &mut receiver,
        Some(&mut output),
        "s",
        "yamaha/vl70-m",
        &sysex::DumpOptions {
            scope: "system".into(),
            ..Default::default()
        },
        &mut clock.clone(),
    )
    .unwrap();
    assert_eq!(report.status, "failed");
    assert!(report.reason.unwrap().contains("checksum"));
    assert_eq!(report.messages[0].data(), bad);
    receiver.scheduled = vec![item(vec![4, 1]), item(vec![4, 1])];
    let report = sysex::dump_with(
        &mut receiver,
        None::<&mut Output>,
        "s",
        "moog/subsequent-37",
        &sysex::DumpOptions {
            message_limit: 1,
            ..Default::default()
        },
        &mut clock.clone(),
    )
    .unwrap();
    assert_eq!(report.status, "limit_reached");
    assert_eq!(report.messages.len(), 1);
}
