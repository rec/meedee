//! Read-only file inspection using the canonical streaming readers.
pub use crate::comparison::{
    compare, Difference, Options as ComparisonOptions, Report as ComparisonReport,
};
pub use crate::inspection_timing::{TimingDiagnostic, TimingSegment};
use crate::messages::Message;
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
    timing: crate::inspection_timing::Timing,
    notes: crate::notes::Tracker,
    complete: bool,
    problem: Option<String>,
    records: u64,
    message_counts: BTreeMap<&'static str, u64>,
    clocks: Vec<events::Clock>,
    /// Only archive Stream descriptors are retained here, in file order.
    streams: Vec<archive::Record>,
    smf_header: Option<smf::Header>,
    /// SMF record counts per zero-based track, excluding inter-track chunks.
    track_records: BTreeMap<u16, u64>,
    gap_records: u64,
    lost_events: u128,
    discontinuities: u64,
    untimed_events: u64,
    opaque_records: u64,
}

impl Report {
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn problem(&self) -> &Option<String> {
        &self.problem
    }
    pub fn records(&self) -> u64 {
        self.records
    }
    pub fn message_counts(&self) -> &BTreeMap<&'static str, u64> {
        &self.message_counts
    }
    pub fn clocks(&self) -> &Vec<events::Clock> {
        &self.clocks
    }
    pub fn streams(&self) -> &Vec<archive::Record> {
        &self.streams
    }
    pub fn smf_header(&self) -> &Option<smf::Header> {
        &self.smf_header
    }
    pub fn track_records(&self) -> &BTreeMap<u16, u64> {
        &self.track_records
    }
    pub fn gap_records(&self) -> u64 {
        self.gap_records
    }
    pub fn lost_events(&self) -> u128 {
        self.lost_events
    }
    pub fn discontinuities(&self) -> u64 {
        self.discontinuities
    }
    pub fn untimed_events(&self) -> u64 {
        self.untimed_events
    }
    pub fn opaque_records(&self) -> u64 {
        self.opaque_records
    }
    pub fn note_state(&self) -> crate::notes::Report {
        self.notes.snapshot()
    }
    pub fn source_counts(&self) -> &BTreeMap<String, u64> {
        &self.timing.source_counts
    }
    pub fn timing_segments(&self) -> &[TimingSegment] {
        &self.timing.segments
    }
    pub fn timing_diagnostics(&self) -> &[TimingDiagnostic] {
        &self.timing.diagnostics
    }
    pub fn timestamp_regressions(&self) -> u64 {
        self.timing.regressions
    }
    pub fn omitted_segments(&self) -> u64 {
        self.timing.omitted_segments
    }
    pub fn omitted_diagnostics(&self) -> u64 {
        self.timing.omitted_diagnostics
    }
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
    /// A bounded projection for live rows; avoids copying unrelated history.
    pub fn channel_states(
        &self,
        group: Option<u8>,
        channel: u8,
    ) -> Vec<crate::notes::ChannelState> {
        self.report
            .notes
            .channel_states(&self.stream, group, channel)
    }
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
        if let crate::live::Item::Message(e) = item {
            if e.stream().is_none() {
                return Err(crate::file_io::invalid(
                    0,
                    "observation requires a source identity",
                ));
            }
        }
        let stream = match item {
            crate::live::Item::Message(e) => e.stream().unwrap_or_default(),
            crate::live::Item::Diagnostic(d) => &d.stream,
        };
        if stream != self.stream {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "observer belongs to one source",
            ));
        }
        self.report.records += 1;
        match item {
            crate::live::Item::Message(e) => {
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
                self.report.notes.boundary(&self.stream, self.detail_limit);
            }
        }
        Ok(())
    }
    /// Asynchronous counters can overlap sequence gaps. Reset without adding
    /// their counts to sequence-derived lost_events a second time.
    pub fn loss_counter_changed(&mut self) {
        self.report.notes.boundary(&self.stream, self.detail_limit);
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
                                } if id == e.stream().unwrap_or_default() => clock_identity.clone(),
                                _ => None,
                            });
                            self.observe_event(&e, clock.as_deref(), detail_limit);
                        }
                        archive::Record::Gap { stream, lost, .. } => {
                            self.notes.boundary(&stream, detail_limit);
                            self.timing
                                .boundary(&stream, self.records - 1, "gap", detail_limit);
                            self.gap_records += 1;
                            self.lost_events += u128::from(lost);
                        }
                        archive::Record::Discontinuity { stream, .. } => {
                            self.notes.boundary(&stream, detail_limit);
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
                        smf::Record::Message(v) => {
                            self.count(midi1_kind(&v));
                            self.notes.feed(
                                &format!("track:{}", event.track),
                                &events::Payload::Midi1(v),
                                Some("smf_ticks"),
                                detail_limit,
                            );
                        }
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
                    self.notes.feed(
                        "clip",
                        &events::Payload::Ump(entry.message.clone()),
                        Some("clip_ticks"),
                        detail_limit,
                    );
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
        e: &events::Payload,
        default_clock: Option<&str>,
        detail_limit: usize,
    ) {
        let clock = e.timestamp().map(|t| t.clock.identity()).or(default_clock);
        self.notes
            .feed(e.stream().unwrap_or_default(), e, clock, detail_limit);
        self.timing.observe(
            e.stream().unwrap_or_default(),
            clock,
            e.timestamp().map(|t| i128::from(t.ticks)),
            self.records - 1,
            detail_limit,
        );
        self.untimed_events += u64::from(e.timestamp().is_none());
        match e {
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

/// File inspection with the same limits and preservation defaults as Python.
#[derive(Clone, Debug)]
pub struct Options {
    pub payload_limit: usize,
    pub preserve_unknown: bool,
    pub detail_limit: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            payload_limit: 1_048_576,
            preserve_unknown: false,
            detail_limit: 128,
        }
    }
}
pub fn inspect_file(
    path: impl AsRef<std::path::Path>,
    format: Format,
    options: &Options,
) -> io::Result<Report> {
    inspect(
        std::fs::File::open(path)?,
        format,
        options.payload_limit,
        options.preserve_unknown,
        options.detail_limit,
    )
}
pub fn compare_files(
    left: impl AsRef<std::path::Path>,
    right: impl AsRef<std::path::Path>,
    format: Format,
    options: &ComparisonOptions,
) -> io::Result<ComparisonReport> {
    compare(
        std::fs::File::open(left)?,
        std::fs::File::open(right)?,
        format,
        options,
    )
}

