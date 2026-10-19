//! Streaming same-format record comparison. No alignment or resynchronization.
use crate::{archive, clip, events, file_io, inspection::Format, smf, syx, ump};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, Read};

#[derive(Debug)]
pub struct Options {
    pub payload_limit: usize,
    pub detail_limit: usize,
    pub preserve_unknown: bool,
    pub ignore_timestamps: bool,
    pub tick_tolerance: u64,
    /// Explicit left-to-right archive identity mappings; values must be unique.
    pub sources: BTreeMap<String, String>,
    pub clocks: BTreeMap<String, String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            payload_limit: file_io::DEFAULT_LIMIT,
            detail_limit: 128,
            preserve_unknown: false,
            ignore_timestamps: false,
            tick_tolerance: 0,
            sources: BTreeMap::new(),
            clocks: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Difference {
    /// None identifies the SMF header; records otherwise use zero-based order.
    pub record: Option<u64>,
    pub reason: &'static str,
}
#[derive(Debug, Default)]
pub struct Report {
    pub equal: bool,
    pub left_complete: bool,
    pub right_complete: bool,
    pub left_problem: Option<String>,
    pub right_problem: Option<String>,
    pub left_records: u64,
    pub right_records: u64,
    pub differences: u64,
    pub examples: Vec<Difference>,
}

pub fn compare<L: Read, R: Read>(
    left: L,
    right: R,
    format: Format,
    options: &Options,
) -> io::Result<Report> {
    options.validate(format)?;
    let mut report = Report::default();
    let mut left = open(left, format, options, &mut report.left_problem)?;
    let mut right = open(right, format, options, &mut report.right_problem)?;
    if let (Some(Records::Smf(a)), Some(Records::Smf(b))) = (&left, &right) {
        if a.header != b.header {
            report.difference(None, "header", options.detail_limit);
        }
    }
    let mut index = 0;
    loop {
        let a = advance(
            &mut left,
            &mut report.left_problem,
            &mut report.left_records,
        )?;
        let b = advance(
            &mut right,
            &mut report.right_problem,
            &mut report.right_records,
        )?;
        match (a, b) {
            (None, None) => break,
            (Some(a), Some(b)) => {
                if !records_equal(a, b, options)? {
                    report.difference(Some(index), "content_or_timing", options.detail_limit);
                }
            }
            _ if report.left_problem.is_none() && report.right_problem.is_none() => {
                report.difference(Some(index), "length", options.detail_limit)
            }
            _ => {}
        }
        index += 1;
    }
    report.left_complete = report.left_problem.is_none();
    report.right_complete = report.right_problem.is_none();
    report.equal = report.left_complete && report.right_complete && report.differences == 0;
    Ok(report)
}

impl Options {
    fn validate(&self, format: Format) -> io::Result<()> {
        if self.payload_limit == 0
            || self.detail_limit == 0
            || self.ignore_timestamps && self.tick_tolerance != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "positive limits required; tolerance cannot accompany ignored timestamps",
            ));
        }
        if format != Format::Archive && (!self.sources.is_empty() || !self.clocks.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "identity mappings apply only to archives",
            ));
        }
        for map in [&self.sources, &self.clocks] {
            if map.len() > 128
                || map.iter().any(|(k, v)| k.is_empty() || v.is_empty())
                || map.values().collect::<BTreeSet<_>>().len() != map.len()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "identity mappings must be nonempty, one-to-one and at most 128 entries",
                ));
            }
        }
        Ok(())
    }
}
impl Report {
    fn difference(&mut self, record: Option<u64>, reason: &'static str, limit: usize) {
        self.differences += 1;
        if self.examples.len() < limit {
            self.examples.push(Difference { record, reason });
        }
    }
}

