//! Read-only file inspection using the canonical streaming readers.
use crate::{archive, clip, events, messages, smf, syx, ump};
use std::collections::BTreeMap;
use std::io::{self, BufReader, Read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Archive,
    Smf,
    Syx,
    Clip,
}

/// Counts describe successfully decoded records before the first problem.
/// Completion does not certify lossless capture, device support or safe replay.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub timing: crate::inspection_timing::Timing,
    pub complete: bool,
    pub problem: Option<String>,
    pub records: u64,
    pub message_counts: BTreeMap<&'static str, u64>,
    pub clocks: Vec<events::Clock>,
    /// Only archive Stream descriptors are retained here, in file order.
    pub streams: Vec<archive::Record>,
    pub smf_header: Option<smf::Header>,
    /// SMF record counts per zero-based track, excluding inter-track chunks.
    pub track_records: BTreeMap<u16, u64>,
    pub gap_records: u64,
    pub lost_events: u128,
    pub discontinuities: u64,
    pub untimed_events: u64,
    pub opaque_records: u64,
}

/// Caller-driven inspection of one live source. It owns no transport or thread.
/// Snapshots are ongoing observations, never certificates of file completion.
pub struct Observer {
    report: Report,
    stream: String,
    clock: events::Clock,
    detail_limit: usize,
}
impl Observer {
    pub fn snapshot(&self) -> Report {
        self.report.clone()
    }
    pub fn new(stream: String, clock: events::Clock, detail_limit: usize) -> io::Result<Self> {
        if stream.is_empty() || detail_limit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stream and positive detail limit required",
            ));
        }
        let report = Report {
            clocks: vec![clock.clone()],
            streams: vec![archive::Record::Stream {
                id: stream.clone(),
                label: String::new(),
                clock_identity: Some(clock.identity().into()),
            }],
            ..Report::default()
        };
        Ok(Self {
            report,
            stream,
            clock,
            detail_limit,
        })
    }
    pub fn feed(&mut self, item: &crate::live::Item) -> io::Result<()> {
        if let crate::live::Item::Event(e) = item {
            e.validate()?;
        }
        let stream = match item {
            crate::live::Item::Event(e) => &e.stream,
            crate::live::Item::Diagnostic(d) => &d.stream,
        };
        if stream != &self.stream {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "observer belongs to one source",
            ));
        }
        self.report.records += 1;
        match item {
            crate::live::Item::Event(e) => {
                self.report
                    .observe_event(e, Some(self.clock.identity()), self.detail_limit)
            }
            crate::live::Item::Diagnostic(d) => {
                let kind = match d.kind {
                    crate::live::DiagnosticKind::Loss => {
                        self.report.gap_records += 1;
                        self.report.lost_events += u128::from(d.lost);
                        "gap"
                    }
                    crate::live::DiagnosticKind::ClockChange => {
                        self.report.discontinuities += 1;
                        "clock_change"
                    }
                    crate::live::DiagnosticKind::Malformed => "malformed",
                    crate::live::DiagnosticKind::Truncated => "truncated",
                };
                self.report.timing.boundary(
                    &self.stream,
                    self.report.records - 1,
                    kind,
                    self.detail_limit,
                );
            }
        }
        Ok(())
    }
    /// Asynchronous counters can overlap sequence gaps. Reset without adding
    /// their counts to sequence-derived lost_events a second time.
    pub fn loss_counter_changed(&mut self) {
        self.report.timing.boundary(
            &self.stream,
            self.report.records,
            "loss_counter",
            self.detail_limit,
        );
    }
}

/// Inspect from the current position through EOF, stopping at the first error.
/// Invalid data is reported; I/O errors propagate. Unknown content requires
/// explicit preservation and is counted as opaque, without validating semantics.
pub fn inspect<R: Read>(
    source: R,
    format: Format,
    payload_limit: usize,
    preserve_unknown: bool,
    detail_limit: usize,
) -> io::Result<Report> {
    if payload_limit == 0 || detail_limit == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "payload and detail limits must be positive",
        ));
    }
    let mut report = Report::default();
    let result = report.scan(
        BufReader::new(source),
        format,
        payload_limit,
        preserve_unknown,
        detail_limit,
    );
    match result {
        Ok(()) => report.complete = true,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidData | io::ErrorKind::Unsupported
            ) =>
        {
            report.problem = Some(e.to_string());
        }
        Err(e) => return Err(e),
    }
    Ok(report)
}