/// Selection applies after interpretation. Diagnostics and loss are never filtered.
#[derive(Clone, Debug)]
pub struct MonitorOptions {
    pub protocol: crate::transport::Protocol,
    pub types: Vec<String>,
    pub channel: Option<u8>,
    pub group: Option<u8>,
    pub detail_limit: usize,
}
impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            protocol: crate::transport::Protocol::Midi1,
            types: Vec::new(),
            channel: None,
            group: None,
            detail_limit: 128,
        }
    }
}

/// An ephemeral observation. The monitor retains no message payload history.
#[derive(Clone, Debug)]
pub enum MonitorEntry {
    Message {
        message: Box<events::Payload>,
        observations: Vec<events::Observation>,
        note_state: Vec<crate::notes::ChannelState>,
    },
    Diagnostic(crate::live::Diagnostic),
    LossCounter(u64),
}
#[derive(Clone, Debug)]
pub struct MonitorReport {
    pub inspection: Report,
    pub filtered: u64,
    pub diagnostics: u64,
    pub transport_drops: u64,
    pub interrupted: bool,
    pub delivery: Option<crate::metrics::DeliveryReport>,
}

/// Passive interpretation of one source, shared by live drivers and offline callers.
pub struct Monitor {
    observer: Observer,
    options: MonitorOptions,
    controls: Option<crate::controls::EventInterpreter>,
    velocity: Option<crate::controls::EventVelocityInterpreter>,
    filtered: u64,
    diagnostics: u64,
    transport_drops: u64,
    started: bool,
    interrupted: bool,
    delivery: Option<crate::metrics::DeliveryReport>,
}
impl Monitor {
    pub fn new(stream: String, clock: events::Clock, options: MonitorOptions) -> io::Result<Self> {
        if [options.channel, options.group]
            .into_iter()
            .flatten()
            .any(|v| !(1..=16).contains(&v))
            || options.protocol == crate::transport::Protocol::Midi1 && options.group.is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "valid channel and UMP-only group filter required",
            ));
        }
        let interpret =
            options.protocol == crate::transport::Protocol::Midi1 || options.group.is_some();
        Ok(Self {
            observer: Observer::new(stream, clock, options.detail_limit)?,
            controls: interpret
                .then(|| crate::controls::EventInterpreter::new(options.group))
                .transpose()?,
            velocity: interpret
                .then(|| crate::controls::EventVelocityInterpreter::new(options.group))
                .transpose()?,
            options,
            filtered: 0,
            diagnostics: 0,
            transport_drops: 0,
            started: false,
            interrupted: false,
            delivery: None,
        })
    }
    pub fn snapshot(&self) -> MonitorReport {
        MonitorReport {
            inspection: self.observer.snapshot(),
            filtered: self.filtered,
            diagnostics: self.diagnostics,
            transport_drops: self.transport_drops,
            interrupted: self.interrupted,
            delivery: self.delivery.clone(),
        }
    }
    pub fn feed(&mut self, item: &events::InputItem) -> io::Result<Option<MonitorEntry>> {
        use events::View;
        self.observer.feed(item)?;
        match item {
            events::InputItem::Diagnostic(d) => {
                self.reset_views();
                self.diagnostics += 1;
                Ok(Some(MonitorEntry::Diagnostic(d.clone())))
            }
            events::InputItem::Message(message) => {
                let mut observations = Vec::new();
                if let Some(view) = &mut self.controls {
                    observations.extend(view.feed(message, None)?);
                }
                if let Some(view) = &mut self.velocity {
                    observations.extend(view.feed(message, None)?);
                }
                let kind = match message.as_ref() {
                    events::Payload::Midi1(v) => midi1_kind(v),
                    events::Payload::Ump(v) => v.kind(),
                    events::Payload::Command(v) => command_kind(v),
                };
                let inner = match message.as_ref() {
                    events::Payload::Ump(ump::UmpMessage::Midi1(v)) => midi1_kind(v.message()),
                    _ => kind,
                };
                let (channel, group) = match message.as_ref() {
                    events::Payload::Ump(ump::UmpMessage::FlexData(v)) => {
                        (v.channel(), Some(v.group()))
                    }
                    events::Payload::Ump(ump::UmpMessage::Opaque(_)) => (None, None),
                    _ => {
                        let info = crate::routing::classify(message).ok();
                        (
                            info.as_ref().and_then(|v| v.channel),
                            info.as_ref().and_then(|v| v.group),
                        )
                    }
                };
                if (!self.options.types.is_empty()
                    && !self.options.types.iter().any(|v| v == kind || v == inner))
                    || self.options.channel.is_some_and(|v| Some(v) != channel)
                    || self.options.group.is_some_and(|v| Some(v) != group)
                {
                    self.filtered += 1;
                    return Ok(None);
                }
                let note_state = channel
                    .map(|c| self.observer.channel_states(group, c))
                    .unwrap_or_default();
                Ok(Some(MonitorEntry::Message {
                    message: message.clone(),
                    observations,
                    note_state,
                }))
            }
        }
    }
    pub fn counter(&mut self, total: u64) -> Option<MonitorEntry> {
        if total <= self.transport_drops {
            return None;
        }
        self.transport_drops = total;
        self.observer.loss_counter_changed();
        self.reset_views();
        Some(MonitorEntry::LossCounter(total))
    }
    fn reset_views(&mut self) {
        use events::View;
        if let Some(view) = &mut self.controls {
            view.reset();
        }
        if let Some(view) = &mut self.velocity {
            view.reset();
        }
    }
}

