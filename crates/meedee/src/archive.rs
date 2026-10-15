//! meedee archive v1. Big endian, CRC-32/ISO-HDLC frames, explicit completion.
use crate::events::{Clock, Event, Payload, Span, Timestamp};
use crate::file_io::{invalid, Source};
use crate::messages::{self as m, PrimitiveMessage};
use crate::ump::{self, UmpValue};
use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Write};
pub const MAGIC: &[u8; 12] = b"MEEDEE\r\n\0\x01\0\0";
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Record {
    Clock(Clock),
    Stream {
        id: String,
        label: String,
    },
    Event(Box<Event>),
    Gap {
        stream: String,
        lost: u64,
        reason: String,
    },
    Metadata {
        key: String,
        data: Vec<u8>,
    },
    Discontinuity {
        stream: String,
        reason: String,
    },
    Opaque {
        kind: u16,
        data: Vec<u8>,
    },
}
#[derive(Default)]
struct Registry {
    clocks: BTreeMap<String, Clock>,
    streams: BTreeMap<String, String>,
    bytes: usize,
    records: u64,
}
impl Registry {
    fn check(&mut self, r: &Record) -> io::Result<()> {
        match r {
            Record::Clock(c) => {
                if self.clocks.contains_key(c.identity()) {
                    return Err(invalid(0, "duplicate clock descriptor"));
                }
                if self.clocks.len() + self.streams.len() >= 128 {
                    return Err(invalid(0, "descriptor count limit (128)"));
                }
                let n = c.identity().len() + c.origin().len();
                if n > 8_388_608usize.saturating_sub(self.bytes) {
                    return Err(invalid(0, "descriptor byte limit"));
                }
                self.bytes += n;
                self.clocks.insert(c.identity().into(), c.clone());
            }
            Record::Stream { id, label } => {
                if id.is_empty()
                    || self.streams.contains_key(id)
                    || self.clocks.len() + self.streams.len() >= 128
                {
                    return Err(invalid(
                        0,
                        "invalid/duplicate stream or descriptor count limit",
                    ));
                }
                let n = id.len() + label.len();
                if n > 8_388_608usize.saturating_sub(self.bytes) {
                    return Err(invalid(0, "descriptor byte limit"));
                }
                self.bytes += n;
                self.streams.insert(id.clone(), label.clone());
            }
            Record::Event(e) => {
                e.validate()?;
                if !self.streams.contains_key(&e.stream) {
                    return Err(invalid(0, "event stream is undeclared"));
                }
                for t in e.timestamp.iter().chain(
                    e.span
                        .iter()
                        .flat_map(|s| s.first_timestamp.iter().chain(s.last_timestamp.iter())),
                ) {
                    if self.clocks.get(t.clock.identity()) != Some(&t.clock) {
                        return Err(invalid(0, "timestamp clock is undeclared or inconsistent"));
                    }
                }
            }
            Record::Gap { stream, .. } | Record::Discontinuity { stream, .. }
                if !self.streams.contains_key(stream) =>
            {
                return Err(invalid(0, "diagnostic stream is undeclared"));
            }
            _ => {}
        }
        self.records = self
            .records
            .checked_add(1)
            .ok_or_else(|| invalid(0, "record count overflow"))?;
        Ok(())
    }
}
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !crc
}
fn string(out: &mut Vec<u8>, s: &str) -> io::Result<()> {
    blob(out, s.as_bytes())
}
fn blob(out: &mut Vec<u8>, b: &[u8]) -> io::Result<()> {
    out.extend(
        u32::try_from(b.len())
            .map_err(|_| invalid(0, "field too long"))?
            .to_be_bytes(),
    );
    out.extend(b);
    Ok(())
}
fn timestamp(out: &mut Vec<u8>, t: Option<&Timestamp>) -> io::Result<()> {
    out.push(u8::from(t.is_some()));
    if let Some(t) = t {
        out.extend(t.ticks.to_be_bytes());
        string(out, t.clock.identity())?;
    }
    Ok(())
}
fn command(out: &mut Vec<u8>, c: &m::AnyCommand) {
    use m::AnyCommand::*;
    let (id, fields): (u16, Vec<i64>) = match c {
        ControlChange14(v) => (
            1,
            vec![v.channel().into(), v.controller().into(), v.value().into()],
        ),
        RpnSet(v) => (
            2,
            vec![
                v.channel().into(),
                v.parameter().into(),
                v.value().into(),
                v.resolution().into(),
            ],
        ),
        NrpnSet(v) => (
            3,
            vec![
                v.channel().into(),
                v.parameter().into(),
                v.value().into(),
                v.resolution().into(),
            ],
        ),
        RpnIncrement(v) => (4, vec![v.channel().into(), v.parameter().into()]),
        RpnDecrement(v) => (5, vec![v.channel().into(), v.parameter().into()]),
        NrpnIncrement(v) => (6, vec![v.channel().into(), v.parameter().into()]),
        NrpnDecrement(v) => (7, vec![v.channel().into(), v.parameter().into()]),
        BankProgramSelect(v) => (
            8,
            vec![
                v.channel().into(),
                v.bank_msb().into(),
                v.bank_lsb().into(),
                v.program().into(),
            ],
        ),
        ZoneConfiguration(v) => (9, vec![v.manager_channel().into(), v.members().into()]),
    };
    out.extend(id.to_be_bytes());
    for n in fields {
        out.extend(n.to_be_bytes());
    }
}
pub fn encode(r: &Record) -> io::Result<Vec<u8>> {
    let mut out = vec![];
    let kind: u16 = match r {
        Record::Clock(c) => {
            string(&mut out, c.identity())?;
            out.extend(c.numerator().to_be_bytes());
            out.extend(c.denominator().to_be_bytes());
            string(&mut out, c.origin())?;
            out.push(u8::from(c.resolution_ticks().is_some()));
            if let Some(n) = c.resolution_ticks() {
                out.extend(n.to_be_bytes());
            }
            out.push(u8::from(c.backend().is_some()));
            if let Some(b) = c.backend() {
                out.extend(b.to_be_bytes());
            }
            out.push(match c.mode() {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
            1
        }
        Record::Stream { id, label } => {
            string(&mut out, id)?;
            string(&mut out, label)?;
            2
        }
        Record::Event(e) => {
            e.validate()?;
            string(&mut out, &e.stream)?;
            timestamp(&mut out, e.timestamp.as_ref())?;
            out.push(u8::from(e.span.is_some()));
            if let Some(s) = &e.span {
                out.extend(s.first_packet.to_be_bytes());
                out.extend(s.last_packet.to_be_bytes());
                timestamp(&mut out, s.first_timestamp.as_ref())?;
                timestamp(&mut out, s.last_timestamp.as_ref())?;
            }
            match &e.message {
                Payload::Midi1(v) => {
                    out.push(1);
                    out.extend(v.encode());
                }
                Payload::Ump(v) => {
                    out.push(2);
                    out.extend(ump::words_to_bytes(&v.encode_words(), false));
                }
                Payload::Command(v) => {
                    out.push(3);
                    command(&mut out, v);
                }
            }
            3
        }
        Record::Gap {
            stream,
            lost,
            reason,
        } => {
            string(&mut out, stream)?;
            out.extend(lost.to_be_bytes());
            string(&mut out, reason)?;
            4
        }
        Record::Metadata { key, data } => {
            string(&mut out, key)?;
            blob(&mut out, data)?;
            5
        }
        Record::Discontinuity { stream, reason } => {
            string(&mut out, stream)?;
            string(&mut out, reason)?;
            6
        }
        Record::Opaque { kind, data } => {
            if matches!(kind, 0 | 65535) {
                return Err(invalid(0, "opaque kind is reserved"));
            }
            out.extend(data);
            *kind
        }
    };
    let mut body = kind.to_be_bytes().to_vec();
    body.extend(out);
    Ok(body)
}
struct Fields<'a> {
    source: Source<Cursor<&'a [u8]>>,
}
impl<'a> Fields<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self {
            source: Source::new(Cursor::new(b), b.len().max(1)).expect("positive"),
        }
    }
    fn byte(&mut self) -> io::Result<u8> {
        self.source.byte()
    }
    fn flag(&mut self) -> io::Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid(self.source.offset, "invalid flag")),
        }
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_be_bytes(self.source.array()?))
    }
    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_be_bytes(self.source.array()?))
    }
    fn blob(&mut self) -> io::Result<Vec<u8>> {
        let n = u32::from_be_bytes(self.source.array()?) as usize;
        if n > self
            .source
            .inner
            .get_ref()
            .len()
            .saturating_sub(self.source.offset as usize)
        {
            return Err(invalid(self.source.offset, "field length exceeds record"));
        }
        self.source.payload(n)
    }
    fn string(&mut self) -> io::Result<String> {
        String::from_utf8(self.blob()?)
            .map_err(|_| invalid(self.source.offset, "invalid UTF-8 field"))
    }
    fn rest(&mut self) -> io::Result<Vec<u8>> {
        let n = self.source.inner.get_ref().len() - self.source.offset as usize;
        self.source.payload(n)
    }
    fn timestamp(&mut self, clocks: &BTreeMap<String, Clock>) -> io::Result<Option<Timestamp>> {
        if !self.flag()? {
            return Ok(None);
        }
        let ticks = self.i64()?;
        let id = self.string()?;
        let clock = clocks
            .get(&id)
            .ok_or_else(|| invalid(self.source.offset, "unknown timestamp clock"))?
            .clone();
        Ok(Some(Timestamp { ticks, clock }))
    }
    fn finish(&self) -> io::Result<()> {
        if self.source.offset as usize != self.source.inner.get_ref().len() {
            return Err(invalid(self.source.offset, "extra record bytes"));
        }
        Ok(())
    }
}
fn decode(body: &[u8], clocks: &BTreeMap<String, Clock>, preserve: bool) -> io::Result<Record> {
    let mut f = Fields::new(body);
    let kind = u16::from_be_bytes(f.source.array()?);
    let r = match kind {
        1 => {
            let id = f.string()?;
            let n = f.u64()?;
            let d = f.u64()?;
            let origin = f.string()?;
            let resolution = if f.flag()? { Some(f.u64()?) } else { None };
            let backend = if f.flag()? {
                Some(i32::from_be_bytes(f.source.array()?))
            } else {
                None
            };
            let mode = match f.byte()? {
                0 => None,
                1 => Some(false),
                2 => Some(true),
                _ => return Err(invalid(0, "unknown timestamp mode")),
            };
            let mut c = Clock::new(id, n, d, origin, resolution)?;
            if backend.is_some() != mode.is_some() {
                return Err(invalid(0, "partial transport clock descriptor"));
            }
            if let (Some(b), Some(m)) = (backend, mode) {
                c = c.with_transport(b, m);
            }
            Record::Clock(c)
        }
        2 => Record::Stream {
            id: f.string()?,
            label: f.string()?,
        },
        3 => {
            let stream = f.string()?;
            let timestamp = f.timestamp(clocks)?;
            let span = if f.flag()? {
                Some(Span {
                    first_packet: f.u64()?,
                    last_packet: f.u64()?,
                    first_timestamp: f.timestamp(clocks)?,
                    last_timestamp: f.timestamp(clocks)?,
                })
            } else {
                None
            };
            let tag = f.byte()?;
            let data = f.rest()?;
            let message = match tag {
                1 => {
                    let mut d = crate::codecs::Decoder::new(data.len().max(1))
                        .map_err(|e| invalid(0, e))?;
                    let mut items = d.feed(&data);
                    if !d.finish().is_empty() || items.len() != 1 {
                        return Err(invalid(0, "expected exactly one complete MIDI 1 message"));
                    }
                    match items.remove(0) {
                        crate::codecs::DecodeItem::Message(v) => Payload::Midi1(v),
                        _ => return Err(invalid(0, "invalid MIDI 1 payload")),
                    }
                }
                2 => Payload::Ump(
                    ump::decode(
                        &ump::bytes_to_words(&data, false).map_err(|e| invalid(0, e))?,
                        preserve,
                    )
                    .map_err(|e| invalid(0, e))?,
                ),
                3 => match decode_command(&data) {
                    Ok(v) => Payload::Command(v),
                    Err(e) if preserve && e.kind() == io::ErrorKind::Unsupported => {
                        return Ok(Record::Opaque {
                            kind: 3,
                            data: body[2..].to_vec(),
                        })
                    }
                    Err(e) => return Err(e),
                },
                _ => {
                    if preserve {
                        return Ok(Record::Opaque {
                            kind: 3,
                            data: body[2..].to_vec(),
                        });
                    }
                    return Err(invalid(0, "unknown message representation"));
                }
            };
            Record::Event(Box::new(Event {
                stream,
                timestamp,
                span,
                message,
            }))
        }
        4 => Record::Gap {
            stream: f.string()?,
            lost: f.u64()?,
            reason: f.string()?,
        },
        5 => Record::Metadata {
            key: f.string()?,
            data: f.blob()?,
        },
        6 => Record::Discontinuity {
            stream: f.string()?,
            reason: f.string()?,
        },
        _ => {
            if !preserve {
                return Err(invalid(0, "unknown record kind"));
            }
            Record::Opaque {
                kind,
                data: f.rest()?,
            }
        }
    };
    f.finish()?;
    Ok(r)
}
fn decode_command(b: &[u8]) -> io::Result<m::AnyCommand> {
    let mut f = Fields::new(b);
    let id = u16::from_be_bytes(f.source.array()?);
    use m::AnyCommand::*;
    let r = match id {
        1 => ControlChange14(
            m::ControlChange14::new(f.i64()?, f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?,
        ),
        2 => RpnSet(
            m::RpnSet::new(f.i64()?, f.i64()?, f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?,
        ),
        3 => NrpnSet(
            m::NrpnSet::new(f.i64()?, f.i64()?, f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?,
        ),
        4 => RpnIncrement(m::RpnIncrement::new(f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?),
        5 => RpnDecrement(m::RpnDecrement::new(f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?),
        6 => NrpnIncrement(m::NrpnIncrement::new(f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?),
        7 => NrpnDecrement(m::NrpnDecrement::new(f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?),
        8 => BankProgramSelect(
            crate::gm::BankProgramSelect::new(f.i64()?, f.i64()?, f.i64()?, f.i64()?)
                .map_err(|e| invalid(0, e))?,
        ),
        9 => ZoneConfiguration(
            crate::mpe::ZoneConfiguration::new(f.i64()?, f.i64()?).map_err(|e| invalid(0, e))?,
        ),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unknown command ID",
            ))
        }
    };
    f.finish()?;
    Ok(r)
}
pub fn payload_size(r: &Record) -> io::Result<usize> {
    fn timestamp_size(t: Option<&Timestamp>) -> usize {
        t.map_or(1, |t| 13 + t.clock.identity().len())
    }
    let n = match r {
        Record::Clock(c) => {
            2 + 4
                + c.identity().len()
                + 16
                + 4
                + c.origin().len()
                + 1
                + if c.resolution_ticks().is_some() { 8 } else { 0 }
                + 1
                + if c.backend().is_some() { 4 } else { 0 }
                + 1
        }
        Record::Stream { id, label } => 10 + id.len() + label.len(),
        Record::Event(e) => {
            let p = match &e.message {
                Payload::Midi1(m::AnyMessage::SysEx(v)) => v.data().len() + 2,
                Payload::Midi1(v) => v.encode().len(),
                Payload::Ump(v) => v.encode_words().len() * 4,
                Payload::Command(_) => 34,
            };
            7 + e.stream.len()
                + timestamp_size(e.timestamp.as_ref())
                + 1
                + e.span.as_ref().map_or(0, |s| {
                    16 + timestamp_size(s.first_timestamp.as_ref())
                        + timestamp_size(s.last_timestamp.as_ref())
                })
                + p
        }
        Record::Gap { stream, reason, .. } => 18 + stream.len() + reason.len(),
        Record::Discontinuity { stream, reason } => 10 + stream.len() + reason.len(),
        Record::Metadata { key, data } => 10 + key.len() + data.len(),
        Record::Opaque { data, .. } => 2 + data.len(),
    };
    Ok(n)
}
pub struct Writer<W> {
    inner: W,
    registry: Registry,
    limit: usize,
    offset: u64,
    finished: bool,
    failed: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(mut inner: W, limit: usize) -> io::Result<Self> {
        if limit == 0 {
            return Err(invalid(0, "limit must be positive"));
        }
        inner.write_all(MAGIC)?;
        Ok(Self {
            inner,
            registry: Registry::default(),
            limit,
            offset: 12,
            finished: false,
            failed: false,
        })
    }
    fn frame(&mut self, body: &[u8]) -> io::Result<()> {
        if body.len() > self.limit {
            return Err(invalid(self.offset, "record payload limit"));
        }
        self.inner.write_all(
            &u32::try_from(body.len())
                .map_err(|_| invalid(self.offset, "record too long"))?
                .to_be_bytes(),
        )?;
        self.inner.write_all(body)?;
        self.inner.write_all(&crc32(body).to_be_bytes())?;
        self.offset += body.len() as u64 + 8;
        Ok(())
    }
    pub fn write(&mut self, r: &Record) -> io::Result<()> {
        if self.finished || self.failed {
            return Err(invalid(self.offset, "writer closed or failed"));
        }
        let result = (|| {
            if payload_size(r)? > self.limit {
                return Err(invalid(self.offset, "record payload limit"));
            }
            let body = encode(r)?;
            if body.len() > self.limit {
                return Err(invalid(self.offset, "record limit"));
            }
            self.registry.check(r)?;
            self.frame(&body)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "record {}, byte {}: {e}",
                    self.registry.records, self.offset
                ),
            )
        })
    }
    pub fn finish(&mut self, complete: bool, reason: &str) -> io::Result<()> {
        if self.finished || self.failed {
            return Err(invalid(self.offset, "writer closed or failed"));
        }
        let result = (|| {
            let mut body = 65535u16.to_be_bytes().to_vec();
            body.extend(self.registry.records.to_be_bytes());
            body.push(u8::from(complete));
            string(&mut body, reason)?;
            self.frame(&body)?;
            self.inner.flush()
        })();
        if result.is_ok() {
            self.finished = true;
        } else {
            self.failed = true;
        }
        result
    }
}
pub struct Reader<R> {
    source: Source<R>,
    registry: Registry,
    recover: bool,
    preserve: bool,
    stopped: bool,
    pub complete: bool,
    pub problem: Option<String>,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, limit: usize, recover: bool, preserve: bool) -> io::Result<Self> {
        let mut source = Source::new(inner, limit)?;
        if source.array::<12>()? != *MAGIC {
            return Err(invalid(0, "invalid archive magic/version"));
        }
        Ok(Self {
            source,
            registry: Registry::default(),
            recover,
            preserve,
            stopped: false,
            complete: false,
            problem: None,
        })
    }
    fn read_record(&mut self) -> io::Result<Option<Record>> {
        let a = self.source.byte()?;
        let t = self.source.array::<3>()?;
        let n = u32::from_be_bytes([a, t[0], t[1], t[2]]) as usize;
        let body = self.source.payload(n)?;
        let expected = u32::from_be_bytes(self.source.array()?);
        if crc32(&body) != expected {
            return Err(invalid(self.source.offset, "CRC mismatch"));
        }
        if body.len() < 2 {
            return Err(invalid(self.source.offset, "short record"));
        }
        if body[..2] == [255, 255] {
            let mut f = Fields::new(&body[2..]);
            if f.u64()? != self.registry.records {
                return Err(invalid(self.source.offset, "trailer record-count mismatch"));
            }
            let complete = f.flag()?;
            let reason = f.string()?;
            f.finish()?;
            if self.source.optional_byte()?.is_some() {
                return Err(invalid(self.source.offset, "data after trailer"));
            }
            if !complete {
                return Err(invalid(
                    self.source.offset,
                    format!("incomplete capture: {reason}"),
                ));
            }
            self.complete = true;
            self.stopped = true;
            return Ok(None);
        }
        let r = decode(&body, &self.registry.clocks, self.preserve)?;
        self.registry.check(&r)?;
        Ok(Some(r))
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.stopped {
            return None;
        }
        match self.read_record() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.stopped = true;
                let message = format!(
                    "record {}, byte {}: {e}",
                    self.registry.records, self.source.offset
                );
                self.problem = Some(message.clone());
                if self.recover {
                    None
                } else {
                    Some(Err(io::Error::new(e.kind(), message)))
                }
            }
        }
    }
}
