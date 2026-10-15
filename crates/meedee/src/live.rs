//! Source-local typed decoding with explicit packet loss and completion-time spans.
use crate::{
    codecs,
    events::{Clock, Event, Payload, Span, Timestamp},
    file_io::invalid,
    transport::{Packet, Protocol},
    ump,
};
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    Loss,
    Malformed,
    ClockChange,
    Truncated,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub stream: String,
    pub packet: u64,
    pub kind: DiagnosticKind,
    pub lost: u64,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Event(Box<Event>),
    Diagnostic(Diagnostic),
}
#[derive(Clone)]
struct Marker {
    index: u64,
    time: Option<Timestamp>,
}
pub struct Decoder {
    stream: String,
    protocol: Protocol,
    bytes: codecs::Decoder,
    words: ump::Decoder,
    pending: Option<Marker>,
    expected: u64,
    clock: Option<Clock>,
    stopped: bool,
}
impl Decoder {
    pub fn new(stream: String, protocol: Protocol, limit: usize) -> io::Result<Self> {
        if stream.is_empty() {
            return Err(invalid(0, "empty source identity"));
        }
        Ok(Self {
            stream,
            protocol,
            bytes: codecs::Decoder::new(limit).map_err(|e| invalid(0, e))?,
            words: ump::Decoder::new(false),
            pending: None,
            expected: 0,
            clock: None,
            stopped: false,
        })
    }
    fn diagnostic(&self, kind: DiagnosticKind, lost: u64, reason: String, packet: u64) -> Item {
        Item::Diagnostic(Diagnostic {
            stream: self.stream.clone(),
            packet,
            kind,
            lost,
            reason,
        })
    }
    pub(crate) fn begin_at(&mut self, index: u64) {
        self.expected = index;
    }
    pub fn pending(&self) -> bool {
        self.bytes.pending_start().is_some() || self.words.pending_start().is_some()
    }
    pub fn reset(&mut self) {
        self.bytes.reset();
        self.words.reset();
        self.pending = None;
    }
    pub fn loss(&mut self, lost: u64, reason: String) -> Item {
        self.reset();
        self.diagnostic(DiagnosticKind::Loss, lost, reason, self.expected)
    }
    pub fn feed(
        &mut self,
        packet: &Packet,
        clock: &Clock,
        timestamp: Option<i64>,
        sequence: Option<u64>,
    ) -> io::Result<Vec<Item>> {
        if self.stopped {
            return Err(invalid(0, "decoder is finished"));
        }
        let index = sequence.unwrap_or(self.expected);
        let mut out = vec![];
        if index != self.expected {
            let lost = index.saturating_sub(self.expected);
            self.reset();
            out.push(
                self.diagnostic(
                    DiagnosticKind::Loss,
                    lost,
                    if index < self.expected {
                        "packet order regressed"
                    } else {
                        "packet sequence gap"
                    }
                    .into(),
                    index,
                ),
            );
        }
        self.expected = index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "packet sequence exhausted"))?;
        if self.clock.as_ref().is_some_and(|c| c != clock) {
            self.reset();
            out.push(self.diagnostic(
                DiagnosticKind::ClockChange,
                0,
                "clock descriptor/generation changed".into(),
                index,
            ));
        }
        self.clock = Some(clock.clone());
        let time = timestamp.map(|ticks| Timestamp {
            ticks,
            clock: clock.clone(),
        });
        let current = Marker {
            index,
            time: time.clone(),
        };
        let previous = self.pending.take();
        let (mut decoded, offset, pending) = match (self.protocol, packet) {
            (Protocol::Midi1, Packet::Bytes(b)) => {
                let offset = self.bytes.offset();
                let values: Vec<_> = self
                    .bytes
                    .feed_spanned(b)
                    .into_iter()
                    .map(|(v, start, _)| {
                        (
                            match v {
                                codecs::DecodeItem::Message(v) => Ok(Payload::Midi1(v)),
                                codecs::DecodeItem::Diagnostic(d) => {
                                    Err(format!("{:?} at byte {}", d.kind, d.offset))
                                }
                            },
                            start,
                        )
                    })
                    .collect();
                (values, offset, self.bytes.pending_start())
            }
            (Protocol::Ump, Packet::Ump(w)) => {
                let offset = self.words.offset();
                let values: Vec<_> = self
                    .words
                    .feed_spanned(w)
                    .into_iter()
                    .map(|(v, start, _)| {
                        (
                            match v {
                                ump::DecodeOutput::Message(v) => Ok(Payload::Ump(v)),
                                ump::DecodeOutput::Diagnostic(d) => {
                                    Err(format!("{} at word {}", d.reason, d.word_offset))
                                }
                            },
                            start,
                        )
                    })
                    .collect();
                (values, offset, self.words.pending_start())
            }
            _ => {
                self.reset();
                out.push(self.diagnostic(
                    DiagnosticKind::Malformed,
                    0,
                    "packet protocol mismatch".into(),
                    index,
                ));
                return Ok(out);
            }
        };
        for (value, start) in decoded.drain(..) {
            match value {
                Ok(message) => {
                    let first = if start < offset {
                        previous.as_ref().unwrap_or(&current)
                    } else {
                        &current
                    };
                    let event = Event {
                        stream: self.stream.clone(),
                        timestamp: time.clone(),
                        span: Some(Span {
                            first_packet: first.index,
                            last_packet: index,
                            first_timestamp: first.time.clone(),
                            last_timestamp: time.clone(),
                        }),
                        message,
                    };
                    event.validate()?;
                    out.push(Item::Event(Box::new(event)));
                }
                Err(reason) => {
                    out.push(self.diagnostic(DiagnosticKind::Malformed, 0, reason, index))
                }
            }
        }
        self.pending = pending.map(|start| {
            if start < offset {
                previous.unwrap_or(current.clone())
            } else {
                current
            }
        });
        Ok(out)
    }
    pub fn finish(&mut self) -> Vec<Item> {
        if self.stopped {
            return vec![];
        }
        self.stopped = true;
        let mut reasons = vec![];
        for d in self.bytes.finish() {
            if let codecs::DecodeItem::Diagnostic(d) = d {
                reasons.push(format!("{:?}", d.kind));
            }
        }
        for d in self.words.finish() {
            if let ump::DecodeOutput::Diagnostic(d) = d {
                reasons.push(d.reason);
            }
        }
        self.pending = None;
        reasons
            .into_iter()
            .map(|r| self.diagnostic(DiagnosticKind::Truncated, 0, r, self.expected))
            .collect()
    }
}

/// Typed output uses the existing owner-thread transport. No implicit protocol translation.
pub fn send_message(
    output: &crate::transport::OutputConnection,
    message: &Payload,
) -> crate::transport::Result<()> {
    use crate::messages::PrimitiveMessage;
    use crate::ump::UmpValue;
    match message {
        Payload::Command(_) => Err(crate::transport::Error::Capability(
            "use send_command for compound commands",
        )),
        Payload::Midi1(v) => output.send(&Packet::Bytes(v.encode())),
        Payload::Ump(
            ump::UmpMessage::Opaque(_)
            | ump::UmpMessage::DeltaClockstamp(_)
            | ump::UmpMessage::DeltaClockstampTicksPerQuarter(_),
        ) => Err(crate::transport::Error::Capability(
            "opaque/file-only UMP requires deliberate raw handling",
        )),
        Payload::Ump(ump::UmpMessage::Stream(v)) if matches!(v.status(), 32 | 33) => Err(
            crate::transport::Error::Capability("clip markers are file-only records"),
        ),
        Payload::Ump(v) => output.send(&Packet::Ump(v.encode_words())),
    }
}