/// Observe through a caller-owned connection. Known loss resets interpretation;
/// terminal failures return an error, with progress retained in monitor.snapshot().
pub fn run(
    monitor: &mut Monitor,
    receiver: &mut crate::midi::Input,
    callback: impl FnMut(&MonitorEntry) -> crate::midi::Result<()>,
    duration: Option<std::time::Duration>,
    cancel: Option<&events::Cancellation>,
    metrics: bool,
) -> crate::midi::Result<MonitorReport> {
    run_with(
        monitor,
        receiver,
        callback,
        &mut events::MonotonicClock::default(),
        duration,
        cancel.unwrap_or(&events::Cancellation::default()),
        metrics,
    )
}

#[doc(hidden)]
pub fn run_with(
    monitor: &mut Monitor,
    receiver: &mut impl crate::drivers::Receiver,
    mut callback: impl FnMut(&MonitorEntry) -> crate::midi::Result<()>,
    clock: &mut impl events::PlaybackClock,
    duration: Option<std::time::Duration>,
    cancel: &events::Cancellation,
    metrics: bool,
) -> crate::midi::Result<MonitorReport> {
    use std::time::Duration;
    if monitor.started || monitor.observer.report.records != 0 || monitor.transport_drops != 0 {
        return Err(crate::drivers::invalid(
            "monitor driver requires a fresh monitor",
        ));
    }
    monitor.started = true;
    let start = clock.now_ns();
    let mut delivery = metrics.then(|| crate::metrics::Delivery::new(start));
    let result = (|| -> crate::midi::Result<()> {
        loop {
            let elapsed = Duration::from_nanos(clock.now_ns().saturating_sub(start));
            if duration.is_some_and(|d| elapsed >= d) {
                break;
            }
            if cancel.cancelled() {
                monitor.interrupted = true;
                break;
            }
            if let Some(entry) = monitor.counter(receiver.dropped_packets()?) {
                callback(&entry)?;
            }
            receiver.check_monitor()?;
            let wait = duration
                .map(|d| d.saturating_sub(elapsed))
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(50));
            if let Some(item) = receiver.receive(wait)? {
                if matches!(item, events::InputItem::Message(_)) {
                    if let Some(delivery) = &mut delivery {
                        delivery.observe(clock.now_ns())?;
                    }
                }
                if let Some(entry) = monitor.feed(&item)? {
                    callback(&entry)?;
                }
            }
        }
        Ok(())
    })();
    let final_counter = receiver
        .dropped_packets()
        .map(|total| monitor.counter(total));
    if let Some(delivery) = delivery {
        monitor.delivery = Some(delivery.snapshot(clock.now_ns())?);
    }
    if let Err(error) = result {
        if crate::drivers::interrupted(&error) {
            monitor.interrupted = true;
        } else {
            return Err(error);
        }
    }
    if let Some(entry) = final_counter? {
        callback(&entry)?;
    }
    Ok(monitor.snapshot())
}

