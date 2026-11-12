//! Opaque dump capture. Only transport envelopes are interpreted.
use super::dump_models::{identify, recipe, Recipe};
use crate::archive::{Record, Recorder};
use crate::drivers::{self, Receiver, Sender};
use crate::events::Payload;
use crate::live::Item;
use crate::messages::{AnyMessage, Message, SysEx};
use crate::midi;
use crate::playback::{Cancellation, MonotonicClock, PlaybackClock};
use crate::universal::{self, is_identity_reply};
use std::collections::BTreeSet;
use std::time::Duration;

/// Opaque messages and transfer evidence. `complete` verifies only the documented
/// requested scope, never restorability or the identity of the physical sender.
#[derive(Clone, Debug)]
pub struct DumpReport {
    pub model: Option<String>,
    pub scope: String,
    pub messages: Vec<SysEx>,
    pub status: &'static str,
    pub complete: bool,
    pub requests_sent: usize,
    pub uncertain_send: bool,
    pub ignored_events: usize,
    pub wire_bytes: usize,
    pub reason: Option<String>,
}

#[derive(Clone)]
pub struct DumpOptions {
    pub scope: String,
    /// Wire device number; None uses the detected address or model's default.
    pub device_id: Option<u8>,
    pub timeout: Duration,
    pub silence: Duration,
    /// Full identity reply collection window, within the overall timeout.
    pub identity_timeout: Duration,
    pub message_limit: usize,
    pub byte_limit: usize,
    pub payload_limit: usize,
    pub cancel: Cancellation,
}
impl Default for DumpOptions {
    fn default() -> Self {
        Self {
            scope: "all".into(),
            device_id: None,
            timeout: Duration::from_secs(120),
            silence: Duration::from_secs(1),
            identity_timeout: Duration::from_secs(1),
            message_limit: 4096,
            byte_limit: 8_388_608,
            payload_limit: 1_048_576,
            cancel: Cancellation::default(),
        }
    }
}

/// Request a supported dump, or capture a front-panel dump when output is None.
/// With an output, model None requests identity discovery; manual capture needs
/// an explicit model. Multiple identity replies never select a model.
/// Connections remain open. Capture consumes input; do not concurrently receive
/// from the same connection. Inspect the report before exporting its messages.
pub fn dump(
    receiver: &mut midi::Input,
    output: Option<&mut midi::Output>,
    model: Option<&str>,
    options: &DumpOptions,
) -> midi::Result<DumpReport> {
    // Validate before opening a connection.
    if let Some(model) = model {
        recipe(model, &options.scope, options.device_id, output.is_some())?;
    } else if output.is_none() {
        return Err(drivers::invalid(
            "manual capture requires an explicit model",
        ));
    }
    validate(options)?;
    let stream = receiver.stream()?.to_owned();
    dump_with(
        receiver,
        output,
        &stream,
        model,
        options,
        &mut MonotonicClock::default(),
    )
}