impl Report {
    fn scan<R: Read>(
        &mut self,
        source: R,
        format: Format,
        limit: usize,
        preserve: bool,
        detail_limit: usize,
    ) -> io::Result<()> {
        match format {
            Format::Archive => {
                let reader = archive::Reader::new(source, limit, false, preserve)?;
                for record in reader {
                    let record = record?;
                    self.records += 1;
                    match record {
                        archive::Record::Clock(c) => self.clocks.push(c),
                        r @ archive::Record::Stream { .. } => self.streams.push(r),
                        archive::Record::Event(e) => {
                            let clock = self.streams.iter().find_map(|r| match r {
                                archive::Record::Stream {
                                    id, clock_identity, ..
                                } if id == &e.stream => clock_identity.clone(),
                                _ => None,
                            });
                            self.observe_event(&e, clock.as_deref(), detail_limit);
                        }
                        archive::Record::Gap { stream, lost, .. } => {
                            self.timing
                                .boundary(&stream, self.records - 1, "gap", detail_limit);
                            self.gap_records += 1;
                            self.lost_events += u128::from(lost);
                        }
                        archive::Record::Discontinuity { stream, .. } => {
                            self.discontinuities += 1;
                            self.timing.boundary(
                                &stream,
                                self.records - 1,
                                "discontinuity",
                                detail_limit,
                            );
                        }
                        archive::Record::Opaque { .. } => self.opaque_records += 1,
                        archive::Record::Metadata { .. } => {}
                    }
                }
            }
            Format::Smf => {
                let reader = smf::Reader::new(source, limit)?;
                self.smf_header = Some(reader.header.clone());
                for event in reader {
                    let event = event?;
                    self.records += 1;
                    if !matches!(event.record, smf::Record::Chunk { .. }) {
                        self.timing.observe(
                            &format!("track:{}", event.track),
                            Some("smf_ticks"),
                            Some(i128::from(event.tick)),
                            self.records - 1,
                            detail_limit,
                        );
                        *self.track_records.entry(event.track).or_default() += 1;
                    }
                    match event.record {
                        smf::Record::Message(v) => self.count(midi1_kind(&v)),
                        smf::Record::SysEx { .. } => self.count("SysExFragment"),
                        smf::Record::Chunk { .. } => self.opaque_records += 1,
                        smf::Record::Meta { .. } => {}
                    }
                }
            }
            Format::Syx => {
                for record in syx::Reader::new(source, limit)? {
                    record?;
                    self.records += 1;
                    self.count("SysEx");
                    self.timing
                        .observe("syx", None, None, self.records - 1, detail_limit);
                    self.untimed_events += 1;
                }
            }
            Format::Clip => {
                for entry in clip::Reader::new(source, preserve)? {
                    let entry = entry?;
                    self.records += 1;
                    self.count_ump(&entry.message);
                    self.timing.observe(
                        "clip",
                        Some("clip_ticks"),
                        entry.tick.map(i128::from),
                        self.records - 1,
                        detail_limit,
                    );
                    self.untimed_events += u64::from(entry.tick.is_none());
                }
            }
        }
        Ok(())
    }

    fn count(&mut self, kind: &'static str) {
        *self.message_counts.entry(kind).or_default() += 1;
    }

    fn observe_event(
        &mut self,
        e: &events::Event,
        default_clock: Option<&str>,
        detail_limit: usize,
    ) {
        let clock = e
            .timestamp
            .as_ref()
            .map(|t| t.clock.identity())
            .or(default_clock);
        self.timing.observe(
            &e.stream,
            clock,
            e.timestamp.as_ref().map(|t| i128::from(t.ticks)),
            self.records - 1,
            detail_limit,
        );
        self.untimed_events += u64::from(e.timestamp.is_none());
        match &e.message {
            events::Payload::Midi1(v) => self.count(midi1_kind(v)),
            events::Payload::Ump(v) => self.count_ump(v),
            events::Payload::Command(v) => self.count(command_kind(v)),
        }
    }

    fn count_ump(&mut self, value: &ump::UmpMessage) {
        if matches!(value, ump::UmpMessage::Opaque(_)) {
            self.opaque_records += 1;
        }
        // UMP counts are packet counts, including clock/configuration packets.
        // Classify by the wire family without copying arbitrary payloads.
        let words = ump::encode(value);
        self.count(match words[0] >> 28 {
            0 => "UMP.Utility",
            1 => "UMP.System",
            2 => "UMP.Midi1ChannelVoice",
            3 => "UMP.Data64",
            4 => "UMP.Midi2ChannelVoice",
            5 => "UMP.Data128",
            13 => "UMP.FlexData",
            15 => "UMP.Stream",
            _ => "UMP.Opaque",
        });
    }
}

fn midi1_kind(value: &messages::AnyMessage) -> &'static str {
    use messages::AnyMessage::*;
    match value {
        NoteOn(_) => "NoteOn",
        NoteOff(_) => "NoteOff",
        PolyPressure(_) => "PolyPressure",
        ControlChange(_) => "ControlChange",
        ProgramChange(_) => "ProgramChange",
        ChannelPressure(_) => "ChannelPressure",
        PitchBend(_) => "PitchBend",
        QuarterFrame(_) => "QuarterFrame",
        SongPosition(_) => "SongPosition",
        SongSelect(_) => "SongSelect",
        TuneRequest(_) => "TuneRequest",
        TimingClock(_) => "TimingClock",
        Start(_) => "Start",
        Continue(_) => "Continue",
        Stop(_) => "Stop",
        ActiveSensing(_) => "ActiveSensing",
        SystemReset(_) => "SystemReset",
        SysEx(_) => "SysEx",
    }
}

fn command_kind(value: &messages::AnyCommand) -> &'static str {
    use messages::AnyCommand::*;
    match value {
        ControlChange14(_) => "ControlChange14",
        RpnSet(_) => "RpnSet",
        NrpnSet(_) => "NrpnSet",
        RpnIncrement(_) => "RpnIncrement",
        RpnDecrement(_) => "RpnDecrement",
        NrpnIncrement(_) => "NrpnIncrement",
        NrpnDecrement(_) => "NrpnDecrement",
        BankProgramSelect(_) => "BankProgramSelect",
        ZoneConfiguration(_) => "ZoneConfiguration",
    }
}
