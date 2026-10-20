//! Bounded MIDI 1 SysEx transfers. The caller owns time and transport handoff.
use crate::{messages::SysEx, syx};
use std::io::{self, Read};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Plan {
    messages: Arc<[SysEx]>,
    wire_bytes: usize,
    gap_ns: u64,
}
impl Plan {
    pub fn new(
        messages: impl IntoIterator<Item = io::Result<SysEx>>,
        gap_ns: u64,
        message_limit: usize,
        byte_limit: usize,
    ) -> io::Result<Self> {
        if message_limit == 0 || byte_limit == 0 {
            return Err(invalid("transfer limits must be positive"));
        }
        let mut values = Vec::new();
        let mut wire_bytes = 0usize;
        for message in messages {
            let message = message?;
            let size = message
                .data()
                .len()
                .checked_add(2)
                .ok_or_else(|| invalid("transfer byte limit"))?;
            wire_bytes = wire_bytes
                .checked_add(size)
                .filter(|v| *v <= byte_limit)
                .ok_or_else(|| invalid("transfer byte limit"))?;
            if values.len() == message_limit {
                return Err(invalid("transfer message limit"));
            }
            values.push(message);
        }
        if values.is_empty() {
            return Err(invalid("transfer requires at least one SysEx message"));
        }
        Ok(Self {
            messages: values.into(),
            wire_bytes,
            gap_ns,
        })
    }
    pub fn read(
        source: impl Read,
        gap_ns: u64,
        message_limit: usize,
        byte_limit: usize,
        payload_limit: usize,
    ) -> io::Result<Self> {
        Self::new(
            syx::Reader::new(source, payload_limit)?,
            gap_ns,
            message_limit,
            byte_limit,
        )
    }
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
    pub fn wire_bytes(&self) -> usize {
        self.wire_bytes
    }
    pub fn gap_ns(&self) -> u64 {
        self.gap_ns
    }
    pub fn message(&self, index: usize) -> Option<&SysEx> {
        self.messages.get(index)
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    pub index: usize,
    pub message: SysEx,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub status: &'static str,
    pub total_messages: usize,
    pub total_bytes: usize,
    pub confirmed_messages: usize,
    pub confirmed_bytes: usize,
    pub remaining_messages: usize,
    pub in_flight: Option<usize>,
    pub uncertain_message: Option<usize>,
    pub next_ready_ns: Option<u64>,
    pub reason: Option<String>,
}

pub struct Session {
    plan: Plan,
    confirmed: usize,
    confirmed_bytes: usize,
    in_flight: Option<usize>,
    uncertain: Option<usize>,
    next_ready: Option<u64>,
    last_time: Option<u64>,
    status: &'static str,
    reason: Option<String>,
}
impl Session {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            confirmed: 0,
            confirmed_bytes: 0,
            in_flight: None,
            uncertain: None,
            next_ready: None,
            last_time: None,
            status: "active",
            reason: None,
        }
    }
    /// Reserves one message for a single send attempt; never returns it twice.
    pub fn prepare(&mut self, now_ns: u64) -> io::Result<Option<Step>> {
        self.active()?;
        if self.in_flight.is_some() {
            return Err(invalid("a transfer send is already in flight"));
        }
        self.time(now_ns)?;
        if self.next_ready.is_some_and(|v| now_ns < v) {
            return Ok(None);
        }
        let index = self.confirmed;
        self.in_flight = Some(index);
        self.next_ready = None;
        Ok(Some(Step {
            index,
            message: self.plan.messages[index].clone(),
        }))
    }
    /// Records transport handoff, not a device acknowledgement.
    pub fn confirm(&mut self, index: usize, now_ns: u64) -> io::Result<()> {
        self.pending(index)?;
        self.time(now_ns)?;
        let done = self.confirmed + 1 == self.plan.message_count();
        self.confirmed_bytes += self.plan.messages[index].data().len() + 2;
        self.confirmed += 1;
        self.in_flight = None;
        if done {
            self.status = "completed";
        } else if let Some(next) = now_ns.checked_add(self.plan.gap_ns) {
            self.next_ready = Some(next);
        } else {
            self.finish("failed", "transfer deadline exceeds u64 nanoseconds");
        }
        Ok(())
    }
    pub fn fail(&mut self, index: usize, reason: &str) -> io::Result<()> {
        self.pending(index)?;
        self.uncertain = self.in_flight.take();
        self.finish("failed", reason);
        Ok(())
    }
    /// Cancellation during a reserved send conservatively marks it uncertain.
    pub fn cancel(&mut self, reason: &str) {
        if self.status == "active" {
            self.uncertain = self.in_flight.take();
            self.finish("cancelled", reason);
        }
    }
    pub fn snapshot(&self) -> Report {
        Report {
            status: self.status,
            total_messages: self.plan.message_count(),
            total_bytes: self.plan.wire_bytes,
            confirmed_messages: self.confirmed,
            confirmed_bytes: self.confirmed_bytes,
            remaining_messages: self.plan.message_count() - self.confirmed,
            in_flight: self.in_flight,
            uncertain_message: self.uncertain,
            next_ready_ns: self.next_ready,
            reason: self.reason.clone(),
        }
    }
    fn active(&self) -> io::Result<()> {
        if self.status != "active" {
            return Err(invalid("transfer session is terminal"));
        }
        Ok(())
    }
    fn pending(&self, index: usize) -> io::Result<()> {
        self.active()?;
        if self.in_flight != Some(index) {
            return Err(invalid("transfer index does not match the in-flight send"));
        }
        Ok(())
    }
    fn time(&mut self, now: u64) -> io::Result<()> {
        if self.last_time.is_some_and(|v| now < v) {
            return Err(invalid("transfer clock regressed"));
        }
        self.last_time = Some(now);
        Ok(())
    }
    fn finish(&mut self, status: &'static str, reason: &str) {
        self.status = status;
        self.reason = Some(reason.chars().take(1024).collect());
        self.next_ready = None;
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