#[doc(hidden)]
pub fn dump_with(
    receiver: &mut impl Receiver,
    mut output: Option<&mut impl Sender>,
    stream: &str,
    model: Option<&str>,
    options: &DumpOptions,
    clock: &mut impl PlaybackClock,
) -> midi::Result<DumpReport> {
    let selected = if let Some(model) = model {
        Some(recipe(
            model,
            &options.scope,
            options.device_id,
            output.is_some(),
        )?)
    } else {
        if output.is_none() {
            return Err(drivers::invalid(
                "manual capture requires an explicit model",
            ));
        }
        None
    };
    validate(options)?;
    if stream.is_empty() {
        return Err(drivers::invalid("dump input stream must not be empty"));
    }
    let deadline = clock
        .now_ns()
        .checked_add(nanos(options.timeout)?)
        .ok_or_else(|| drivers::invalid("dump deadline overflow"))?;
    let recorder = Recorder::memory(
        vec![Record::Stream {
            id: stream.into(),
            label: "SysEx dump".into(),
            clock_identity: None,
        }],
        false,
    )?;
    let mut capture = Capture {
        report: DumpReport {
            model: model.map(str::to_owned),
            scope: options.scope.clone(),
            messages: vec![],
            status: "timed_out",
            complete: false,
            requests_sent: 0,
            uncertain_send: false,
            ignored_events: 0,
            wire_bytes: 0,
            reason: Some("no matching response before deadline".into()),
        },
        recorder,
        clocks: BTreeSet::new(),
        count: 0,
        step: 0,
        last_match: None,
        autodetected: false,
    };
    let baseline = receiver.dropped_packets()?;
    // Discovery establishes its own boundary. Explicit selection and manual
    // capture drain here so stale traffic never enters the recorder.
    if selected.is_some() {
        loop {
            if capture.check(receiver, options, baseline, clock, deadline) {
                return capture.finish();
            }
            match receiver.receive(Duration::ZERO) {
                Ok(Some(_)) => {
                    capture.report.ignored_events += 1;
                    if capture.report.ignored_events >= options.message_limit {
                        capture.end("limit_reached", "input did not become quiet before capture");
                        return capture.finish();
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    capture.error(&error);
                    return capture.finish();
                }
            }
        }
    }
    let recipe = match selected {
        Some(recipe) => recipe,
        None => {
            let Some((model, device)) = capture.detect(
                receiver,
                output.as_deref_mut().expect("remote discovery"),
                stream,
                options,
                baseline,
                clock,
                deadline,
            )?
            else {
                return capture.finish();
            };
            if options.device_id.zip(device).is_some_and(|(a, b)| a != b) {
                capture.end("failed", "device_id differs from the identity reply; specify model explicitly to select another address");
                return capture.finish();
            }
            let selected = recipe(model, &options.scope, options.device_id.or(device), true)?;
            capture.report.model = Some(model.into());
            capture.autodetected = true;
            selected
        }
    };
    if let Some(sender) = output.as_deref_mut() {
        if !capture.send(sender, &recipe) {
            return capture.finish();
        }
    }
    loop {
        if capture.check(receiver, options, baseline, clock, deadline) {
            break;
        }
        let now = clock.now_ns();
        let idle_deadline = capture
            .last_match
            .map(|n: u64| n.saturating_add(nanos(options.silence).expect("validated silence")));
        if idle_deadline.is_some_and(|d| now >= d) {
            if output.is_none() {
                capture.end("unverified", "matching traffic ended in silence; model identity and dump completion are unverified");
            } else {
                capture.end(
                    "timed_out",
                    "response sequence ended before all requested replies arrived",
                );
            }
            break;
        }
        let wait_until = idle_deadline.unwrap_or(deadline).min(deadline);
        match receiver.receive(drivers::remaining(clock, wait_until)) {
            Ok(Some(item)) => {
                // Retain an admitted prefix even when a loss counter changes on
                // this receive; the next health check prevents false success.
                if !capture.feed(
                    item,
                    stream,
                    &recipe,
                    options,
                    clock.now_ns(),
                    output.is_some(),
                ) {
                    break;
                }
                if capture.check(receiver, options, baseline, clock, deadline) {
                    break;
                }
                if output.is_some() && capture.step == recipe.steps.len() {
                    capture.report.status = if recipe.verified {
                        "complete"
                    } else {
                        "unverified"
                    };
                    capture.report.complete = recipe.verified;
                    capture.report.reason = if recipe.verified {
                        None
                    } else {
                        Some("requested replies received; published payload lengths are inconsistent".into())
                    };
                    break;
                }
                if let Some(sender) = output.as_deref_mut() {
                    if capture.step > capture.report.requests_sent.saturating_sub(1)
                        && !capture.send(sender, &recipe)
                    {
                        break;
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                capture.error(&error);
                break;
            }
        }
    }
    capture.finish()
}

struct Capture {
    report: DumpReport,
    recorder: Recorder,
    clocks: BTreeSet<String>,
    count: usize,
    step: usize,
    last_match: Option<u64>,
    autodetected: bool,
}
impl Capture {
    #[allow(clippy::too_many_arguments)]
    fn detect(
        &mut self,
        receiver: &mut impl Receiver,
        output: &mut impl Sender,
        stream: &str,
        options: &DumpOptions,
        baseline: u64,
        clock: &mut impl PlaybackClock,
        deadline: u64,
    ) -> midi::Result<Option<(&'static str, Option<u8>)>> {
        if self.check(receiver, options, baseline, clock, deadline) {
            return Ok(None);
        }
        let inquiry = universal::identify_with(
            receiver,
            output,
            stream,
            &universal::IdentityOptions {
                timeout: options.identity_timeout.min(Duration::from_nanos(
                    deadline.saturating_sub(clock.now_ns()),
                )),
                message_limit: options.message_limit,
                byte_limit: options.byte_limit,
                payload_limit: options.payload_limit,
                cancel: options.cancel.clone(),
                ..Default::default()
            },
            clock,
        )?;
        self.report.ignored_events += inquiry.ignored_events;
        self.report.uncertain_send = inquiry.uncertain_send;
        if self.check(receiver, options, baseline, clock, deadline) {
            return Ok(None);
        }
        if inquiry.status != "completed" {
            self.end(
                if inquiry.status == "timed_out" {
                    "failed"
                } else {
                    inquiry.status
                },
                &format!(
                    "{}; specify model explicitly",
                    inquiry
                        .reason
                        .as_deref()
                        .unwrap_or("identity inquiry failed")
                ),
            );
            return Ok(None);
        }
        if inquiry.messages.len() != 1 {
            self.end("failed","multiple identity replies; specify model and device_id explicitly instead of guessing");
            return Ok(None);
        }
        match identify(&inquiry.messages[0]) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => {
                self.end(
                    "failed",
                    "unrecognized or unsupported identity; specify model explicitly",
                );
                Ok(None)
            }
            Err(reason) => {
                self.end("failed", reason);
                Ok(None)
            }
        }
    }
    fn end(&mut self, status: &'static str, reason: &str) {
        self.report.status = status;
        self.report.complete = false;
        self.report.reason = Some(reason.into());
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
        options: &DumpOptions,
        baseline: u64,
        clock: &mut impl PlaybackClock,
        deadline: u64,
    ) -> bool {
        if options.cancel.cancelled() {
            self.end("cancelled", "cancelled by caller");
            return true;
        }
        if let Err(error) = receiver.check_monitor() {
            self.error(&error);
            return true;
        }
        match receiver.dropped_packets() {
            Ok(n) if n == baseline => {}
            Ok(_) => {
                self.end("failed", "input packet loss during dump");
                return true;
            }
            Err(error) => {
                self.error(&error);
                return true;
            }
        }
        if clock.now_ns() >= deadline {
            self.end(
                "timed_out",
                if self.count == 0 {
                    "no matching response before deadline"
                } else {
                    "overall dump deadline expired"
                },
            );
            return true;
        }
        false
    }
    fn send(&mut self, output: &mut impl Sender, recipe: &Recipe) -> bool {
        self.report.uncertain_send = true;
        match output.send(Payload::Message(AnyMessage::SysEx(
            recipe.steps[self.step].request.clone(),
        ))) {
            Ok(()) => {
                self.report.requests_sent += 1;
                self.report.uncertain_send = false;
                true
            }
            Err(error) => {
                self.error(&error);
                false
            }
        }
    }
    fn feed(
        &mut self,
        item: Item,
        stream: &str,
        recipe: &Recipe,
        options: &DumpOptions,
        now: u64,
        remote: bool,
    ) -> bool {
        let message = match item {
            Item::Diagnostic(d) => {
                if d.stream == stream {
                    self.end("failed", &d.reason);
                    return false;
                }
                self.report.ignored_events += 1;
                return true;
            }
            Item::Message(m) => m,
        };
        let Payload::Message(AnyMessage::SysEx(sysex)) = message.as_ref() else {
            self.report.ignored_events += 1;
            return true;
        };
        if message.stream() != Some(stream) {
            self.report.ignored_events += 1;
            return true;
        }
        if self.autodetected && is_identity_reply(sysex.data()) {
            self.end("failed", "additional identity reply after discovery; multiple responders require explicit model selection");
            return false;
        }
        let matched = if remote {
            recipe.steps[self.step].matches(sysex.data())
        } else {
            recipe.manual_matches(sysex.data())
        };
        if !matched {
            self.report.ignored_events += 1;
            return true;
        }
        let size = sysex.data().len() + 2;
        if self.count >= options.message_limit
            || sysex.data().len() > options.payload_limit
            || size > options.byte_limit.saturating_sub(self.report.wire_bytes)
        {
            self.end("limit_reached", "dump message or byte limit exceeded");
            return false;
        }
        if let Some(timestamp) = message.timestamp() {
            if self.clocks.insert(timestamp.clock.identity().into()) {
                self.recorder.record(Record::Clock(timestamp.clock.clone()));
            }
        }
        let validation = if remote {
            recipe.steps[self.step].validate(sysex.data())
        } else {
            Ok(())
        };
        self.recorder.record(Record::Event(message));
        self.count += 1;
        self.report.wire_bytes += size;
        self.last_match = Some(now);
        if let Err(reason) = validation {
            self.end("failed", reason);
            return false;
        }
        if remote {
            self.step += 1;
        }
        true
    }
    fn finish(mut self) -> midi::Result<DumpReport> {
        self.recorder.stop(Duration::from_secs(5))?;
        self.report.messages = self
            .recorder
            .records()
            .expect("stopped memory recorder")
            .into_iter()
            .filter_map(|r| match r {
                Record::Event(m) => match *m {
                    Payload::Message(AnyMessage::SysEx(s)) => Some(s),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        Ok(self.report)
    }
}
fn nanos(duration: Duration) -> midi::Result<u64> {
    duration
        .as_nanos()
        .try_into()
        .map_err(|_| drivers::invalid("dump duration exceeds u64 nanoseconds"))
}
fn validate(options: &DumpOptions) -> midi::Result<()> {
    if options.timeout.is_zero()
        || options.identity_timeout.is_zero()
        || options.silence.is_zero()
        || options.message_limit == 0
        || options.byte_limit == 0
        || options.payload_limit == 0
    {
        return Err(drivers::invalid(
            "dump durations and limits must be positive",
        ));
    }
    nanos(options.timeout)?;
    nanos(options.identity_timeout)?;
    nanos(options.silence)?;
    Ok(())
}
