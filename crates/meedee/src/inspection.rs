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
#[derive(Debug, Default)]
pub struct Report {
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

/// Inspect from the current position through EOF, stopping at the first error.
/// Invalid data is reported; I/O errors propagate. Unknown content requires
/// explicit preservation and is counted as opaque, without validating semantics.
pub fn inspect<R: Read>(
    source: R,
    format: Format,
    payload_limit: usize,
    preserve_unknown: bool,
) -> io::Result<Report> {
    if payload_limit == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "payload limit must be positive",
        ));
    }
    let mut report = Report::default();
    let result = report.scan(
        BufReader::new(source),
        format,
        payload_limit,
        preserve_unknown,
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
                            self.untimed_events += u64::from(e.timestamp.is_none());
                            match &e.message {
                                events::Payload::Midi1(v) => self.count(midi1_kind(v)),
                                events::Payload::Ump(v) => self.count_ump(v),
                                events::Payload::Command(v) => self.count(command_kind(v)),
                            }
                        }
                        archive::Record::Gap { lost, .. } => {
                            self.gap_records += 1;
                            self.lost_events += u128::from(lost);
                        }
                        archive::Record::Discontinuity { .. } => self.discontinuities += 1,
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
                    self.untimed_events += 1;
                }
            }
            Format::Clip => {
                for entry in clip::Reader::new(source, preserve)? {
                    let entry = entry?;
                    self.records += 1;
                    self.count_ump(&entry.message);
                    self.untimed_events += u64::from(entry.tick.is_none());
                }
            }
        }
        Ok(())
    }

    fn count(&mut self, kind: &'static str) {
        *self.message_counts.entry(kind).or_default() += 1;
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
