//! A fresh MIDI-CI discovery and endpoint scan over MIDI 1 or UMP SysEx7.
use crate::{
    ci::{BrowseReport, Browser},
    drivers::{self, Receiver, Sender},
    events::{ObservationValue, Payload, View},
    live::Item,
    messages::{AnyMessage, Message, PrimitiveMessage, SysEx},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    ump::{self, AssemblyItem, UmpMessage},
};
use std::{collections::VecDeque, time::Duration};

/// Controls shared with Python's keyword options; drivers own their clock.
#[derive(Clone)]
pub struct Options {
    pub cancel: Cancellation,
    pub message_limit: usize,
    pub byte_limit: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            cancel: Cancellation::default(),
            message_limit: 1_048_576,
            byte_limit: 8_388_608,
        }
    }
}

pub fn run(
    browser: &mut Browser,
    receiver: &mut midi::Input,
    output: &mut midi::Output,
    options: &Options,
) -> midi::Result<BrowseReport> {
    if browser.snapshot().status != "ready" {
        return Err(drivers::invalid("browser must be fresh"));
    }
    if browser.route().input() != receiver.stream()? {
        return Err(drivers::invalid(
            "browser route must match the receiver stream",
        ));
    }
    run_with(
        browser,
        receiver,
        output,
        &mut MonotonicClock::default(),
        options,
    )
}
fn run_with(
    browser: &mut Browser,
    receiver: &mut impl Receiver,
    output: &mut impl Sender,
    clock: &mut impl PlaybackClock,
    options: &Options,
) -> midi::Result<BrowseReport> {
    let route = browser.route().clone();
    let cancel = &options.cancel;
    let mut assembler = ump::EventAssembler::new(options.message_limit, options.byte_limit, 1)?;
    let mut queue = VecDeque::from(browser.start(clock.now_ns())?);
    let mut sending = None;
    let result = (|| -> midi::Result<()> {
        while matches!(
            browser.snapshot().status,
            "discovering" | "inquiring" | "stopping"
        ) {
            if cancel.cancelled() {
                browser.stop("cancelled by caller");
                break;
            }
            receiver.check()?;
            queue.extend(browser.poll(clock.now_ns())?);
            let pending = browser.snapshot().pending_tokens;
            queue.retain(|a| pending.contains(&a.token));
            if let Some(action) = queue.pop_front() {
                let wire = action.message.encode()?;
                let raw = SysEx::new(wire[1..wire.len() - 1].to_vec())?;
                let packets = if let Some(group) = route.group() {
                    ump::packetize_sysex7(group.into(), raw.data())?
                        .into_iter()
                        .map(|p| Payload::Ump(UmpMessage::SysEx7(p)))
                        .collect::<Vec<_>>()
                } else {
                    vec![Payload::Midi1(AnyMessage::SysEx(raw))]
                };
                for packet in packets {
                    receiver.check()?;
                    if cancel.cancelled() {
                        return Err(drivers::invalid("scan cancelled during management send"));
                    }
                    sending = Some(action.token);
                    output.send(packet)?;
                }
                queue.extend(browser.report_send(action.token, true, clock.now_ns())?);
                sending = None;
                continue;
            }
            if !matches!(browser.snapshot().status, "discovering" | "inquiring") {
                continue;
            }
            let Some(item) = receiver.receive(Duration::from_millis(50))? else {
                continue;
            };
            receiver.check()?;
            if cancel.cancelled() {
                browser.stop("cancelled by caller");
                break;
            }
            let message = match item {
                Item::Message(m) => m,
                Item::Diagnostic(d) => {
                    browser.stop(&d.reason);
                    break;
                }
            };
            if message.stream() != Some(route.input()) {
                browser.stop("received event from an unrelated stream");
                break;
            }
            match (&*message, route.group()) {
                (Payload::Midi1(AnyMessage::SysEx(raw)), None) => {
                    queue.extend(browser.feed(raw, route.input(), None, clock.now_ns())?)
                }
                (Payload::Ump(UmpMessage::SysEx7(packet)), Some(group))
                    if packet.group() == group =>
                {
                    for observation in assembler.feed(&message, None)? {
                        match observation.value {
                            ObservationValue::Ump(AssemblyItem::Data(data)) => {
                                queue.extend(browser.feed(
                                    &SysEx::new(data.data)?,
                                    route.input(),
                                    Some(group),
                                    clock.now_ns(),
                                )?)
                            }
                            _ => {
                                browser.stop("UMP SysEx7 assembly diagnostic");
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();
    if let Some(token) = sending {
        if browser.snapshot().pending_tokens.contains(&token) {
            browser.report_send(token, false, clock.now_ns())?;
        }
    }
    assembler.reset();
    if let Err(error) = result {
        browser.stop(&error.to_string());
        return Err(error);
    }
    Ok(browser.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ci::{Identity, Route, Session},
        drivers::tests::{Clock, Input, Output},
    };
    fn browser(group: Option<u8>) -> Browser {
        let identity = Identity::new(vec![125, 0, 0], 1, 2, vec![0; 4]).unwrap();
        let session = Session::new(
            1,
            identity,
            Route::new("s".into(), "out".into(), group.map(i64::from)).unwrap(),
            0,
            None,
            1024,
            2,
            4,
            4096,
        )
        .unwrap();
        Browser::new(session, 3_000_000_000, 100, 4_000_000_000, 16).unwrap()
    }
    #[test]
    fn options_reject_invalid_limits_and_cancel_without_sending() {
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [].into(),
            lost: false,
        };
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        let options = Options {
            message_limit: 0,
            ..Default::default()
        };
        assert!(run_with(
            &mut browser(Some(1)),
            &mut input,
            &mut output,
            &mut clock,
            &options
        )
        .is_err());
        let options = Options::default();
        options.cancel.cancel();
        let report = run_with(
            &mut browser(None),
            &mut input,
            &mut output,
            &mut clock,
            &options,
        )
        .unwrap();
        assert_eq!(report.status, "stopped");
        assert!(output.sent.is_empty());
    }

    #[test]
    fn scan_packetizes_ump_and_marks_partial_action_uncertain() {
        for group in [None, Some(1)] {
            let mut clock = Clock::default();
            let mut input = Input {
                clock: clock.clone(),
                items: [].into(),
                lost: false,
            };
            let mut output = Output {
                clock: clock.clone(),
                sent: vec![],
                fail_at: None,
            };
            let report = run_with(
                &mut browser(group),
                &mut input,
                &mut output,
                &mut clock,
                &Options::default(),
            )
            .unwrap();
            assert_eq!(report.status, "completed");
            assert_eq!(report.confirmed_actions, 1);
            assert!(output.sent.iter().all(|(_, m)| matches!(
                (group, m),
                (None, Payload::Midi1(_)) | (Some(_), Payload::Ump(_))
            )));
        }
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [].into(),
            lost: false,
        };
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: Some(1),
        };
        let mut browser = browser(Some(1));
        assert!(run_with(
            &mut browser,
            &mut input,
            &mut output,
            &mut clock,
            &Options::default()
        )
        .is_err());
        assert_eq!(browser.snapshot().status, "stopped");
        assert!(browser.snapshot().uncertain_token.is_some());
    }
}
