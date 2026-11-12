//! Bounded universal identity inquiry. Replies stay opaque and are never deduplicated.
use super::{is_identity_reply, IdentityRequest};
use crate::drivers::{self, Receiver, Sender};
use crate::events::Payload;
use crate::live::Item;
use crate::messages::{AnyMessage, Message, PrimitiveMessage, SysEx};
use crate::midi;
use crate::playback::{Cancellation, MonotonicClock, PlaybackClock};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct IdentityReport {
    pub messages: Vec<SysEx>,
    pub status: &'static str,
    pub uncertain_send: bool,
    pub ignored_events: usize,
    pub reason: Option<String>,
}
#[derive(Clone)]
pub struct IdentityOptions {
    pub device_id: u8,
    pub timeout: Duration,
    pub message_limit: usize,
    pub byte_limit: usize,
    pub payload_limit: usize,
    pub cancel: Cancellation,
}
impl Default for IdentityOptions {
    fn default() -> Self {
        Self {
            device_id: 127,
            timeout: Duration::from_secs(1),
            message_limit: 4096,
            byte_limit: 8_388_608,
            payload_limit: 1_048_576,
            cancel: Cancellation::default(),
        }
    }
}
/// Send one identity request, then collect all raw replies through the timeout.
/// Connections remain open. Buffered input is discarded before the request.
pub fn identify(
    receiver: &mut midi::Input,
    output: &mut midi::Output,
    options: &IdentityOptions,
) -> midi::Result<IdentityReport> {
    validate(options)?;
    let stream = receiver.stream()?.to_owned();
    identify_with(
        receiver,
        output,
        &stream,
        options,
        &mut MonotonicClock::default(),
    )
}
#[doc(hidden)]
pub fn identify_with(
    receiver: &mut impl Receiver,
    output: &mut impl Sender,
    stream: &str,
    options: &IdentityOptions,
    clock: &mut impl PlaybackClock,
) -> midi::Result<IdentityReport> {
    validate(options)?;
    if stream.is_empty() {
        return Err(drivers::invalid("identity input stream must not be empty"));
    }
    let deadline = clock
        .now_ns()
        .checked_add(options.timeout.as_nanos() as u64)
        .ok_or_else(|| drivers::invalid("identity deadline overflow"))?;
    let mut report = IdentityReport {
        messages: vec![],
        status: "completed",
        uncertain_send: false,
        ignored_events: 0,
        reason: None,
    };
    let baseline = receiver.dropped_packets()?;
    loop {
        if report.check(receiver, options, baseline) {
            return Ok(report);
        }
        if clock.now_ns() >= deadline {
            report.end("timed_out", "identity deadline expired before request");
            return Ok(report);
        }
        match receiver.receive(Duration::ZERO) {
            Ok(Some(_)) => {
                report.ignored_events += 1;
                if report.ignored_events >= options.message_limit {
                    report.end(
                        "limit_reached",
                        "input did not become quiet before identity request",
                    );
                    return Ok(report);
                }
            }
            Ok(None) => break,
            Err(e) => {
                report.error(&e);
                return Ok(report);
            }
        }
    }
    let wire = IdentityRequest::new(options.device_id.into())?.encode()?;
    report.uncertain_send = true;
    if let Err(e) = output.send(Payload::Message(AnyMessage::SysEx(SysEx::new(
        wire[1..wire.len() - 1].to_vec(),
    )?))) {
        report.error(&e);
        return Ok(report);
    }
    report.uncertain_send = false;
    let mut seen = 0;
    let mut bytes = 0;
    while clock.now_ns() < deadline {
        if report.check(receiver, options, baseline) {
            return Ok(report);
        }
        let item = match receiver.receive(drivers::remaining(clock, deadline)) {
            Ok(Some(i)) => i,
            Ok(None) => continue,
            Err(e) => {
                report.error(&e);
                return Ok(report);
            }
        };
        seen += 1;
        if seen > options.message_limit {
            report.end(
                "limit_reached",
                "identity reply collection exceeded its event limit",
            );
            return Ok(report);
        }
        match item {
            Item::Diagnostic(d) if d.stream == stream => {
                report.end("failed", &d.reason);
                return Ok(report);
            }
            Item::Message(m) if m.stream() == Some(stream) => {
                if let Payload::Message(AnyMessage::SysEx(reply)) = m.as_ref() {
                    if is_identity_reply(reply.data())
                        && (options.device_id == 127 || reply.data()[1] == options.device_id)
                    {
                        let size = reply.data().len() + 2;
                        if reply.data().len() > options.payload_limit
                            || size > options.byte_limit.saturating_sub(bytes)
                        {
                            report.end(
                                "limit_reached",
                                "identity reply exceeds byte or payload limit",
                            );
                            return Ok(report);
                        }
                        bytes += size;
                        report.messages.push(reply.clone());
                        continue;
                    }
                }
                report.ignored_events += 1;
            }
            _ => report.ignored_events += 1,
        }
    }
    if !report.check(receiver, options, baseline) && report.messages.is_empty() {
        report.end("timed_out", "no identity reply within timeout");
    }
    Ok(report)
}
impl IdentityReport {
    fn end(&mut self, status: &'static str, reason: &str) {
        self.status = status;
        self.reason = Some(reason.into());
    }
    fn error(&mut self, error: &midi::Error) {
        self.end(
            if drivers::interrupted(error) {
                "cancelled"
            } else {
                "failed"
            },
            &error.to_string(),
        );
    }
    fn check(
        &mut self,
        receiver: &impl Receiver,
        options: &IdentityOptions,
        baseline: u64,
    ) -> bool {
        if options.cancel.cancelled() {
            self.end("cancelled", "cancelled by caller");
            return true;
        }
        if let Err(e) = receiver.check_monitor() {
            self.error(&e);
            return true;
        }
        match receiver.dropped_packets() {
            Ok(n) if n == baseline => false,
            Ok(_) => {
                self.end("failed", "input packet loss during identity inquiry");
                true
            }
            Err(e) => {
                self.error(&e);
                true
            }
        }
    }
}
fn validate(options: &IdentityOptions) -> midi::Result<()> {
    if options.device_id > 127
        || options.timeout.is_zero()
        || options.timeout.as_nanos() > u64::MAX.into()
        || options.message_limit == 0
        || options.byte_limit == 0
        || options.payload_limit == 0
    {
        return Err(drivers::invalid(
            "identity address must be 0..127 and durations/limits must be positive and bounded",
        ));
    }
    Ok(())
}