enum Records<R> {
    Archive(archive::Reader<BufReader<R>>),
    Smf(smf::Reader<BufReader<R>>),
    Syx(syx::Reader<BufReader<R>>),
    Clip(clip::Reader<BufReader<R>>),
}
enum Record {
    Archive(archive::Record),
    Smf(smf::Event),
    Syx(crate::messages::SysEx),
    Clip(clip::Entry),
}
impl<R: Read> Records<R> {
    fn new(source: R, format: Format, o: &Options) -> io::Result<Self> {
        let source = BufReader::new(source);
        Ok(match format {
            Format::Archive => Self::Archive(archive::Reader::new(
                source,
                o.payload_limit,
                false,
                o.preserve_unknown,
            )?),
            Format::Smf => Self::Smf(smf::Reader::new(source, o.payload_limit)?),
            Format::Syx => Self::Syx(syx::Reader::new(source, o.payload_limit)?),
            Format::Clip => Self::Clip(clip::Reader::new(source, o.preserve_unknown)?),
        })
    }
}
impl<R: Read> Iterator for Records<R> {
    type Item = io::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Archive(r) => r.next().map(|v| v.map(Record::Archive)),
            Self::Smf(r) => r.next().map(|v| v.map(Record::Smf)),
            Self::Syx(r) => r.next().map(|v| v.map(Record::Syx)),
            Self::Clip(r) => r.next().map(|v| v.map(Record::Clip)),
        }
    }
}
fn format_error(e: io::Error, problem: &mut Option<String>) -> io::Result<()> {
    if matches!(
        e.kind(),
        io::ErrorKind::InvalidData | io::ErrorKind::Unsupported
    ) {
        *problem = Some(e.to_string());
        Ok(())
    } else {
        Err(e)
    }
}
fn open<R: Read>(
    source: R,
    format: Format,
    o: &Options,
    problem: &mut Option<String>,
) -> io::Result<Option<Records<R>>> {
    match Records::new(source, format, o) {
        Ok(r) => Ok(Some(r)),
        Err(e) => {
            format_error(e, problem)?;
            Ok(None)
        }
    }
}
fn advance<R: Read>(
    reader: &mut Option<Records<R>>,
    problem: &mut Option<String>,
    count: &mut u64,
) -> io::Result<Option<Record>> {
    match reader.as_mut().and_then(Iterator::next) {
        Some(Ok(r)) => {
            *count += 1;
            Ok(Some(r))
        }
        Some(Err(e)) => {
            *reader = None;
            format_error(e, problem)?;
            Ok(None)
        }
        None => {
            *reader = None;
            Ok(None)
        }
    }
}
fn tick_equal(a: i128, b: i128, o: &Options) -> bool {
    o.ignore_timestamps || a.abs_diff(b) <= u128::from(o.tick_tolerance)
}
fn time_equal(a: &Option<events::Timestamp>, b: &Option<events::Timestamp>, o: &Options) -> bool {
    if o.ignore_timestamps {
        return true;
    }
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.clock == b.clock && tick_equal(i128::from(a.ticks), i128::from(b.ticks), o)
        }
        _ => false,
    }
}
fn map_id(id: &mut String, map: &BTreeMap<String, String>) {
    if let Some(v) = map.get(id) {
        *id = v.clone();
    }
}
fn map_clock(c: &mut events::Clock, o: &Options) -> io::Result<()> {
    if let Some(id) = o.clocks.get(c.identity()) {
        let mut mapped = events::Clock::new(
            id.clone(),
            c.numerator(),
            c.denominator(),
            c.origin().into(),
            c.resolution_ticks(),
        )?;
        if let (Some(b), Some(m)) = (c.backend(), c.mode()) {
            mapped = mapped.with_transport(b, m);
        }
        *c = mapped;
    }
    Ok(())
}
fn map_time(t: &mut Option<events::Timestamp>, o: &Options) -> io::Result<()> {
    if let Some(t) = t {
        map_clock(&mut t.clock, o)?;
    }
    Ok(())
}
fn map_record(r: &mut archive::Record, o: &Options) -> io::Result<()> {
    match r {
        archive::Record::Clock(c) => map_clock(c, o)?,
        archive::Record::Stream {
            id, clock_identity, ..
        } => {
            map_id(id, &o.sources);
            if let Some(c) = clock_identity {
                map_id(c, &o.clocks);
            }
        }
        archive::Record::Event(e) => {
            map_id(&mut e.stream, &o.sources);
            map_time(&mut e.timestamp, o)?;
            if let Some(s) = &mut e.span {
                map_time(&mut s.first_timestamp, o)?;
                map_time(&mut s.last_timestamp, o)?;
            }
        }
        archive::Record::Gap { stream, .. } | archive::Record::Discontinuity { stream, .. } => {
            map_id(stream, &o.sources)
        }
        archive::Record::Metadata {
            stream, timestamp, ..
        } => {
            if let Some(s) = stream {
                map_id(s, &o.sources);
            }
            map_time(timestamp, o)?;
        }
        _ => {}
    }
    Ok(())
}
fn archive_equal(a: &archive::Record, b: &archive::Record, o: &Options) -> bool {
    match (a, b) {
        (archive::Record::Event(a), archive::Record::Event(b)) => {
            a.stream == b.stream
                && a.message == b.message
                && time_equal(&a.timestamp, &b.timestamp, o)
                && match (&a.span, &b.span) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        a.first_packet == b.first_packet
                            && a.last_packet == b.last_packet
                            && time_equal(&a.first_timestamp, &b.first_timestamp, o)
                            && time_equal(&a.last_timestamp, &b.last_timestamp, o)
                    }
                    _ => false,
                }
        }
        (
            archive::Record::Metadata {
                key: ak,
                data: ad,
                stream: as_,
                timestamp: at,
            },
            archive::Record::Metadata {
                key: bk,
                data: bd,
                stream: bs,
                timestamp: bt,
            },
        ) => ak == bk && ad == bd && as_ == bs && time_equal(at, bt, o),
        _ => a == b,
    }
}
fn records_equal(a: Record, b: Record, o: &Options) -> io::Result<bool> {
    Ok(match (a, b) {
        (Record::Archive(mut a), Record::Archive(b)) => {
            map_record(&mut a, o)?;
            archive_equal(&a, &b, o)
        }
        (Record::Smf(a), Record::Smf(b)) => {
            a.track == b.track
                && a.record == b.record
                && tick_equal(i128::from(a.tick), i128::from(b.tick), o)
        }
        (Record::Syx(a), Record::Syx(b)) => a == b,
        (Record::Clip(a), Record::Clip(b)) => {
            let ticks = o.ignore_timestamps
                || match (a.tick, b.tick) {
                    (Some(a), Some(b)) => tick_equal(i128::from(a), i128::from(b), o),
                    (None, None) => true,
                    _ => false,
                };
            let delta = matches!(
                (&a.message, &b.message),
                (
                    ump::UmpMessage::DeltaClockstamp(_),
                    ump::UmpMessage::DeltaClockstamp(_)
                )
            );
            ticks
                && a.sequence == b.sequence
                && (a.message == b.message
                    || delta && (o.ignore_timestamps || o.tick_tolerance > 0))
        }
        _ => false,
    })
}
