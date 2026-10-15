//! Exact standard-file clocks and explicit conversion to/from timed live values.
use crate::{
    clip,
    events::{timestamp_seconds, Clock, Event, Payload, Rational, Timestamp},
    file_io::invalid,
    live,
    messages::{Command, PrimitiveMessage},
    playback::Mapping,
    smf,
    transport::{Packet, Protocol},
    ump::{self, UmpMessage, UmpValue},
};
use std::io;
pub struct SmfTimeline {
    header: smf::Header,
    sequence: Option<u16>,
    clock: Clock,
    decoders: std::collections::BTreeMap<u16, live::Decoder>,
    stream: String,
    limit: usize,
    tick: u64,
    elapsed: i128,
    tempo: u32,
    index: u64,
}
impl SmfTimeline {
    pub fn new(
        header: smf::Header,
        stream: String,
        sequence: Option<u16>,
        limit: usize,
    ) -> io::Result<Self> {
        if header.format() == 2 {
            if sequence.is_none_or(|n| n >= header.tracks()) {
                return Err(invalid(0, "type 2 requires a selected sequence"));
            }
        } else if sequence.is_some() {
            return Err(invalid(0, "sequence selection is only for type 2"));
        }
        let division = header.division();
        let (n, d) = if division & 0x8000 == 0 {
            (1, u64::from(division) * 1_000_000)
        } else {
            let fps = (division >> 8) as u8 as i8;
            let ticks = u64::from(division & 255);
            if fps == -29 {
                (1001, 30_000 * ticks)
            } else {
                (1, u64::from(fps.unsigned_abs()) * ticks)
            }
        };
        let clock = Clock::new(
            crate::events::fresh_identity("smf")?,
            n,
            d,
            "smf_timeline".into(),
            None,
        )?;
        live::Decoder::new(stream.clone(), Protocol::Midi1, limit)?;
        Ok(Self {
            header,
            sequence,
            clock,
            decoders: std::collections::BTreeMap::new(),
            stream,
            limit,
            tick: 0,
            elapsed: 0,
            tempo: 500_000,
            index: 0,
        })
    }
    pub fn clock(&self) -> &Clock {
        &self.clock
    }
    pub fn origin(&self) -> Timestamp {
        Timestamp {
            ticks: 0,
            clock: self.clock.clone(),
        }
    }
    fn begin_record(&mut self, event: &smf::Event) -> io::Result<(Vec<u8>, i64)> {
        if event.track >= self.header.tracks() || self.sequence.is_some_and(|n| n != event.track) {
            return Err(invalid(0, "event track is outside selected sequence"));
        }
        if event.tick < self.tick {
            return Err(invalid(0, "feed SMF events in merged time order"));
        }
        let delta = event.tick - self.tick;
        self.elapsed = self
            .elapsed
            .checked_add(
                i128::from(delta)
                    * if self.header.division() & 0x8000 == 0 {
                        i128::from(self.tempo)
                    } else {
                        1
                    },
            )
            .ok_or_else(|| invalid(0, "timeline overflow"))?;
        self.tick = event.tick;
        let timestamp: i64 = self
            .elapsed
            .try_into()
            .map_err(|_| invalid(0, "exact file clock exceeds signed 64-bit timestamp range"))?;
        let data = match &event.record {
            smf::Record::Message(v) => v.encode(),
            smf::Record::SysEx { status, data } => {
                let mut bytes = Vec::with_capacity(data.len() + 1);
                if *status == 240 {
                    bytes.push(240);
                } else if *status != 247 {
                    return Err(invalid(0, "invalid SMF SysEx status"));
                }
                bytes.extend(data);
                bytes
            }
            smf::Record::Meta { kind, data } => {
                smf::validate_meta(*kind, data, 0)?;
                if *kind == 81 {
                    if self.header.format() == 1 && event.track != 0 {
                        return Err(invalid(0, "type 1 tempo must come from track 0"));
                    }
                    self.tempo = u32::from_be_bytes([0, data[0], data[1], data[2]]);
                }
                if *kind == 33 && data[0] != 0 {
                    return Err(invalid(
                        0,
                        "route nonzero SMF port prefixes explicitly before conversion",
                    ));
                }
                vec![]
            }
            smf::Record::Chunk { .. } => {
                return Err(invalid(
                    0,
                    "unknown chunks are file records; use Reader for preservation",
                ))
            }
        };
        Ok((data, timestamp))
    }
    fn feed_chunk(
        &mut self,
        track: u16,
        data: &[u8],
        timestamp: i64,
    ) -> io::Result<Vec<live::Item>> {
        if !self.decoders.contains_key(&track) {
            self.decoders.insert(
                track,
                live::Decoder::new(self.stream.clone(), Protocol::Midi1, self.limit)?,
            );
        }
        let decoder = self
            .decoders
            .get_mut(&track)
            .expect("inserted track decoder");
        decoder.begin_at(self.index);
        let out = decoder.feed(
            &Packet::Bytes(data.to_vec()),
            &self.clock,
            Some(timestamp),
            Some(self.index),
        )?;
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "file chunk index exhausted"))?;
        Ok(out)
    }
    pub fn feed(&mut self, event: &smf::Event) -> io::Result<Vec<live::Item>> {
        let (data, time) = self.begin_record(event)?;
        self.feed_chunk(event.track, &data, time)
    }
    pub fn finish(&mut self) -> Vec<live::Item> {
        self.decoders
            .values_mut()
            .flat_map(|d| d.finish())
            .collect()
    }
}
pub struct ClipTimeline {
    stream: String,
    clock: Option<Clock>,
    decoder: Option<live::Decoder>,
    tick: u64,
    elapsed: i128,
    tempo: u32,
    index: u64,
    include_configuration: bool,
    origin_ticks: Option<i64>,
}
impl ClipTimeline {
    pub fn new(stream: String, include_configuration: bool) -> io::Result<Self> {
        if stream.is_empty() {
            return Err(invalid(0, "empty source"));
        }
        Ok(Self {
            stream,
            clock: None,
            decoder: None,
            tick: 0,
            elapsed: 0,
            tempo: 50_000_000,
            index: 0,
            include_configuration,
            origin_ticks: include_configuration.then_some(0),
        })
    }
    pub fn clock(&self) -> Option<&Clock> {
        self.clock.as_ref()
    }
    pub fn origin(&self) -> Option<Timestamp> {
        Some(Timestamp {
            ticks: self.origin_ticks?,
            clock: self.clock.as_ref()?.clone(),
        })
    }
    pub fn feed(&mut self, entry: &clip::Entry) -> io::Result<Vec<live::Item>> {
        let index = self.index;
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "file index exhausted"))?;
        if let UmpMessage::DeltaClockstampTicksPerQuarter(v) = &entry.message {
            if self.clock.is_some() {
                return Err(invalid(0, "duplicate file clock"));
            }
            let clock = Clock::new(
                crate::events::fresh_identity("clip")?,
                1,
                u64::from(v.ticks()) * 100_000_000,
                "clip_timeline".into(),
                None,
            )?;
            let mut decoder = live::Decoder::new(self.stream.clone(), Protocol::Ump, 1_048_576)?;
            decoder.begin_at(index);
            self.clock = Some(clock);
            self.decoder = Some(decoder);
        }
        let Some(clock) = self.clock.as_ref() else {
            if matches!(entry.message, UmpMessage::DeltaClockstamp(_)) {
                return Ok(vec![]);
            }
            return Err(invalid(0,"profile-tag configuration needs explicit MIDI-CI handling before timeline conversion"));
        };
        let tick = entry
            .tick
            .ok_or_else(|| invalid(0, "clip entry has no clockstamp"))?;
        if tick < self.tick {
            return Err(invalid(0, "clip time regressed"));
        }
        self.elapsed = self
            .elapsed
            .checked_add(i128::from(tick - self.tick) * i128::from(self.tempo))
            .ok_or_else(|| invalid(0, "clip time overflow"))?;
        self.tick = tick;
        let file_only = matches!(
            entry.message,
            UmpMessage::DeltaClockstamp(_) | UmpMessage::DeltaClockstampTicksPerQuarter(_)
        ) || matches!(&entry.message,UmpMessage::Stream(v)if matches!(v.status(),32|33));
        let tempo = if let UmpMessage::FlexData(f) = &entry.message {
            if f.status_bank() == 0 && f.status() == 0 {
                Some(f.data()[0])
            } else {
                None
            }
        } else {
            None
        };
        let signature =
            matches!(&entry.message,UmpMessage::FlexData(f)if f.status_bank()==0&&f.status()==1);
        if !entry.sequence
            && !file_only
            && !self.include_configuration
            && tempo.is_none()
            && !signature
        {
            return Err(invalid(
                0,
                "clip configuration needs include_configuration=True or explicit handling",
            ));
        }
        let suppress = file_only || !entry.sequence && !self.include_configuration;
        let timestamp = self
            .elapsed
            .try_into()
            .map_err(|_| invalid(0, "exact clip time exceeds signed 64-bit clock range"))?;
        if !self.include_configuration
            && matches!(&entry.message,UmpMessage::Stream(v)if v.status()==32)
        {
            self.origin_ticks = Some(timestamp);
        }
        let out = self.decoder.as_mut().expect("created with clock").feed(
            &Packet::Ump(if suppress {
                vec![]
            } else {
                entry.message.encode_words()
            }),
            clock,
            Some(timestamp),
            Some(index),
        )?;
        if let Some(tempo) = tempo {
            self.tempo = tempo;
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Vec<live::Item> {
        self.decoder.as_mut().map_or_else(Vec::new, |d| d.finish())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExportReport {
    pub status: ump::TranslationStatus,
    pub reason: String,
    pub rounding_error_ticks: Rational,
    pub events: Vec<smf::Event>,
}
pub fn export_smf(
    event: &Event,
    header: &smf::Header,
    origin: &Timestamp,
    track: u16,
    tempo: u32,
    mapping: Option<&Mapping>,
    allow_loss: bool,
) -> io::Result<ExportReport> {
    event.validate()?;
    if origin.clock.mode() == Some(false) {
        return Err(invalid(0, "disabled export origin"));
    }
    let zero = Rational::new(0, 1)?;
    let denied = |status, reason: &str| ExportReport {
        status,
        reason: reason.into(),
        rounding_error_ticks: zero,
        events: vec![],
    };
    if track >= header.tracks() || tempo == 0 || tempo > 0xffffff {
        return Err(invalid(0, "invalid target track or tempo"));
    }
    let Some(t) = &event.timestamp else {
        return Ok(denied(
            ump::TranslationStatus::ContextRequired,
            "timestamp is absent; supply an explicit timing policy",
        ));
    };
    let seconds = if t.clock == origin.clock {
        timestamp_seconds(t)?
    } else if let Some(m) = mapping {
        if m.target != origin.clock {
            return Err(invalid(0, "mapping target differs from export origin"));
        }
        m.map(t)?
    } else {
        return Ok(denied(
            ump::TranslationStatus::ContextRequired,
            "clock mapping required",
        ));
    };
    let relative = seconds.checked_sub(timestamp_seconds(origin)?)?;
    if relative.numerator() < 0 {
        return Ok(denied(
            ump::TranslationStatus::Unsupported,
            "event precedes export origin",
        ));
    }
    let division = header.division();
    let tick_value = if division & 0x8000 == 0 {
        relative.multiply(i128::from(division) * 1_000_000, i128::from(tempo))?
    } else {
        let fps = (division >> 8) as u8 as i8;
        let ticks = i128::from(division & 255);
        if fps == -29 {
            relative.multiply(30_000 * ticks, 1001)?
        } else {
            relative.multiply(i128::from(fps.unsigned_abs()) * ticks, 1)?
        }
    };
    let (tick, error) = tick_value.round(true)?;
    let Ok(tick) = u64::try_from(tick) else {
        return Ok(denied(
            ump::TranslationStatus::Unsupported,
            "SMF absolute tick overflow",
        ));
    };
    let mut status = if error.numerator() == 0 {
        ump::TranslationStatus::Exact
    } else {
        ump::TranslationStatus::Quantized
    };
    let mut reason = if error.numerator() == 0 {
        "exact timing conversion"
    } else {
        "timing requires rounding"
    }
    .to_string();
    let messages = match &event.message {
        Payload::Midi1(v) => vec![v.clone()],
        Payload::Command(c) => {
            reason.push_str("; command explicitly expanded");
            c.expand()
        }
        Payload::Ump(v) => {
            let report = ump::to_midi1(v, allow_loss).map_err(|e| invalid(0, e))?;
            if report.group.is_some() {
                return Ok(denied(ump::TranslationStatus::ContextRequired,"resolve UMP group routing explicitly before legacy SMF export; use the translation report to choose a destination"));
            }
            if report.status != ump::TranslationStatus::Exact {
                status = report.status;
            }
            reason.push_str("; ");
            reason.push_str(report.reason);
            report.messages
        }
    };
    let permitted = matches!(status, ump::TranslationStatus::Exact)
        || status == ump::TranslationStatus::Quantized && allow_loss;
    let events = if permitted {
        messages
            .into_iter()
            .map(|m| {
                let bytes = m.encode();
                let record = if bytes[0] == 240 {
                    smf::Record::SysEx {
                        status: 240,
                        data: bytes[1..].to_vec(),
                    }
                } else if bytes[0] >= 240 {
                    smf::Record::SysEx {
                        status: 247,
                        data: bytes,
                    }
                } else {
                    smf::Record::Message(m)
                };
                smf::Event {
                    track,
                    tick,
                    record,
                }
            })
            .collect()
    } else {
        vec![]
    };
    Ok(ExportReport {
        status,
        reason,
        rounding_error_ticks: error,
        events,
    })
}

/// Bounded performance iterator. Escape events are decoded in 256-byte chunks.
pub struct SmfReader<R> {
    reader: smf::MergedReader<R>,
    timeline: SmfTimeline,
    data: Option<(u16, Vec<u8>, usize, i64)>,
    pending: std::collections::VecDeque<live::Item>,
    finished: bool,
    failed: bool,
}
impl<R: std::io::Read + std::io::Seek> SmfReader<R> {
    pub fn new(
        reader: smf::MergedReader<R>,
        stream: String,
        sequence: Option<u16>,
        limit: usize,
    ) -> io::Result<Self> {
        let timeline = SmfTimeline::new(reader.header.clone(), stream, sequence, limit)?;
        Ok(Self {
            reader,
            timeline,
            data: None,
            pending: std::collections::VecDeque::new(),
            finished: false,
            failed: false,
        })
    }
    pub fn clock(&self) -> &Clock {
        self.timeline.clock()
    }
    pub fn origin(&self) -> Timestamp {
        self.timeline.origin()
    }
    pub fn header(&self) -> &smf::Header {
        &self.reader.header
    }
    fn next_item(&mut self) -> io::Result<Option<live::Item>> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(Some(item));
            }
            if let Some((track, data, offset, time)) = &mut self.data {
                let end = (*offset + 256).min(data.len());
                self.pending.extend(self.timeline.feed_chunk(
                    *track,
                    &data[*offset..end],
                    *time,
                )?);
                *offset = end;
                if end == data.len() {
                    self.data = None;
                }
                continue;
            }
            if self.finished {
                return Ok(None);
            }
            match self.reader.next() {
                Some(record) => {
                    let record = record?;
                    let (data, time) = self.timeline.begin_record(&record)?;
                    self.data = Some((record.track, data, 0, time));
                }
                None => {
                    self.finished = true;
                    self.pending.extend(self.timeline.finish());
                }
            }
        }
    }
}
impl<R: std::io::Read + std::io::Seek> Iterator for SmfReader<R> {
    type Item = io::Result<live::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_item() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub struct ClipReader<R> {
    reader: clip::Reader<R>,
    timeline: ClipTimeline,
    pending: std::collections::VecDeque<live::Item>,
    finished: bool,
    failed: bool,
}
impl<R: std::io::Read> ClipReader<R> {
    pub fn new(
        reader: clip::Reader<R>,
        stream: String,
        include_configuration: bool,
    ) -> io::Result<Self> {
        let mut result = Self {
            reader,
            timeline: ClipTimeline::new(stream, include_configuration)?,
            pending: std::collections::VecDeque::new(),
            finished: false,
            failed: false,
        };
        while result.timeline.origin().is_none() {
            let entry = result
                .reader
                .next()
                .ok_or_else(|| invalid(0, "missing clip clock/start"))??;
            result.pending.extend(result.timeline.feed(&entry)?);
        }
        Ok(result)
    }
    pub fn clock(&self) -> &Clock {
        self.timeline.clock().expect("primed file clock")
    }
    pub fn origin(&self) -> Timestamp {
        self.timeline.origin().expect("primed origin")
    }
    fn next_item(&mut self) -> io::Result<Option<live::Item>> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(Some(item));
            }
            if self.finished {
                return Ok(None);
            }
            match self.reader.next() {
                Some(entry) => self.pending.extend(self.timeline.feed(&entry?)?),
                None => {
                    self.finished = true;
                    self.pending.extend(self.timeline.finish());
                }
            }
        }
    }
}
impl<R: std::io::Read> Iterator for ClipReader<R> {
    type Item = io::Result<live::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_item() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