#[cfg(test)]
mod monitor_tests {
    use super::*;
    use crate::{
        drivers::{
            tests::{Clock, Input},
            Receiver,
        },
        live::Item,
        midi,
    };
    use std::{cell::Cell, time::Duration};

    #[test]
    fn live_monitor_continues_known_loss_and_retains_progress_on_terminal_failure() {
        struct Source {
            input: Input,
            failed: Cell<bool>,
        }
        impl Receiver for Source {
            fn receive(&mut self, timeout: Duration) -> midi::Result<Option<Item>> {
                let item = self.input.receive(timeout)?;
                self.failed.set(true);
                Ok(item)
            }
            fn check(&self) -> midi::Result<()> {
                if self.failed.get() {
                    Err(crate::drivers::invalid("receiver stopped"))
                } else {
                    Ok(())
                }
            }
            fn dropped_packets(&self) -> midi::Result<u64> {
                Ok(2)
            }
        }
        let mut clock = Clock::default();
        let mut source = Source {
            input: Input {
                clock: clock.clone(),
                items: [crate::drivers::tests::sysex(&[1])].into(),
                lost: false,
            },
            failed: Cell::new(false),
        };
        let mut monitor = Monitor::new(
            "s".into(),
            events::Clock::new("c".into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
            MonitorOptions::default(),
        )
        .unwrap();
        let mut entries = Vec::new();
        let result = run_with(
            &mut monitor,
            &mut source,
            |entry| {
                entries.push(entry.clone());
                Ok(())
            },
            &mut clock,
            Some(Duration::from_secs(1)),
            &events::Cancellation::default(),
            true,
        );
        assert!(result.is_err());
        assert!(matches!(
            entries.as_slice(),
            [MonitorEntry::LossCounter(2), MonitorEntry::Message { .. }]
        ));
        let report = monitor.snapshot();
        assert_eq!(report.inspection.records(), 1);
        assert_eq!(report.transport_drops, 2);
        assert_eq!(report.delivery.unwrap().events(), 1);
    }

    #[test]
    fn filtering_preserves_note_state_and_counter_resets_interpretation() {
        let mut monitor = Monitor::new(
            "s".into(),
            events::Clock::new("c".into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
            MonitorOptions {
                types: vec!["NoteOff".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let on = messages::NoteOn::new(60)
            .unwrap()
            .with_channel(1)
            .unwrap()
            .with_stream("s")
            .unwrap();
        let off = messages::NoteOff::new(60)
            .unwrap()
            .with_channel(1)
            .unwrap()
            .with_stream("s")
            .unwrap();
        assert!(monitor
            .feed(&Item::Message(Box::new(events::Payload::Midi1(
                messages::AnyMessage::NoteOn(on)
            ))))
            .unwrap()
            .is_none());
        let entry = monitor
            .feed(&Item::Message(Box::new(events::Payload::Midi1(
                messages::AnyMessage::NoteOff(off),
            ))))
            .unwrap()
            .unwrap();
        let MonitorEntry::Message { note_state, .. } = entry else {
            panic!("expected note")
        };
        assert_eq!(note_state[0].notes[0].released, 1);
        assert!(monitor.counter(2).is_some());
        assert!(monitor.counter(2).is_none());
        let report = monitor.snapshot();
        assert_eq!(report.filtered, 1);
        assert!(report.inspection.note_state().channels[0].uncertain);
    }
}
