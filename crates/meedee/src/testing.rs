//! Deterministic, caller-driven transport doubles. No devices, threads or sleeps.
use crate::events::{Clock, Payload};
use crate::live::{Decoder, Item};
use crate::messages::PrimitiveMessage;
use crate::transport::{validate_packet, Error, Packet, Protocol, Result};
use crate::ump::UmpValue;
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct ManualClock {
    now_ns: u64,
}
impl ManualClock {
    pub fn new(now_ns: u64) -> Self {
        Self { now_ns }
    }
    pub fn now_ns(&self) -> u64 {
        self.now_ns
    }
    pub fn advance(&mut self, nanoseconds: u64) -> Result<()> {
        self.now_ns = self
            .now_ns
            .checked_add(nanoseconds)
            .ok_or(Error::Invalid("manual clock overflow"))?;
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct InputReport {
    pub next_sequence: u64,
    pub dropped_packets: u64,
    pub dropped_events: u64,
    pub queued_packets: usize,
    pub queued_events: usize,
    pub queued_bytes: usize,
    pub closed: bool,
    pub failure: Option<String>,
}
struct QueuedPacket {
    packet: Packet,
    clock: Clock,
    timestamp_ns: Option<i64>,
    sequence: u64,
    prior_drops: u64,
}
pub struct Input {
    stream: String,
    protocol: Protocol,
    decoder: Decoder,
    packets: VecDeque<QueuedPacket>,
    items: VecDeque<Item>,
    packet_limit: usize,
    event_limit: usize,
    byte_limit: usize,
    bytes: usize,
    sequence: u64,
    dropped: u64,
    reported_drops: u64,
    dropped_events: u64,
    pending_event_loss: u64,
    closed: bool,
    failure: Option<String>,
}
impl Input {
    pub fn new(
        stream: String,
        protocol: Protocol,
        packet_limit: usize,
        event_limit: usize,
        byte_limit: usize,
        sysex_limit: usize,
    ) -> Result<Self> {
        if stream.is_empty()
            || packet_limit == 0
            || event_limit == 0
            || byte_limit == 0
            || sysex_limit == 0
        {
            return Err(Error::Invalid(
                "testing input requires a nonempty stream and positive limits",
            ));
        }
        let decoder = Decoder::new(stream.clone(), protocol, sysex_limit)
            .map_err(|e| Error::Native(e.to_string()))?;
        Ok(Self {
            stream,
            protocol,
            decoder,
            packets: VecDeque::new(),
            items: VecDeque::new(),
            packet_limit,
            event_limit,
            byte_limit,
            bytes: 0,
            sequence: 0,
            dropped: 0,
            reported_drops: 0,
            dropped_events: 0,
            pending_event_loss: 0,
            closed: false,
            failure: None,
        })
    }
    pub fn stream(&self) -> &str {
        &self.stream
    }
    /// Raw packets may contain fragments or malformed content; their protocol must match.
    pub fn inject(
        &mut self,
        packet: Packet,
        clock: Clock,
        timestamp_ns: Option<i64>,
    ) -> Result<bool> {
        self.available()?;
        if !matches!(
            (self.protocol, &packet),
            (Protocol::Midi1, Packet::Bytes(_)) | (Protocol::Ump, Packet::Ump(_))
        ) {
            return Err(Error::Invalid("injected packet protocol mismatch"));
        }
        if clock.numerator() != 1
            || clock.denominator() != 1_000_000_000
            || clock.mode() == Some(false) && timestamp_ns.is_some()
        {
            return Err(Error::Invalid(
                "injection requires a nanosecond clock and consistent timestamp mode",
            ));
        }
        let next = self
            .sequence
            .checked_add(1)
            .ok_or(Error::Invalid("packet sequence exhausted"))?;
        let sequence = self.sequence;
        self.sequence = next;
        let size = packet_size(&packet);
        if self.packets.len() == self.packet_limit || size > self.byte_limit - self.bytes {
            self.dropped += 1;
            return Ok(false);
        }
        self.bytes += size;
        self.packets.push_back(QueuedPacket {
            packet,
            clock,
            timestamp_ns,
            sequence,
            prior_drops: self.dropped,
        });
        Ok(true)
    }
    /// Simulate missing raw packets even when no later packet arrives.
    pub fn lose(&mut self, count: u64) -> Result<()> {
        self.available()?;
        if count == 0 {
            return Err(Error::Invalid("loss count must be positive"));
        }
        let next = self
            .sequence
            .checked_add(count)
            .ok_or(Error::Invalid("packet sequence exhausted"))?;
        self.dropped = self
            .dropped
            .checked_add(count)
            .ok_or(Error::Invalid("loss counter exhausted"))?;
        self.sequence = next;
        Ok(())
    }
    pub fn receive(&mut self) -> Result<Option<Item>> {
        if self.closed {
            return Err(Error::Closed);
        }
        loop {
            if let Some(item) = self.items.pop_front() {
                self.bytes -= item_size(&item);
                return Ok(Some(item));
            }
            if self.pending_event_loss != 0 {
                let count = std::mem::take(&mut self.pending_event_loss);
                return Ok(Some(self.decoder.loss(
                    0,
                    format!("simulated decoded queue overflow; {count} items dropped"),
                )));
            }
            if let Some(packet) = self.packets.pop_front() {
                self.bytes -= packet_size(&packet.packet);
                self.reported_drops = packet.prior_drops;
                let items = self
                    .decoder
                    .feed(
                        &packet.packet,
                        &packet.clock,
                        packet.timestamp_ns,
                        Some(packet.sequence),
                    )
                    .map_err(|e| Error::Native(e.to_string()))?;
                let mut dropping = false;
                for item in items {
                    let size = item_size(&item);
                    if dropping
                        || self.items.len() == self.event_limit
                        || size > self.byte_limit - self.bytes
                    {
                        dropping = true;
                        self.dropped_events += 1;
                        self.pending_event_loss += 1;
                    } else {
                        self.bytes += size;
                        self.items.push_back(item);
                    }
                }
                continue;
            }
            if self.reported_drops < self.dropped {
                let lost = self.dropped - self.reported_drops;
                self.reported_drops = self.dropped;
                let item = self.decoder.loss(lost, "simulated input loss".into());
                self.decoder.begin_at(self.sequence);
                return Ok(Some(item));
            }
            if let Some(reason) = &self.failure {
                return Err(Error::Worker(reason.clone()));
            }
            return Ok(None);
        }
    }
    pub fn fail(&mut self, reason: &str) {
        if !self.closed && self.failure.is_none() {
            self.failure = Some(reason.chars().take(1024).collect());
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.packets.clear();
        self.items.clear();
        self.bytes = 0;
        self.pending_event_loss = 0;
        self.decoder.reset();
    }
    pub fn snapshot(&self) -> InputReport {
        InputReport {
            next_sequence: self.sequence,
            dropped_packets: self.dropped,
            dropped_events: self.dropped_events,
            queued_packets: self.packets.len(),
            queued_events: self.items.len(),
            queued_bytes: self.bytes,
            closed: self.closed,
            failure: self.failure.clone(),
        }
    }
    fn available(&self) -> Result<()> {
        if self.closed {
            return Err(Error::Closed);
        }
        if let Some(reason) = &self.failure {
            return Err(Error::Worker(reason.clone()));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct OutputReport {
    pub attempts: u64,
    pub accepted: u64,
    pub queued_packets: usize,
    pub queued_bytes: usize,
    pub closed: bool,
    pub failure: Option<String>,
}
pub struct Output {
    protocol: Protocol,
    packets: VecDeque<Packet>,
    packet_limit: usize,
    byte_limit: usize,
    bytes: usize,
    attempts: u64,
    accepted: u64,
    fault: Option<(bool, String)>,
    failure: Option<String>,
    closed: bool,
}
impl Output {
    pub fn new(protocol: Protocol, packet_limit: usize, byte_limit: usize) -> Result<Self> {
        if packet_limit == 0 || byte_limit == 0 {
            return Err(Error::Invalid("testing output limits must be positive"));
        }
        Ok(Self {
            protocol,
            packets: VecDeque::new(),
            packet_limit,
            byte_limit,
            bytes: 0,
            attempts: 0,
            accepted: 0,
            fault: None,
            failure: None,
            closed: false,
        })
    }
    pub fn send(&mut self, packet: Packet) -> Result<()> {
        if self.closed {
            return Err(Error::Closed);
        }
        if let Some(reason) = &self.failure {
            return Err(Error::Native(reason.clone()));
        }
        validate_packet(self.protocol, &packet)?;
        self.attempts = self
            .attempts
            .checked_add(1)
            .ok_or(Error::Invalid("send attempt counter exhausted"))?;
        let size = packet_size(&packet);
        if self.packets.len() == self.packet_limit || size > self.byte_limit - self.bytes {
            return Err(Error::Resource(
                "simulated output recording capacity exhausted; packet not accepted",
            ));
        }
        let fault = self.fault.take();
        if fault.as_ref().is_none_or(|(accepted, _)| *accepted) {
            self.bytes += size;
            self.accepted += 1;
            self.packets.push_back(packet);
        }
        if let Some((_, reason)) = fault {
            self.failure = Some(reason.clone());
            return Err(Error::Native(reason));
        }
        Ok(())
    }
    pub fn fail_next_send(&mut self, accepted: bool, reason: &str) -> Result<()> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.failure.is_some() || self.fault.is_some() {
            return Err(Error::Invalid(
                "output already failed or has an armed fault",
            ));
        }
        self.fault = Some((accepted, reason.chars().take(1024).collect()));
        Ok(())
    }
    pub fn take_packet(&mut self) -> Option<Packet> {
        let packet = self.packets.pop_front()?;
        self.bytes -= packet_size(&packet);
        Some(packet)
    }
    /// Closing does not erase evidence already accepted by the simulated sink.
    pub fn close(&mut self) {
        self.closed = true;
        self.fault = None;
    }
    pub fn snapshot(&self) -> OutputReport {
        OutputReport {
            attempts: self.attempts,
            accepted: self.accepted,
            queued_packets: self.packets.len(),
            queued_bytes: self.bytes,
            closed: self.closed,
            failure: self.failure.clone(),
        }
    }
}
fn packet_size(packet: &Packet) -> usize {
    match packet {
        Packet::Bytes(v) => v.len(),
        Packet::Ump(v) => v.len() * 4,
    }
}
fn item_size(item: &Item) -> usize {
    match item {
        Item::Diagnostic(v) => v.reason.len(),
        Item::Event(e) => match &e.message {
            Payload::Midi1(v) => v.encode().len(),
            Payload::Ump(v) => v.encode_words().len() * 4,
            Payload::Command(_) => 64,
        },
    }
}
