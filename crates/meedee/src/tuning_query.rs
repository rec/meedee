//! One unicast MTS dump request. Caller owns time, decoding and transport.
use crate::{
    events::Payload,
    live::Item,
    messages::{AnyMessage, PrimitiveMessage, SysEx, ValueError},
    tuning::{self, Decoded, ReplyTracker, RequestStatus, TuningMessage},
};
use std::io::{self, Write};

#[derive(Clone, Debug)]
pub struct Report {
    pub status: &'static str,
    pub request_sent: bool,
    pub uncertain_send: bool,
    pub ignored_events: u64,
    pub deadline_ns: Option<u64>,
    pub reply: Option<SysEx>,
    pub decoded: Option<Decoded>,
    pub reason: Option<String>,
}
pub struct Query {
    request: TuningMessage,
    stream: String,
    timeout_ns: u64,
    tracker: Option<ReplyTracker>,
    report: Report,
}
impl Query {
    pub fn new(
        request: TuningMessage,
        stream: String,
        timeout_ns: u64,
    ) -> Result<Self, ValueError> {
        if stream.is_empty() {
            return Err(invalid("query requires an input stream"));
        }
        ReplyTracker::new(&request, 0, timeout_ns)?;
        Ok(Self {
            request,
            stream,
            timeout_ns,
            tracker: None,
            report: Report {
                status: "ready",
                request_sent: false,
                uncertain_send: false,
                ignored_events: 0,
                deadline_ns: None,
                reply: None,
                decoded: None,
                reason: None,
            },
        })
    }
    pub fn prepare(&mut self, now_ns: u64) -> Result<SysEx, ValueError> {
        if self.report.status != "ready" {
            return Err(invalid("query must be fresh"));
        }
        let tracker = ReplyTracker::new(&self.request, now_ns, self.timeout_ns)?;
        self.report.deadline_ns = Some(tracker.deadline_ns());
        self.tracker = Some(tracker);
        self.report.status = "sending";
        let wire = self.request.encode();
        SysEx::new(wire[1..wire.len() - 1].to_vec())
    }
    pub fn confirm(&mut self, now_ns: u64) -> Result<(), ValueError> {
        if self.report.status != "sending" {
            return Err(invalid("no query send is pending"));
        }
        // Validate time before recording the known whole-message handoff.
        let status = self.tracker.as_mut().expect("started query").poll(now_ns)?;
        self.report.request_sent = true;
        self.report.status = "waiting";
        if status == RequestStatus::TimedOut {
            self.finish("timed_out", "query deadline expired during send");
        }
        Ok(())
    }
    pub fn poll(&mut self, now_ns: u64) -> Result<(), ValueError> {
        if !matches!(self.report.status, "sending" | "waiting") {
            return Err(invalid("query is not active"));
        }
        if self.tracker.as_mut().expect("started query").poll(now_ns)? == RequestStatus::TimedOut {
            self.finish("timed_out", "query deadline expired");
        }
        Ok(())
    }
    pub fn feed(&mut self, item: &Item, now_ns: u64) -> Result<(), ValueError> {
        if self.report.status != "waiting" {
            return Err(invalid("query must have a confirmed send before receiving"));
        }
        self.poll(now_ns)?;
        if self.report.status != "waiting" {
            return Ok(());
        }
        let event = match item {
            Item::Diagnostic(d) => {
                self.fail(&format!("input diagnostic {:?}: {}", d.kind, d.reason));
                return Ok(());
            }
            Item::Event(e) => e,
        };
        if event.stream != self.stream {
            self.fail("query received an unrelated source");
            return Ok(());
        }
        match &event.message {
            Payload::Midi1(AnyMessage::SysEx(raw)) => {
                let decoded = match tuning::decode(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        self.fail(&e.to_string());
                        return Ok(());
                    }
                };
                if let Some(decoded) = decoded {
                    if self
                        .tracker
                        .as_mut()
                        .expect("started query")
                        .observe(&decoded.message, now_ns)?
                    {
                        self.report.reply = Some(raw.clone());
                        self.report.decoded = Some(decoded);
                        self.report.status = "completed";
                        return Ok(());
                    }
                }
            }
            Payload::Midi1(_) => {}
            _ => {
                self.fail("query requires MIDI 1 byte events");
                return Ok(());
            }
        }
        self.report.ignored_events = self
            .report
            .ignored_events
            .checked_add(1)
            .ok_or_else(|| invalid("query ignored-event count exhausted"))?;
        Ok(())
    }
    pub fn fail(&mut self, reason: &str) {
        if matches!(self.report.status, "ready" | "sending" | "waiting") {
            self.finish("failed", reason);
        }
    }
    pub fn cancel(&mut self, reason: &str) {
        if matches!(self.report.status, "ready" | "sending" | "waiting") {
            self.finish("cancelled", reason);
        }
    }
    pub fn snapshot(&self) -> Report {
        self.report.clone()
    }
    pub fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        let reply = self
            .report
            .reply
            .as_ref()
            .filter(|_| self.report.status == "completed")
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only a completed query can be exported",
                )
            })?;
        crate::syx::write(writer, reply, reply.data().len())?;
        writer.flush()
    }
    fn finish(&mut self, status: &'static str, reason: &str) {
        self.report.uncertain_send = self.report.status == "sending";
        self.report.status = status;
        self.report.reason = Some(reason.chars().take(1024).collect());
    }
}
fn invalid(reason: &str) -> ValueError {
    ValueError(reason.into())
}
