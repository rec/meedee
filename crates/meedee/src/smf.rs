//! RP-001 v1.0 streaming Standard MIDI Files. File records are never sent implicitly.
use crate::codecs::{DecodeItem, Decoder};
use crate::file_io::{invalid, Source};
use crate::messages::{AnyMessage, PrimitiveMessage};
use std::io::{self, Read, Seek, SeekFrom, Write};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    format: u8,
    tracks: u16,
    division: u16,
    extra: Vec<u8>,
}
impl Header {
    pub fn new(format: u8, tracks: u16, division: u16, extra: Vec<u8>) -> io::Result<Self> {
        if format > 2 || tracks == 0 || format == 0 && tracks != 1 {
            return Err(invalid(0, "invalid SMF format/track count"));
        }
        if division & 0x8000 == 0 {
            if division == 0 {
                return Err(invalid(0, "PPQN must be positive"));
            }
        } else if !matches!((division >> 8) as u8 as i8, -24 | -25 | -29 | -30)
            || division & 255 == 0
        {
            return Err(invalid(0, "invalid SMPTE division"));
        }
        Ok(Self {
            format,
            tracks,
            division,
            extra,
        })
    }
    pub fn format(&self) -> u8 {
        self.format
    }
    pub fn tracks(&self) -> u16 {
        self.tracks
    }
    pub fn division(&self) -> u16 {
        self.division
    }
    pub fn extra(&self) -> &[u8] {
        &self.extra
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Record {
    Message(AnyMessage),
    Meta { kind: u8, data: Vec<u8> },
    SysEx { status: u8, data: Vec<u8> },
    Chunk { id: [u8; 4], data: Vec<u8> },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Event {
    pub track: u16,
    pub tick: u64,
    pub record: Record,
}

pub struct Reader<R> {
    source: Source<R>,
    pub header: Header,
    track: u16,
    end: u64,
    tick: u64,
    running: Option<u8>,
    ended: bool,
    failed: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, limit: usize) -> io::Result<Self> {
        let mut source = Source::new(inner, limit)?;
        if source.array::<4>()? != *b"MThd" {
            return Err(invalid(0, "expected MThd"));
        }
        let n = u32::from_be_bytes(source.array()?) as usize;
        if n < 6 {
            return Err(invalid(4, "short SMF header"));
        }
        let bytes = source.payload(n)?;
        let header = Header::new(
            u16::from_be_bytes([bytes[0], bytes[1]])
                .try_into()
                .map_err(|_| invalid(8, "invalid format"))?,
            u16::from_be_bytes([bytes[2], bytes[3]]),
            u16::from_be_bytes([bytes[4], bytes[5]]),
            bytes[6..].to_vec(),
        )?;
        let end = source.offset;
        Ok(Self {
            source,
            header,
            track: 0,
            end,
            tick: 0,
            running: None,
            ended: true,
            failed: false,
        })
    }
    fn byte(&mut self) -> io::Result<u8> {
        if self.source.offset >= self.end {
            return Err(invalid(self.source.offset, "event crosses track boundary"));
        }
        self.source.byte()
    }
    fn vlq(&mut self) -> io::Result<u32> {
        let mut n = 0;
        for _ in 0..4 {
            let b = self.byte()?;
            n = (n << 7) | u32::from(b & 127);
            if b < 128 {
                return Ok(n);
            }
        }
        Err(invalid(self.source.offset, "VLQ exceeds four bytes"))
    }
    fn payload(&mut self, n: usize) -> io::Result<Vec<u8>> {
        if n as u64 > self.end - self.source.offset {
            return Err(invalid(
                self.source.offset,
                "payload crosses track boundary",
            ));
        }
        self.source.payload(n)
    }
    fn read_event(&mut self) -> io::Result<Option<Event>> {
        if self.source.offset == self.end && self.end != 0 && !self.ended {
            return Err(invalid(self.source.offset, "track is missing EndOfTrack"));
        }
        if self.source.offset == self.end {
            let Some(first) = self.source.optional_byte()? else {
                if self.track != self.header.tracks {
                    return Err(invalid(self.source.offset, "missing tracks"));
                }
                return Ok(None);
            };
            let tail = self.source.array::<3>()?;
            let id = [first, tail[0], tail[1], tail[2]];
            let n = u32::from_be_bytes(self.source.array()?) as usize;
            if id != *b"MTrk" {
                let data = self.source.payload(n)?;
                self.end = self.source.offset;
                return Ok(Some(Event {
                    track: self.track,
                    tick: 0,
                    record: Record::Chunk { id, data },
                }));
            }
            if self.track >= self.header.tracks {
                return Err(invalid(self.source.offset, "extra track"));
            }
            self.track += 1;
            self.end = self.source.offset + n as u64;
            self.tick = 0;
            self.running = None;
            self.ended = false;
        }
        if self.ended {
            return Err(invalid(self.source.offset, "data after EndOfTrack"));
        }
        self.tick = self
            .tick
            .checked_add(u64::from(self.vlq()?))
            .ok_or_else(|| invalid(self.source.offset, "tick overflow"))?;
        let first = self.byte()?;
        let status = if first < 128 {
            self.running
                .ok_or_else(|| invalid(self.source.offset - 1, "data without running status"))?
        } else {
            first
        };
        let record = match status {
            0x80..=0xef => {
                self.running = Some(status);
                let length = if matches!(status & 0xf0, 0xc0 | 0xd0) {
                    1
                } else {
                    2
                };
                let mut data = vec![status];
                if first < 128 {
                    data.push(first);
                }
                while data.len() <= length {
                    let b = self.byte()?;
                    if b >= 128 {
                        return Err(invalid(self.source.offset - 1, "status in channel data"));
                    }
                    data.push(b);
                }
                let mut decoder = Decoder::default();
                let items = decoder.feed(&data);
                let Some(DecodeItem::Message(m)) = items.into_iter().next() else {
                    return Err(invalid(self.source.offset, "invalid channel message"));
                };
                Record::Message(m)
            }
            0xff => {
                self.running = None;
                let kind = self.byte()?;
                if kind > 127 {
                    return Err(invalid(self.source.offset - 1, "invalid meta type"));
                }
                let n = self.vlq()? as usize;
                let data = self.payload(n)?;
                validate_meta(kind, &data, self.source.offset)?;
                if kind == 47 {
                    self.ended = true;
                }
                Record::Meta { kind, data }
            }
            0xf0 | 0xf7 => {
                self.running = None;
                let n = self.vlq()? as usize;
                Record::SysEx {
                    status,
                    data: self.payload(n)?,
                }
            }
            _ => {
                return Err(invalid(
                    self.source.offset - 1,
                    "system status is not an SMF event",
                ))
            }
        };
        Ok(Some(Event {
            track: self.track - 1,
            tick: self.tick,
            record,
        }))
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<Event>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.read_event() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub fn validate_meta(kind: u8, data: &[u8], offset: u64) -> io::Result<()> {
    let expected = match kind {
        0 => Some(2),
        32 | 33 => Some(1),
        47 => Some(0),
        81 => Some(3),
        84 => Some(5),
        88 => Some(4),
        89 => Some(2),
        _ => None,
    };
    if kind > 127 || expected.is_some_and(|n| data.len() < n) {
        return Err(invalid(offset, "invalid meta length/type"));
    }
    if kind == 81 && data[..3] == [0, 0, 0] || kind == 32 && data[0] > 15 {
        return Err(invalid(offset, "invalid tempo/channel prefix"));
    }
    Ok(())
}
pub struct Writer<W> {
    inner: W,
    header: Header,
    track: u16,
    start: Option<u64>,
    tick: u64,
    ended: bool,
    limit: usize,
    failed: bool,
}
impl<W: Write + Seek> Writer<W> {
    pub fn new(mut inner: W, header: Header, limit: usize) -> io::Result<Self> {
        if limit == 0 || header.extra.len() > limit {
            return Err(invalid(0, "invalid payload limit/header size"));
        }
        inner.stream_position()?;
        inner.write_all(b"MThd")?;
        inner.write_all(
            &(6u32
                .checked_add(
                    header
                        .extra
                        .len()
                        .try_into()
                        .map_err(|_| invalid(0, "header too large"))?,
                )
                .ok_or_else(|| invalid(0, "header too large"))?)
            .to_be_bytes(),
        )?;
        inner.write_all(&[0, header.format])?;
        inner.write_all(&header.tracks.to_be_bytes())?;
        inner.write_all(&header.division.to_be_bytes())?;
        inner.write_all(&header.extra)?;
        Ok(Self {
            inner,
            header,
            track: 0,
            start: None,
            tick: 0,
            ended: true,
            limit,
            failed: false,
        })
    }
    pub fn write(&mut self, event: &Event) -> io::Result<()> {
        if self.failed {
            return Err(invalid(0, "writer failed"));
        }
        let result = self.write_event(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn write_event(&mut self, event: &Event) -> io::Result<()> {
        if let Record::Chunk { id, data } = &event.record {
            if !self.ended || *id == *b"MThd" || *id == *b"MTrk" {
                return Err(invalid(0, "invalid unknown chunk placement/id"));
            }
            if data.len() > self.limit {
                return Err(invalid(0, "chunk limit"));
            }
            self.finish_track()?;
            self.inner.write_all(id)?;
            self.inner.write_all(&(data.len() as u32).to_be_bytes())?;
            return self.inner.write_all(data);
        }
        if self.start.is_none() || self.ended {
            if event.track != self.track || self.track >= self.header.tracks {
                return Err(invalid(0, "tracks must be contiguous and ordered"));
            }
            self.finish_track()?;
            self.inner.write_all(b"MTrk\0\0\0\0")?;
            self.start = Some(self.inner.stream_position()?);
            self.tick = 0;
            self.ended = false;
            self.track += 1;
        }
        if event.track != self.track - 1 || event.tick < self.tick {
            return Err(invalid(0, "track/tick order mismatch"));
        }
        let delta = event.tick - self.tick;
        if delta > 0x0fffffff {
            return Err(invalid(0, "delta exceeds SMF VLQ range"));
        }
        let mut bytes = vlq(delta as u32);
        match &event.record {
            Record::Message(m) => {
                let b = m.encode();
                if !(0x80..=0xef).contains(&b[0]) {
                    return Err(invalid(0, "SMF channel event required; use SysEx record"));
                }
                bytes.extend(b);
            }
            Record::Meta { kind, data } => {
                validate_meta(*kind, data, 0)?;
                if data.len() > self.limit || data.len() > 0x0fffffff {
                    return Err(invalid(0, "meta payload limit"));
                }
                bytes.extend([255, *kind]);
                bytes.extend(vlq(data.len() as u32));
                bytes.extend(data);
                self.ended = *kind == 47;
            }
            Record::SysEx { status, data } => {
                if !matches!(status, 0xf0 | 0xf7)
                    || data.len() > self.limit
                    || data.len() > 0x0fffffff
                {
                    return Err(invalid(0, "invalid SysEx record/limit"));
                }
                bytes.push(*status);
                bytes.extend(vlq(data.len() as u32));
                bytes.extend(data);
            }
            Record::Chunk { .. } => unreachable!(),
        }
        self.inner.write_all(&bytes)?;
        self.tick = event.tick;
        Ok(())
    }
    fn finish_track(&mut self) -> io::Result<()> {
        if let Some(start) = self.start.take() {
            if !self.ended {
                return Err(invalid(start, "missing EndOfTrack"));
            }
            let end = self.inner.stream_position()?;
            let n: u32 = (end - start)
                .try_into()
                .map_err(|_| invalid(end, "track exceeds u32 length"))?;
            self.inner.seek(SeekFrom::Start(start - 4))?;
            self.inner.write_all(&n.to_be_bytes())?;
            self.inner.seek(SeekFrom::Start(end))?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(invalid(0, "writer failed"));
        }
        if self.track != self.header.tracks {
            return Err(invalid(0, "track count mismatch"));
        }
        self.finish_track()?;
        self.inner.flush()
    }
}
pub fn vlq(mut n: u32) -> Vec<u8> {
    let mut b = vec![(n & 127) as u8];
    while {
        n >>= 7;
        n != 0
    } {
        b.push((n & 127) as u8 | 128);
    }
    b.reverse();
    b
}

#[derive(Clone)]
struct TrackState {
    position: u64,
    end: u64,
    tick: u64,
    running: Option<u8>,
    ended: bool,
}
/// Seek-based performance ordering, one bounded lookahead record per selected track.
/// Non-track chunks remain available through Reader, not this performance view.
pub struct MergedReader<R> {
    reader: Reader<R>,
    pub header: Header,
    base: u64,
    tracks: Vec<TrackState>,
    pending: Vec<Option<Event>>,
    heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, usize)>>,
    refill: Option<usize>,
    used: usize,
    byte_limit: usize,
    failed: bool,
}
impl<R: Read + Seek> MergedReader<R> {
    pub fn new(
        mut inner: R,
        limit: usize,
        track_limit: usize,
        byte_limit: usize,
        sequence: Option<u16>,
    ) -> io::Result<Self> {
        if track_limit == 0 || byte_limit == 0 {
            return Err(invalid(0, "merge limits must be positive"));
        }
        let base = inner.stream_position()?;
        let file_end = inner.seek(SeekFrom::End(0))?;
        inner.seek(SeekFrom::Start(base))?;
        let mut reader = Reader::new(inner, limit)?;
        let header = reader.header.clone();
        if usize::from(header.tracks) > track_limit {
            return Err(invalid(0, "track merge limit"));
        }
        if header.format == 2 {
            if sequence.is_none_or(|n| n >= header.tracks) {
                return Err(invalid(
                    0,
                    "type 2 requires an explicit valid sequence index",
                ));
            }
        } else if sequence.is_some() {
            return Err(invalid(0, "sequence selection applies only to type 2"));
        }
        let mut tracks = vec![];
        while let Some(first) = reader.source.optional_byte()? {
            let t = reader.source.array::<3>()?;
            let id = [first, t[0], t[1], t[2]];
            let n = u32::from_be_bytes(reader.source.array()?) as u64;
            let end = reader
                .source
                .offset
                .checked_add(n)
                .ok_or_else(|| invalid(reader.source.offset, "chunk length overflow"))?;
            if end > file_end - base {
                return Err(invalid(reader.source.offset, "chunk extends beyond input"));
            }
            if id == *b"MTrk" {
                if tracks.len() >= usize::from(header.tracks) {
                    return Err(invalid(reader.source.offset, "extra track"));
                }
                tracks.push(TrackState {
                    position: reader.source.offset,
                    end,
                    tick: 0,
                    running: None,
                    ended: false,
                });
            } else if id == *b"MThd" {
                return Err(invalid(reader.source.offset, "duplicate header"));
            }
            reader.source.inner.seek(SeekFrom::Start(base + end))?;
            reader.source.offset = end;
        }
        if tracks.len() != usize::from(header.tracks) {
            return Err(invalid(reader.source.offset, "missing tracks"));
        }
        let pending = vec![None; tracks.len()];
        let mut result = Self {
            reader,
            header,
            base,
            tracks,
            pending,
            heap: std::collections::BinaryHeap::new(),
            refill: None,
            used: 0,
            byte_limit,
            failed: false,
        };
        for i in 0..result.tracks.len() {
            if sequence.is_none_or(|n| usize::from(n) == i) {
                result.fill(i)?;
            }
        }
        Ok(result)
    }
    fn fill(&mut self, i: usize) -> io::Result<()> {
        let state = &self.tracks[i];
        if state.ended {
            return Ok(());
        }
        self.reader
            .source
            .inner
            .seek(SeekFrom::Start(self.base + state.position))?;
        self.reader.source.offset = state.position;
        self.reader.end = state.end;
        self.reader.tick = state.tick;
        self.reader.running = state.running;
        self.reader.track = i as u16 + 1;
        self.reader.ended = false;
        let event = self
            .reader
            .read_event()?
            .ok_or_else(|| invalid(state.position, "missing track event"))?;
        if self.reader.ended && self.reader.source.offset != state.end {
            return Err(invalid(self.reader.source.offset, "data after EndOfTrack"));
        }
        self.tracks[i] = TrackState {
            position: self.reader.source.offset,
            end: self.reader.end,
            tick: self.reader.tick,
            running: self.reader.running,
            ended: self.reader.ended,
        };
        let n = record_size(&event.record);
        if n > self.byte_limit.saturating_sub(self.used) {
            return Err(invalid(
                self.reader.source.offset,
                "track lookahead byte limit",
            ));
        }
        self.used += n;
        self.heap.push(std::cmp::Reverse((event.tick, i)));
        self.pending[i] = Some(event);
        Ok(())
    }
}
impl<R: Read + Seek> Iterator for MergedReader<R> {
    type Item = io::Result<Event>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        if let Some(i) = self.refill.take() {
            if let Err(e) = self.fill(i) {
                self.failed = true;
                return Some(Err(e));
            }
        }
        let std::cmp::Reverse((_, i)) = self.heap.pop()?;
        let event = self.pending[i].take().expect("heap indexes pending event");
        self.used -= record_size(&event.record);
        self.refill = Some(i);
        Some(Ok(event))
    }
}
fn record_size(r: &Record) -> usize {
    match r {
        Record::Message(AnyMessage::SysEx(v)) => v.data().len() + 2,
        Record::Message(v) => v.encode().len(),
        Record::Meta { data, .. } | Record::SysEx { data, .. } | Record::Chunk { data, .. } => {
            data.len()
        }
    }
}
