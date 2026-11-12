//! Convenient caller-thread output with exact sequence timing and scoped cleanup.
use crate::codecs::{DecodeItem, Decoder};
use crate::events::{Payload, Rational};
use crate::messages::{self, AnyCommand, AnyMessage, Command, Message, PrimitiveMessage};
use crate::messages::{Beats, Note, NoteDuration, Rest};
use crate::playback::{Cancellation, MonotonicClock, PlaybackClock};
use crate::transport::{self, Encoding, Packet, Port};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Duration;

/// Open the first output lazily, or select a unique name with `with_port`.
/// Explicit channels override MIDI 1 message channels; otherwise assigned
/// channels survive and unassigned channels become channel 1. Raw packets and
/// native UMP values retain their addresses. Drop releases confirmed MIDI 1
/// state best-effort; call `close` to receive cleanup errors.
///
/// ```no_run
/// use meedee::messages::{Beats, Note, Rest};
/// use meedee::midi::{Output};
/// use std::time::Duration;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// Output::new().send(Note::new(Duration::from_millis(500), 60)?)?;
/// let mut output = Output::new().with_bpm(120)?;
/// output.send([
///     meedee::midi::OutputValue::from(Note::new(Beats::from(1), 60)?),
///     Note::new(Beats::new(1, 2)?, 62)?.with_velocity(70)?.into(),
///     Rest::new(Beats::from(1)).into(),
/// ])?;
/// output.close()?;
/// # Ok(())
/// # }
/// ```
pub struct Output {
    options: transport::OutputOptions,
    run_in_subprocess: bool,
    subprocess_options: transport::subprocess::SubprocessOptions,
    port: Option<String>,
    channel: Option<u8>,
    bpm: Option<Rational>,
    playback: PlaybackOptions,
    client: Option<transport::Output>,
    connection: Option<Connection>,
    clock: Box<dyn PlaybackClock>,
    state: OutputState,
    closed: bool,
}

/// One element of a heterogeneous output sequence. `From` conversions allow
/// notes, rests, messages, packets, Duration and Beats to be passed directly.
#[derive(Clone, Debug)]
pub enum OutputValue {
    Message(Payload),
    Packet(Packet),
    Note(Note),
    Rest(Rest),
    Wait(NoteDuration),
    Record(crate::archive::Record),
}

type OutputSegment<'a> = (bool, Box<dyn Iterator<Item = io::Result<OutputValue>> + 'a>);

/// Material accepted by Output::send: one value, an iterable of values/records,
/// or a tuple of independent sources sent successively.
pub trait OutputSource<Kind> {
    #[doc(hidden)]
    fn into_output<'a>(self) -> impl Iterator<Item = OutputSegment<'a>>
    where
        Self: 'a;
}
#[doc(hidden)]
pub struct Single;
#[doc(hidden)]
pub struct Sequence;
#[doc(hidden)]
pub struct Sources<K>(std::marker::PhantomData<K>);
impl<T: Into<OutputValue>> OutputSource<Single> for T {
    fn into_output<'a>(self) -> impl Iterator<Item = OutputSegment<'a>>
    where
        Self: 'a,
    {
        std::iter::once((
            false,
            Box::new(std::iter::once(Ok(self.into())))
                as Box<dyn Iterator<Item = io::Result<OutputValue>> + 'a>,
        ))
    }
}
trait OutputItem {
    fn output_item(self) -> io::Result<OutputValue>;
}
impl<T: Into<OutputValue>> OutputItem for T {
    fn output_item(self) -> io::Result<OutputValue> {
        Ok(self.into())
    }
}
impl<T: Into<OutputValue>> OutputItem for io::Result<T> {
    fn output_item(self) -> io::Result<OutputValue> {
        self.map(Into::into)
    }
}
impl<T: OutputItem> OutputItem for crate::events::GapItem<T> {
    fn output_item(self) -> io::Result<OutputValue> {
        match self {
            Self::Item(i) => i.output_item(),
            Self::Gap(gap) => Ok(OutputValue::Wait(gap.into())),
        }
    }
}
impl OutputItem for io::Result<crate::file_io::Record> {
    fn output_item(self) -> io::Result<OutputValue> {
        match self? {
            crate::file_io::Record::Archive(record) => Ok(OutputValue::Record(record)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "use smf::TimedReader for MIDI-file playback",
            )),
        }
    }
}
impl<I: IntoIterator> OutputSource<Sequence> for I
where
    I::Item: OutputItem,
{
    fn into_output<'a>(self) -> impl Iterator<Item = OutputSegment<'a>>
    where
        Self: 'a,
    {
        std::iter::once((
            true,
            Box::new(self.into_iter().map(OutputItem::output_item))
                as Box<dyn Iterator<Item = io::Result<OutputValue>> + 'a>,
        ))
    }
}
impl From<crate::archive::Record> for OutputValue {
    fn from(value: crate::archive::Record) -> Self {
        Self::Record(value)
    }
}
macro_rules! tuple_sources {
    ($($value:ident: $kind:ident),+) => {
        impl<$($value: OutputSource<$kind>, $kind),+> OutputSource<Sources<($($kind,)+)>> for ($($value,)+) {
            #[allow(non_snake_case)]
            fn into_output<'a>(self) -> impl Iterator<Item = OutputSegment<'a>> where Self: 'a {
                let ($($value,)+) = self;
                std::iter::empty()$(.chain($value.into_output()))+
            }
        }
    };
}
tuple_sources!(A: KA, B: KB);
tuple_sources!(A: KA, B: KB, C: KC);
tuple_sources!(A: KA, B: KB, C: KC, D: KD);

/// Persistent options for subsequent Output::send calls.
#[derive(Clone, Default)]
pub struct PlaybackOptions {
    pub origin: Option<crate::events::Timestamp>,
    pub mappings: Vec<crate::playback::Mapping>,
    pub cancel: Cancellation,
    pub allow_rounding: bool,
    pub metrics: bool,
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}
impl Output {
    /// Construction opens no ports and creates no transport workers.
    pub fn new() -> Self {
        Self {
            options: transport::OutputOptions::default(),
            run_in_subprocess: false,
            subprocess_options: Default::default(),
            port: None,
            channel: None,
            bpm: None,
            playback: PlaybackOptions::default(),
            client: None,
            connection: None,
            clock: Box::new(MonotonicClock::default()),
            state: OutputState::default(),
            closed: false,
        }
    }
    pub fn with_playback(mut self, options: PlaybackOptions) -> Self {
        self.playback = options;
        self
    }
    pub fn with_channel(mut self, channel: i64) -> crate::midi::Result<Self> {
        if !(1..=16).contains(&channel) {
            return Err(invalid("channel must be in 1..=16").into());
        }
        self.channel = Some(channel as u8);
        Ok(self)
    }
    pub fn with_bpm(mut self, bpm: impl Into<Rational>) -> crate::midi::Result<Self> {
        let bpm = bpm.into();
        if bpm.numerator() <= 0 {
            return Err(invalid("tempo must be positive").into());
        }
        self.bpm = Some(bpm);
        Ok(self)
    }
    /// Case-sensitive substring or shell pattern (`*`, `?`, `[abc]`, `[!abc]`) matching. Multiple
    /// matching ports are an error, including duplicate names.
    pub fn with_port(mut self, pattern: impl Into<String>) -> crate::midi::Result<Self> {
        if self.connection.is_some() || self.closed {
            return Err(invalid("output is already connected or closed").into());
        }
        let pattern = pattern.into();
        if pattern.is_empty() {
            return Err(invalid("port pattern must not be empty").into());
        }
        self.port = Some(pattern);
        Ok(self)
    }
    pub fn with_run_in_subprocess(mut self, enabled: bool) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure subprocess before discovery or connection").into());
        }
        self.run_in_subprocess = enabled;
        Ok(self)
    }
    pub fn with_subprocess_options(
        mut self,
        options: transport::subprocess::SubprocessOptions,
    ) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure subprocess before discovery or connection").into());
        }
        if options.timeout.is_zero()
            || std::time::Instant::now()
                .checked_add(options.timeout)
                .is_none()
        {
            return Err(invalid("worker timeout must be positive and representable").into());
        }
        self.subprocess_options = options;
        Ok(self)
    }
    /// Select advanced backend/encoding options before discovery or connection.
    pub fn with_transport(
        mut self,
        options: transport::OutputOptions,
    ) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure transport before discovery or connection").into());
        }
        self.options = options;
        Ok(self)
    }
    pub fn ports(&mut self) -> crate::midi::Result<Vec<Port>> {
        transport::ensure_control_thread().map_err(crate::midi::Error::Transport)?;
        self.client()?
            .ports()
            .map_err(crate::midi::Error::Transport)
    }
    /// Connect a port from this output's most recent discovery snapshot.
    pub fn connect(mut self, port: &Port) -> crate::midi::Result<Self> {
        transport::ensure_control_thread().map_err(crate::midi::Error::Transport)?;
        self.client()?;
        let client = self.client.take().expect("initialized output client");
        self.connection =
            Some(Connection::Native(client.connect(port).map_err(|e| {
                io::Error::from(crate::midi::Error::Connect(e))
            })?));
        Ok(self)
    }
    /// Create a virtual output with the same sending, playback and cleanup
    /// lifecycle as a connected physical output.
    pub fn create_virtual(mut self, name: &str) -> crate::midi::Result<Self> {
        transport::ensure_control_thread().map_err(crate::midi::Error::Transport)?;
        self.client()?;
        let client = self.client.take().expect("initialized output client");
        self.connection = Some(Connection::Native(
            client
                .create_virtual(name)
                .map_err(crate::midi::Error::Connect)?,
        ));
        Ok(self)
    }
    /// Send a value or source, preserving relative timestamps within each source.
    /// Ordinary sequences use note/rest durations and delays. Mixing timestamped
    /// and untimestamped values inside one source is rejected. Tuples concatenate
    /// independent sources. Empty sends open no ports. Always returns a report.
    pub fn send<K>(
        &mut self,
        source: impl OutputSource<K>,
    ) -> Result<crate::playback::Report, crate::playback::Failure<crate::midi::Error>> {
        use crate::playback::{Cause, Failure, Planner, Report};
        let mut histogram = self
            .playback
            .metrics
            .then(crate::metrics::Histogram::default);
        let mut report = Report {
            lateness: histogram.as_ref().map(|h| Box::new(h.snapshot())),
            ..Report::default()
        };
        let mut origin_ns = None;
        let mut elapsed = 0u64;
        let result = (|| {
            transport::ensure_control_thread()
                .map_err(|e| Cause::Send(crate::midi::Error::Transport(e)))?;
            if self.closed {
                return Err(Cause::Input(invalid("output is closed")));
            }
            for (timestamps, segment) in source.into_output() {
                if self.playback.cancel.cancelled() {
                    report.cancelled = true;
                    break;
                }
                let mut values = segment.filter_map(|v| match v {
                    Ok(OutputValue::Record(record)) => {
                        replay_message(Ok(record)).map(|v| v.map(OutputValue::Message))
                    }
                    v => Some(v),
                });
                let Some(first) = values.next() else {
                    continue;
                };
                let first = first.map_err(Cause::Input)?;
                let timed = timestamps
                    && matches!(&first, OutputValue::Message(m) if m.timestamp().is_some());
                if timed {
                    let OutputValue::Message(first) = first else {
                        unreachable!()
                    };
                    let origin = self
                        .playback
                        .origin
                        .clone()
                        .or_else(|| first.timestamp().cloned())
                        .expect("timestamped message");
                    let mut planner = Planner::new(
                        origin,
                        self.playback.mappings.clone(),
                        self.playback.allow_rounding,
                    )
                    .map_err(Cause::Input)?;
                    self.prepare_message(first.clone()).map_err(Cause::Input)?;
                    self.open().map_err(|e| Cause::Send(e.into()))?;
                    let start = *origin_ns.get_or_insert_with(|| self.clock.now_ns());
                    let mut duration = 0;
                    for value in std::iter::once(Ok(OutputValue::Message(first))).chain(values) {
                        if self.playback.cancel.cancelled() {
                            report.cancelled = true;
                            break;
                        }
                        let OutputValue::Message(message) = value.map_err(Cause::Input)? else {
                            return Err(Cause::Input(invalid(
                                "cannot mix timestamped and untimestamped values within one source",
                            )));
                        };
                        if message.timestamp().is_none() {
                            return Err(Cause::Input(invalid(
                                "cannot mix timestamped and untimestamped values within one source",
                            )));
                        }
                        let planned = planner.deadline(&message).map_err(Cause::Input)?;
                        duration = planned.delay_ns;
                        let offset = elapsed
                            .checked_add(duration)
                            .ok_or_else(|| Cause::Input(invalid("sequence deadline overflow")))?;
                        let prepared = self.prepare_message(message).map_err(Cause::Input)?;
                        if !self.emit(
                            start,
                            offset,
                            Some(prepared),
                            planned.rounding_error_ns.numerator() != 0,
                            &mut report,
                            &mut histogram,
                        )? {
                            break;
                        }
                    }
                    elapsed = elapsed
                        .checked_add(duration)
                        .ok_or_else(|| Cause::Input(invalid("sequence deadline overflow")))?;
                } else {
                    let mut ordinary = vec![first];
                    for value in values {
                        let value = value.map_err(Cause::Input)?;
                        if timestamps
                            && matches!(&value, OutputValue::Message(m) if m.timestamp().is_some())
                        {
                            return Err(Cause::Input(invalid(
                                "cannot mix timestamped and untimestamped values within one source",
                            )));
                        }
                        ordinary.push(value);
                    }
                    let plan = self.prepare(ordinary).map_err(Cause::Input)?;
                    if plan.iter().any(|(_, p, _)| p.is_some()) {
                        self.open().map_err(|e| Cause::Send(e.into()))?;
                    }
                    let start = *origin_ns.get_or_insert_with(|| self.clock.now_ns());
                    let duration = plan.last().expect("final sequence deadline").0;
                    // Validate the final absolute deadline before this sequence sends.
                    start
                        .checked_add(elapsed)
                        .and_then(|v| v.checked_add(duration))
                        .ok_or_else(|| Cause::Input(invalid("sequence deadline overflow")))?;
                    for (offset, packet, rounded) in plan {
                        let offset = elapsed + offset;
                        if !self.emit(
                            start,
                            offset,
                            packet,
                            rounded,
                            &mut report,
                            &mut histogram,
                        )? {
                            break;
                        }
                    }
                    elapsed += duration;
                }
                if report.cancelled {
                    break;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(report),
            Err(cause) => Err(Failure { report, cause }),
        }
    }
    pub fn closed(&self) -> bool {
        self.closed
            || self
                .client
                .as_ref()
                .is_some_and(|c| c.worker_failure().is_some())
            || matches!(&self.connection, Some(Connection::Native(c)) if c.worker_failure().is_some())
    }
    /// Cleanup covers held note counts, pitch bend, CC1/64/66/67 and channel/poly
    /// pressure changed by confirmed sends. Other controller/program state is
    /// left alone. The transport closes even if a cleanup send fails.
    pub fn close(&mut self) -> crate::midi::Result<()> {
        transport::ensure_control_thread().map_err(crate::midi::Error::Transport)?;
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut result = Ok(());
        if self.connection.is_some() {
            for message in std::mem::take(&mut self.state).cleanup() {
                if let Err(error) = self.transmit(Prepared::Packet(Packet::Bytes(
                    message.encode().expect("cleanup channels are resolved"),
                ))) {
                    result = Err(error);
                    break;
                }
            }
        }
        self.client = None;
        if let Some(connection) = self.connection.take() {
            let closed = connection.close().map_err(transport_error);
            if result.is_ok() {
                result = closed;
            } else if let Err(error) = closed {
                report(&error);
            }
        }
        result.map_err(crate::midi::Error::from)
    }
    fn client(&mut self) -> io::Result<&transport::Output> {
        if self.closed || self.connection.is_some() {
            return Err(invalid("output is connected or closed"));
        }
        if self.client.is_none() {
            self.client = Some(
                if self.run_in_subprocess {
                    transport::Output::new_subprocess(self.options, self.subprocess_options.clone())
                } else {
                    transport::Output::new(self.options)
                }
                .map_err(|e| io::Error::from(crate::midi::Error::Init(e)))?,
            );
        }
        Ok(self.client.as_ref().expect("initialized output client"))
    }
    fn open(&mut self) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "output is closed",
            ));
        }
        if self.connection.is_none() {
            let ports = self.ports()?;
            let selected = select_port(&ports, self.port.as_deref())?;
            let client = self.client.take().expect("discovery initialized client");
            self.connection =
                Some(Connection::Native(client.connect(selected).map_err(
                    |e| io::Error::from(crate::midi::Error::Connect(e)),
                )?));
        }
        Ok(())
    }
    fn prepare(
        &self,
        values: impl IntoIterator<Item = OutputValue>,
    ) -> io::Result<Vec<(u64, Option<Prepared>, bool)>> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "output is closed",
            ));
        }
        let mut elapsed = Rational::from(0);
        let mut plan = Vec::new();
        for value in values {
            let start = nanoseconds(elapsed)?;
            let rounded = elapsed.multiply(1_000_000_000, 1)?.denominator() != 1;
            match value {
                OutputValue::Record(_) => {
                    return Err(invalid("archive record requires source normalization"))
                }
                OutputValue::Note(note) => {
                    elapsed = elapsed.checked_add(note.duration().seconds(self.bpm)?)?;
                    let end = nanoseconds(elapsed)?;
                    let channel = self.channel.or(note.channel()).unwrap_or(1);
                    let attack = messages::NoteOn::new(note.note().into())?
                        .with_velocity(note.velocity().into())?
                        .with_channel(channel.into())?;
                    let release =
                        messages::NoteOff::new(note.note().into())?.with_channel(channel.into())?;
                    plan.push((
                        start,
                        Some(self.prepare_message(Payload::Message(AnyMessage::NoteOn(attack)))?),
                        rounded,
                    ));
                    plan.push((
                        end,
                        Some(self.prepare_message(Payload::Message(AnyMessage::NoteOff(release)))?),
                        elapsed.multiply(1_000_000_000, 1)?.denominator() != 1,
                    ));
                }
                OutputValue::Rest(rest) => {
                    elapsed = elapsed.checked_add(rest.duration().seconds(self.bpm)?)?
                }
                OutputValue::Wait(duration) => {
                    elapsed = elapsed.checked_add(duration.seconds(self.bpm)?)?
                }
                OutputValue::Message(message) => {
                    plan.push((start, Some(self.prepare_message(message)?), rounded))
                }
                OutputValue::Packet(packet) => {
                    transport::validate_packet(self.options.encoding, &packet)
                        .map_err(transport_error)?;
                    plan.push((start, Some(Prepared::Packet(packet)), rounded));
                }
            }
        }
        plan.push((nanoseconds(elapsed)?, None, false));
        Ok(plan)
    }
    fn prepare_message(&self, message: Payload) -> io::Result<Prepared> {
        message.validate()?;
        let message = message.resolve_channel(self.channel)?;
        match message {
            Payload::Message(message) if self.options.encoding == Encoding::Bytes => {
                Ok(Prepared::Packet(Packet::Bytes(message.encode()?)))
            }
            Payload::Message(message) if self.options.encoding == Encoding::Ump => {
                use crate::ump::UmpValue;
                let packet = Packet::Ump(message.encode_words()?);
                transport::validate_packet(self.options.encoding, &packet)
                    .map_err(transport_error)?;
                Ok(Prepared::Packet(packet))
            }
            Payload::Command(command) if self.options.encoding == Encoding::Ump => {
                use crate::ump::UmpValue;
                let packet = Packet::Ump(Payload::Command(command).encode_words()?);
                transport::validate_packet(self.options.encoding, &packet)
                    .map_err(transport_error)?;
                Ok(Prepared::Packet(packet))
            }
            Payload::Command(command) if self.options.encoding == Encoding::Bytes => {
                let packets = command
                    .expand()?
                    .iter()
                    .map(PrimitiveMessage::encode)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Prepared::Command { command, packets })
            }
            Payload::Ump(message) if self.options.encoding == Encoding::Ump => {
                use crate::ump::{PacketValue, UmpValue};
                if matches!(
                    message,
                    PacketValue::Opaque(_)
                        | PacketValue::DeltaClockstamp(_)
                        | PacketValue::DeltaClockstampTicksPerQuarter(_)
                ) || matches!(&message, PacketValue::Stream(m) if matches!(m.status(),32|33))
                {
                    return Err(invalid(
                        "opaque and file-only UMP require explicit raw packets",
                    ));
                }

                let packet = Packet::Ump(message.encode_words()?);
                transport::validate_packet(self.options.encoding, &packet)
                    .map_err(transport_error)?;
                Ok(Prepared::Packet(packet))
            }
            _ => Err(invalid(
                "message and output protocols differ; no automatic translation",
            )),
        }
    }
    fn emit(
        &mut self,
        start: u64,
        offset: u64,
        packet: Option<Prepared>,
        rounded: bool,
        report: &mut crate::playback::Report,
        histogram: &mut Option<crate::metrics::Histogram>,
    ) -> Result<bool, crate::playback::Cause<crate::midi::Error>> {
        use crate::playback::Cause;
        if self.playback.cancel.cancelled() {
            report.cancelled = true;
            return Ok(false);
        }
        let deadline = start
            .checked_add(offset)
            .ok_or_else(|| Cause::Input(invalid("sequence deadline overflow")))?;
        if !self
            .clock
            .wait_until(deadline, &self.playback.cancel)
            .map_err(Cause::Input)?
            || self.playback.cancel.cancelled()
        {
            report.cancelled = true;
            return Ok(false);
        }
        if let Some(packet) = packet {
            let lateness = self.clock.now_ns().saturating_sub(deadline);
            self.transmit(packet).map_err(|e| Cause::Send(e.into()))?;
            report.sent_events += 1;
            report.rounded_events += u64::from(rounded);
            report.max_lateness_ns = report.max_lateness_ns.max(lateness);
            if let Some(histogram) = histogram {
                histogram.observe(lateness).map_err(Cause::Input)?;
                report.lateness = Some(Box::new(histogram.snapshot()));
            }
        }
        Ok(true)
    }
    fn transmit(&mut self, prepared: Prepared) -> io::Result<()> {
        let connection = self.connection.as_mut().expect("connected output");
        match prepared {
            Prepared::Packet(packet) => {
                connection
                    .send(&packet)
                    .map_err(|e| io::Error::from(crate::midi::Error::Send(e)))?;
                if let Packet::Bytes(bytes) = packet {
                    self.state.feed(&bytes)?;
                }
                Ok(())
            }
            Prepared::Command { command, packets } => {
                let result = connection.send_command(&command);
                let completed = match &result {
                    Ok(()) => packets.len(),
                    Err(transport::Error::CompoundSend {
                        completed_messages, ..
                    }) => completed_messages.unwrap_or(0),
                    Err(_) => 0,
                };
                for bytes in packets.iter().take(completed) {
                    self.state.feed(bytes)?;
                }
                result.map_err(|e| io::Error::from(crate::midi::Error::Send(e)))
            }
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if self.client.is_none() && self.connection.is_none() {
            return;
        }
        if let Err(error) = self.close() {
            report(&error);
        }
    }
}

enum Prepared {
    Packet(Packet),
    Command {
        command: AnyCommand,
        packets: Vec<Vec<u8>>,
    },
}
fn replay_message(record: io::Result<crate::archive::Record>) -> Option<io::Result<Payload>> {
    use crate::archive::Record;
    match record {
        Ok(Record::Event(message)) => Some(Ok(*message)),
        Ok(Record::Clock(_) | Record::Stream { .. } | Record::Metadata { .. }) => None,
        Ok(Record::Gap { .. }) => Some(Err(invalid("cannot replay across input loss"))),
        Ok(Record::Discontinuity { .. }) => {
            Some(Err(invalid("cannot replay across a discontinuity")))
        }
        Ok(Record::Opaque { .. }) => Some(Err(invalid("cannot replay an unknown archive record"))),
        Err(error) => Some(Err(error)),
    }
}
enum Connection {
    Native(transport::OutputConnection),
    #[cfg(test)]
    Test(std::rc::Rc<std::cell::RefCell<crate::testing::Output>>),
}
impl Connection {
    fn send(&mut self, packet: &Packet) -> transport::Result<()> {
        match self {
            Self::Native(connection) => connection.send(packet),
            #[cfg(test)]
            Self::Test(connection) => connection.borrow_mut().send(packet.clone()),
        }
    }
    fn send_command(&mut self, command: &AnyCommand) -> transport::Result<()> {
        match self {
            Self::Native(connection) => connection.send_command(command),
            #[cfg(test)]
            Self::Test(connection) => {
                let packets = command.expand().expect("prepared command");
                for (index, message) in packets.iter().enumerate() {
                    connection
                        .borrow_mut()
                        .send(Packet::Bytes(message.encode().expect("resolved command")))
                        .map_err(|cause| transport::Error::CompoundSend {
                            completed_messages: Some(index),
                            total_messages: packets.len(),
                            cause: Box::new(cause),
                        })?;
                }
                Ok(())
            }
        }
    }
    fn close(self) -> transport::Result<()> {
        match self {
            Self::Native(connection) => connection.close().map(drop),
            #[cfg(test)]
            Self::Test(connection) => {
                connection.borrow_mut().close();
                Ok(())
            }
        }
    }
}
#[doc(hidden)]
pub struct OutputState {
    decoder: Decoder,
    held: BTreeMap<(u8, u8), u64>,
    resets: BTreeMap<(u8, u8, u8), AnyMessage>,
}
impl Default for OutputState {
    fn default() -> Self {
        Self {
            decoder: Decoder::new(Decoder::DEFAULT_SYSEX_LIMIT).expect("positive limit"),
            held: BTreeMap::new(),
            resets: BTreeMap::new(),
        }
    }
}
impl OutputState {
    pub fn feed(&mut self, bytes: &[u8]) -> io::Result<()> {
        for item in self.decoder.feed(bytes) {
            let DecodeItem::Message(message) = item else {
                continue;
            };
            use AnyMessage::*;
            match message {
                NoteOn(m) if m.velocity() != 0 => {
                    let count = self
                        .held
                        .entry((m.channel().expect("decoded channel"), m.note()))
                        .or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or_else(|| invalid("held note count overflow"))?;
                }
                NoteOn(m) => self.release(m.channel().expect("decoded channel"), m.note()),
                NoteOff(m) => self.release(m.channel().expect("decoded channel"), m.note()),
                ControlChange(m) => {
                    let channel = m.channel().expect("decoded channel");
                    match m.controller() {
                        120 | 123 => self.held.retain(|(c, _), _| *c != channel),
                        121 => self.resets.retain(|(c, _, _), _| *c != channel),
                        1 | 64 | 66 | 67 => self.reset(
                            (channel, 0, m.controller()),
                            m.value() != 0,
                            ControlChange(m.with_value(0)?),
                        ),
                        _ => {}
                    }
                }
                PitchBend(m) => self.reset(
                    (m.channel().expect("decoded channel"), 1, 0),
                    m.value() != 0,
                    PitchBend(m.with_value(0)?),
                ),
                ChannelPressure(m) => self.reset(
                    (m.channel().expect("decoded channel"), 2, 0),
                    m.pressure() != 0,
                    ChannelPressure(m.with_pressure(0)?),
                ),
                PolyPressure(m) => self.reset(
                    (m.channel().expect("decoded channel"), 3, m.note()),
                    m.pressure() != 0,
                    PolyPressure(m.with_pressure(0)?),
                ),
                SystemReset(_) => {
                    self.held.clear();
                    self.resets.clear();
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn release(&mut self, channel: u8, note: u8) {
        if let Some(count) = self.held.get_mut(&(channel, note)) {
            *count -= 1;
            if *count == 0 {
                self.held.remove(&(channel, note));
            }
        }
    }
    fn reset(&mut self, key: (u8, u8, u8), active: bool, message: AnyMessage) {
        if active {
            self.resets.insert(key, message);
        } else {
            self.resets.remove(&key);
        }
    }
    pub fn cleanup(self) -> impl Iterator<Item = AnyMessage> {
        self.held
            .into_iter()
            .flat_map(|((channel, note), count)| {
                (0..count).map(move |_| {
                    AnyMessage::NoteOff(
                        messages::NoteOff::new(note.into())
                            .expect("decoded note")
                            .with_channel(channel.into())
                            .expect("decoded channel"),
                    )
                })
            })
            .chain(self.resets.into_values())
    }
}
pub(crate) fn select_port<'a>(ports: &'a [Port], pattern: Option<&str>) -> io::Result<&'a Port> {
    Ok(&ports[select_port_index(ports.iter().map(|p| p.name()), pattern)?])
}
/// Shared selection policy for native ports and Python transport descriptors.
#[doc(hidden)]
pub fn select_port_index<'a>(
    names: impl IntoIterator<Item = &'a str>,
    pattern: Option<&str>,
) -> io::Result<usize> {
    let mut names = names.into_iter().enumerate();
    let Some(pattern) = pattern else {
        return names
            .next()
            .map(|(i, _)| i)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no MIDI ports available"));
    };
    if pattern.is_empty() {
        return Err(invalid("port pattern must not be empty"));
    }
    let mut matches = names.filter(|(_, n)| n.contains(pattern) || wildcard(pattern, n));
    let (index, _) = matches
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no matching MIDI port"))?;
    if matches.next().is_some() {
        return Err(invalid("port pattern matches multiple ports"));
    }
    Ok(index)
}
fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<_> = pattern.chars().collect();
    let n: Vec<_> = name.chars().collect();
    let (mut i, mut j, mut star, mut retry) = (0, 0, None, 0);
    while j < n.len() {
        if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            retry = j;
        } else if let Some(next) = pattern_character(&p, i, n[j]) {
            i = next;
            j += 1;
        } else if let Some(s) = star {
            retry += 1;
            j = retry;
            i = s + 1;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}
fn pattern_character(pattern: &[char], start: usize, value: char) -> Option<usize> {
    let c = *pattern.get(start)?;
    if c == '?' {
        return Some(start + 1);
    }
    if c != '[' {
        return (c == value).then_some(start + 1);
    }
    let mut first = start + 1;
    let negate = pattern.get(first) == Some(&'!');
    first += usize::from(negate);
    let mut end = first + usize::from(pattern.get(first) == Some(&']'));
    while end < pattern.len() && pattern[end] != ']' {
        end += 1;
    }
    if end == pattern.len() {
        return (value == '[').then_some(start + 1);
    }
    let mut matched = false;
    let mut i = first;
    while i < end {
        if i + 2 < end && pattern[i + 1] == '-' {
            matched |= pattern[i] <= value && value <= pattern[i + 2];
            i += 3;
        } else {
            matched |= pattern[i] == value;
            i += 1;
        }
    }
    (matched != negate).then_some(end + 1)
}
fn nanoseconds(seconds: Rational) -> io::Result<u64> {
    seconds
        .multiply(1_000_000_000, 1)?
        .round(true)?
        .0
        .try_into()
        .map_err(|_| invalid("duration exceeds scheduler range"))
}
pub(crate) fn transport_error(error: transport::Error) -> io::Error {
    let kind = match error {
        transport::Error::Invalid(_) => io::ErrorKind::InvalidInput,
        transport::Error::Closed => io::ErrorKind::NotConnected,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
fn report(error: &impl std::fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "meedee: {error}");
}

impl From<Payload> for OutputValue {
    fn from(v: Payload) -> Self {
        Self::Message(v)
    }
}
impl From<Packet> for OutputValue {
    fn from(v: Packet) -> Self {
        Self::Packet(v)
    }
}
impl From<Vec<u8>> for OutputValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Packet(Packet::Bytes(v))
    }
}
impl From<Note> for OutputValue {
    fn from(v: Note) -> Self {
        Self::Note(v)
    }
}
impl From<Rest> for OutputValue {
    fn from(v: Rest) -> Self {
        Self::Rest(v)
    }
}
impl From<Duration> for OutputValue {
    fn from(v: Duration) -> Self {
        Self::Wait(v.into())
    }
}
impl From<Beats> for OutputValue {
    fn from(v: Beats) -> Self {
        Self::Wait(v.into())
    }
}
impl From<NoteDuration> for OutputValue {
    fn from(v: NoteDuration) -> Self {
        Self::Wait(v)
    }
}
impl From<AnyMessage> for OutputValue {
    fn from(v: AnyMessage) -> Self {
        Self::Message(v.into())
    }
}
impl From<AnyCommand> for OutputValue {
    fn from(v: AnyCommand) -> Self {
        Self::Message(Payload::Command(v))
    }
}
impl From<crate::ump::PacketValue> for OutputValue {
    fn from(v: crate::ump::PacketValue) -> Self {
        Self::Message(Payload::Ump(v))
    }
}
macro_rules! midi1_values {($($name:ident),*)=>{$(impl From<messages::$name> for OutputValue {fn from(v:messages::$name)->Self {AnyMessage::$name(v).into()}})*};}
midi1_values!(
    PerNoteController,
    PerNoteManagement,
    ParameterController,
    RelativeParameterController,
    PerNotePitchBend,
    NoteOn,
    NoteOff,
    PolyPressure,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
    QuarterFrame,
    SongPosition,
    SongSelect,
    TuneRequest,
    TimingClock,
    Start,
    Continue,
    Stop,
    ActiveSensing,
    SystemReset,
    SysEx
);
macro_rules! command_values {($($name:ident),*)=>{$(impl From<messages::$name> for OutputValue {fn from(v:messages::$name)->Self {AnyCommand::$name(v).into()}})*};}
command_values!(ParameterSet, ParameterStep);
impl From<crate::mpe::ZoneConfiguration> for OutputValue {
    fn from(v: crate::mpe::ZoneConfiguration) -> Self {
        AnyCommand::ZoneConfiguration(v).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Clock(Rc<RefCell<Vec<u64>>>);
    impl PlaybackClock for Clock {
        fn now_ns(&mut self) -> u64 {
            self.0.borrow().last().copied().unwrap_or(0)
        }
        fn wait_until(&mut self, deadline: u64, _: &Cancellation) -> io::Result<bool> {
            self.0.borrow_mut().push(deadline);
            Ok(true)
        }
    }
    struct Fixture {
        output: Output,
        sink: Rc<RefCell<crate::testing::Output>>,
        deadlines: Rc<RefCell<Vec<u64>>>,
    }
    impl Fixture {
        fn new() -> Self {
            let sink = Rc::new(RefCell::new(
                crate::testing::Output::new(Encoding::Bytes, 128, 4096).unwrap(),
            ));
            let deadlines = Rc::new(RefCell::new(Vec::new()));
            let mut output = Output::new();
            output.connection = Some(Connection::Test(sink.clone()));
            output.clock = Box::new(Clock(deadlines.clone()));
            Self {
                output,
                sink,
                deadlines,
            }
        }
        fn packets(&self) -> Vec<Packet> {
            let mut packets = Vec::new();
            while let Some(packet) = self.sink.borrow_mut().take_packet() {
                packets.push(packet);
            }
            packets
        }
    }
    fn bytes(values: &[&[u8]]) -> Vec<Packet> {
        values.iter().map(|v| Packet::Bytes(v.to_vec())).collect()
    }

    #[test]
    fn with_gap_schedules_fallible_sysex_and_a_trailing_wait() {
        let mut fixture = Fixture::new();
        let source =
            crate::file_io::Reader::new(&b"\xf0\x7d\xf7\xf0\x7d\xf7"[..], 1024, false, false)
                .unwrap();
        let report = fixture
            .output
            .send(crate::events::with_gap(source, Duration::from_millis(100)))
            .unwrap();
        assert_eq!(report.sent_events, 2);
        assert_eq!(*fixture.deadlines.borrow(), [0, 100_000_000, 200_000_000]);
        assert_eq!(
            fixture.packets(),
            bytes(&[&[0xf0, 0x7d, 0xf7], &[0xf0, 0x7d, 0xf7]])
        );
    }

    #[test]
    fn with_gap_preserves_source_errors_before_sending() {
        let mut fixture = Fixture::new();
        let source =
            crate::file_io::Reader::new(&b"\xf0\x7d\xf7\xf0\x7d"[..], 1024, false, false).unwrap();
        let failure = fixture
            .output
            .send(crate::events::with_gap(source, Duration::ZERO))
            .unwrap_err();
        assert!(matches!(failure.cause, crate::playback::Cause::Input(_)));
        assert_eq!(failure.report.sent_events, 0);
        assert!(fixture.packets().is_empty());
    }

    fn recorded(ticks: i64, note: u8) -> crate::archive::Record {
        let clock =
            crate::events::Clock::new("test".into(), 1, 1_000_000_000, "capture".into(), None)
                .unwrap();
        let message = messages::NoteOn::new(note.into())
            .unwrap()
            .with_channel(1)
            .unwrap()
            .with_timestamp(crate::events::Timestamp { ticks, clock })
            .unwrap()
            .with_stream("input")
            .unwrap();
        crate::archive::Record::Event(Box::new(Payload::Message(AnyMessage::NoteOn(message))))
    }

    #[test]
    fn replay_uses_first_message_as_origin_after_previous_output_activity() {
        let mut f = Fixture::new();
        f.output.send(Duration::from_secs(2)).unwrap();
        let report = f
            .output
            .send([
                Ok(recorded(9_000_000_000, 60)),
                Ok(recorded(9_500_000_000, 62)),
            ])
            .unwrap();
        assert_eq!(report.sent_events, 2);
        assert_eq!(report.max_lateness_ns, 0);
        assert_eq!(
            *f.deadlines.borrow(),
            [2_000_000_000, 2_000_000_000, 2_500_000_000]
        );
        assert_eq!(f.packets(), bytes(&[&[0x90, 60, 90], &[0x90, 62, 90]]));
        f.output.close().unwrap();
        assert_eq!(f.packets(), bytes(&[&[0x80, 60, 0], &[0x80, 62, 0]]));
    }

    #[test]
    fn replay_options_persist_and_control_origin_metrics_and_cancellation() {
        let mut f = Fixture::new();
        let origin = recorded(100, 60);
        let crate::archive::Record::Event(message) = origin else {
            unreachable!()
        };
        let cancel = Cancellation::default();
        f.output = f.output.with_playback(PlaybackOptions {
            origin: message.timestamp().cloned(),
            metrics: true,
            cancel: cancel.clone(),
            ..Default::default()
        });
        for _ in 0..2 {
            let report = f.output.send([Ok(recorded(200, 60))]).unwrap();
            assert_eq!(report.lateness.unwrap().count(), 1);
        }
        assert_eq!(*f.deadlines.borrow(), [100, 200]);
        cancel.cancel();
        let report = f.output.send([Ok(recorded(300, 60))]).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.sent_events, 0);
        assert_eq!(f.packets().len(), 2);
    }

    #[test]
    fn replay_stops_at_loss_or_regression_and_reports_confirmed_prefix() {
        let mut f = Fixture::new();
        let failure = f
            .output
            .send([
                Ok(recorded(100, 60)),
                Ok(crate::archive::Record::Gap {
                    stream: "input".into(),
                    lost: 1,
                    reason: "test".into(),
                }),
                Ok(recorded(200, 62)),
            ])
            .unwrap_err();
        assert_eq!(failure.report.sent_events, 1);
        assert!(matches!(failure.cause, crate::playback::Cause::Input(_)));
        assert_eq!(f.packets(), bytes(&[&[0x90, 60, 90]]));
        let failure = f
            .output
            .send([Ok(recorded(200, 64)), Ok(recorded(100, 65))])
            .unwrap_err();
        assert_eq!(failure.report.sent_events, 1);
        assert_eq!(f.packets(), bytes(&[&[0x90, 64, 90]]));
    }

    #[test]
    fn replay_accepts_a_strict_archive_reader_and_detects_missing_trailer() {
        use crate::archive::{Reader, Record, Writer};
        let first = recorded(100, 60);
        let Record::Event(message) = &first else {
            unreachable!()
        };
        let clock = message.timestamp().unwrap().clock.clone();
        let mut data = Vec::new();
        {
            let mut writer = Writer::new(&mut data, 1024).unwrap();
            writer.write(&Record::Clock(clock)).unwrap();
            writer
                .write(&Record::Stream {
                    id: "input".into(),
                    label: "keyboard".into(),
                    clock_identity: Some("test".into()),
                })
                .unwrap();
            writer.write(&first).unwrap();
            writer.finish(true, "").unwrap();
        }
        let mut f = Fixture::new();
        assert_eq!(
            f.output
                .send(Reader::new(data.as_slice(), 1024, false, false).unwrap())
                .unwrap()
                .sent_events,
            1
        );
        data.pop();
        let failure = f
            .output
            .send(Reader::new(data.as_slice(), 1024, false, false).unwrap())
            .unwrap_err();
        assert_eq!(failure.report.sent_events, 1);
        assert!(matches!(failure.cause, crate::playback::Cause::Input(_)));
    }

    #[test]
    fn empty_replay_and_invalid_first_record_do_not_open_ports() {
        let mut output = Output::new();
        assert_eq!(output.send([] as [OutputValue; 0]).unwrap().sent_events, 0);
        assert!(output
            .send([Err::<crate::archive::Record, _>(invalid("bad capture"))])
            .is_err());
        assert!(output.client.is_none());
        assert!(output.connection.is_none());
    }

    #[test]
    fn channels_default_preserve_and_override_but_raw_bytes_keep_addresses() {
        let mut f = Fixture::new();
        f.output
            .send(messages::ProgramChange::new(34).unwrap())
            .unwrap();
        f.output
            .send(
                messages::ProgramChange::new(81)
                    .unwrap()
                    .with_channel(3)
                    .unwrap(),
            )
            .unwrap();
        f.output = f.output.with_channel(2).unwrap();
        f.output
            .send(
                messages::ProgramChange::new(81)
                    .unwrap()
                    .with_channel(3)
                    .unwrap(),
            )
            .unwrap();
        f.output.send(vec![0xc2, 80]).unwrap();
        assert_eq!(
            f.packets(),
            bytes(&[&[0xc0, 33], &[0xc2, 80], &[0xc1, 80], &[0xc2, 80]])
        );
    }

    #[test]
    fn beat_deadlines_accumulate_exactly_and_elapsed_time_stays_in_seconds() {
        let mut f = Fixture::new();
        f.output = f.output.with_bpm(120).unwrap();
        f.output
            .send([
                OutputValue::from(Note::new(Beats::new(1, 3).unwrap(), 60).unwrap()),
                Note::new(Beats::new(1, 3).unwrap(), 62)
                    .unwrap()
                    .with_velocity(70)
                    .unwrap()
                    .into(),
                Rest::new(Beats::new(1, 3).unwrap()).into(),
                Duration::from_millis(500).into(),
                messages::ProgramChange::new(2).unwrap().into(),
            ])
            .unwrap();
        assert_eq!(
            *f.deadlines.borrow(),
            [
                0,
                166_666_667,
                166_666_667,
                333_333_333,
                1_000_000_000,
                1_000_000_000
            ]
        );
        assert_eq!(
            f.packets(),
            bytes(&[
                &[0x90, 60, 90],
                &[0x80, 60, 0],
                &[0x90, 62, 70],
                &[0x80, 62, 0],
                &[0xc0, 1]
            ])
        );
        f.output.close().unwrap();
        assert!(f.packets().is_empty());
    }

    #[test]
    fn drop_releases_overlapping_counts_and_only_changed_resettable_state() {
        let mut f = Fixture::new();
        f.output
            .send([
                OutputValue::from(messages::NoteOn::new(60).unwrap()),
                messages::NoteOn::new(60).unwrap().into(),
                messages::NoteOn::new(62).unwrap().into(),
                messages::NoteOff::new(62).unwrap().into(),
                messages::ControlChange::new(64, 127).unwrap().into(),
                messages::ControlChange::new(7, 99).unwrap().into(),
                messages::ControlChange::new(1, 55).unwrap().into(),
                messages::ControlChange::new(1, 0).unwrap().into(),
                messages::PitchBend::new()
                    .unwrap()
                    .with_value(100)
                    .unwrap()
                    .into(),
                messages::ChannelPressure::new()
                    .unwrap()
                    .with_pressure(50)
                    .unwrap()
                    .into(),
                messages::PolyPressure::new(60)
                    .unwrap()
                    .with_pressure(40)
                    .unwrap()
                    .into(),
            ])
            .unwrap();
        f.packets();
        drop(std::mem::take(&mut f.output));
        assert_eq!(
            f.packets(),
            bytes(&[
                &[0x80, 60, 0],
                &[0x80, 60, 0],
                &[0xb0, 64, 0],
                &[0xe0, 0, 64],
                &[0xd0, 0],
                &[0xa0, 60, 0]
            ])
        );
        assert!(f.sink.borrow().snapshot().closed);
    }

    #[test]
    fn invalid_later_duration_does_not_send_the_valid_prefix() {
        let mut f = Fixture::new();
        assert!(f
            .output
            .send([
                OutputValue::from(messages::NoteOn::new(60).unwrap()),
                Beats::from(1).into(),
            ])
            .is_err());
        assert_eq!(f.sink.borrow().snapshot().attempts, 0);
        assert!(f.deadlines.borrow().is_empty());
        assert!(f
            .output
            .send([
                OutputValue::from(messages::NoteOn::new(60).unwrap()),
                Duration::MAX.into(),
            ])
            .is_err());
        assert_eq!(f.sink.borrow().snapshot().attempts, 0);
    }

    #[test]
    fn cleanup_failure_still_closes_and_cannot_send_again() {
        let mut f = Fixture::new();
        f.output.send(messages::NoteOn::new(60).unwrap()).unwrap();
        f.sink
            .borrow_mut()
            .fail_next_send(false, "disconnected")
            .unwrap();
        assert!(f
            .output
            .close()
            .unwrap_err()
            .to_string()
            .contains("disconnected"));
        assert!(f.output.closed());
        assert!(f.sink.borrow().snapshot().closed);
        assert_eq!(f.packets(), bytes(&[&[0x90, 60, 90]]));
        f.output.close().unwrap();
        assert!(f.output.send(messages::NoteOn::new(62).unwrap()).is_err());
    }

    #[test]
    fn compound_prefix_is_tracked_when_later_packet_cannot_be_accepted() {
        let mut f = Fixture::new();
        f.sink = Rc::new(RefCell::new(
            crate::testing::Output::new(Encoding::Bytes, 1, 3).unwrap(),
        ));
        f.output.connection = Some(Connection::Test(f.sink.clone()));
        let error = f
            .output
            .send(messages::ControlChange::new_with_resolution(1, 128, 14).unwrap())
            .unwrap_err();
        assert!(matches!(
            error.cause,
            crate::playback::Cause::Send(crate::midi::Error::Send(
                transport::Error::CompoundSend {
                    completed_messages: Some(1),
                    ..
                }
            ))
        ));
        assert_eq!(f.packets(), bytes(&[&[0xb0, 1, 1]]));
        f.output.close().unwrap();
        assert_eq!(f.packets(), bytes(&[&[0xb0, 1, 0]]));
    }

    #[test]
    fn protocol_checks_precede_sends_and_midi2_resolves_output_channel() {
        let mut f = Fixture::new();
        let note = crate::messages::NoteOn::new_midi2(60)
            .and_then(|m| m.with_group(2))
            .and_then(|m| m.with_channel(3))
            .and_then(|m| m.with_velocity(1000))
            .and_then(|m| m.with_attribute_type(0))
            .and_then(|m| m.with_attribute_data(0))
            .unwrap();
        assert!(f.output.send(note.clone()).is_err());
        assert_eq!(f.sink.borrow().snapshot().attempts, 0);
        f.output.options.encoding = Encoding::Ump;
        f.sink = Rc::new(RefCell::new(
            crate::testing::Output::new(Encoding::Ump, 8, 128).unwrap(),
        ));
        f.output.connection = Some(Connection::Test(f.sink.clone()));
        f.output = f.output.with_channel(9).unwrap();
        f.output.send(note).unwrap();
        f.output.send(crate::ump::Noop::new().unwrap()).unwrap();
        assert!(f
            .output
            .send(crate::ump::DeltaClockstamp::new(1).unwrap())
            .is_err());
        f.output.close().unwrap();
        assert_eq!(
            f.packets(),
            vec![
                Packet::Ump(vec![0x4198_3c00, 1000 << 16]),
                Packet::Ump(vec![0])
            ]
        );
    }

    #[test]
    fn ump_semantics_resolve_per_note_channels_and_batch_logical_values() {
        let mut f = Fixture::new();
        f.output.options.encoding = Encoding::Ump;
        f.sink = Rc::new(RefCell::new(
            crate::testing::Output::new(Encoding::Ump, 8, 128).unwrap(),
        ));
        f.output.connection = Some(Connection::Test(f.sink.clone()));
        f.output = f.output.with_channel(3).unwrap();
        let per_note = messages::PerNoteController::new(5, 7, 60, 3, 1234).unwrap();
        let command = messages::ControlChange::new_with_resolution(7, 10000, 14)
            .unwrap()
            .with_group(3)
            .unwrap();
        let sysex = messages::SysEx::new((0..7).collect())
            .unwrap()
            .with_group(3)
            .unwrap();
        let report = f
            .output
            .send(vec![
                OutputValue::from(per_note),
                command.into(),
                sysex.into(),
            ])
            .unwrap();
        assert_eq!(report.sent_events, 3);
        assert_eq!(
            f.packets(),
            vec![
                Packet::Ump(vec![0x44023c03, 1234]),
                Packet::Ump(vec![0x22b2074e, 0x22b22710]),
                Packet::Ump(vec![0x32160001, 0x02030405, 0x32310600, 0]),
            ]
        );
        let error = f
            .output
            .send(vec![
                OutputValue::from(messages::NoteOn::new(60).unwrap().with_group(1).unwrap()),
                messages::TimingClock::new().unwrap().into(),
            ])
            .unwrap_err();
        assert_eq!(error.report.sent_events, 0);
        assert!(f.packets().is_empty());
        f.output.close().unwrap();
        assert!(f.packets().is_empty());
    }

    #[test]
    fn wait_only_sequence_does_not_discover_or_connect() {
        let mut output = Output::new();
        let deadlines = Rc::new(RefCell::new(Vec::new()));
        output.clock = Box::new(Clock(deadlines.clone()));
        output.send(Duration::from_millis(500)).unwrap();
        assert_eq!(*deadlines.borrow(), [500_000_000]);
        assert!(output.client.is_none());
        assert!(output.connection.is_none());
    }
    #[test]
    fn unified_send_concatenates_sources_and_reports_every_handoff() {
        let mut f = Fixture::new();
        f.output = f.output.with_playback(PlaybackOptions {
            metrics: true,
            ..Default::default()
        });
        let report = f
            .output
            .send((
                Note::new(Duration::from_millis(1), 60).unwrap(),
                [Ok(recorded(10_000_000, 62)), Ok(recorded(11_000_000, 64))]
                    .into_iter()
                    .take(2),
                messages::NoteOff::new(62).unwrap(),
            ))
            .unwrap();
        assert_eq!(report.sent_events, 5);
        assert_eq!(report.lateness.unwrap().count(), 5);
        assert_eq!(
            f.packets(),
            bytes(&[
                &[0x90, 60, 90],
                &[0x80, 60, 0],
                &[0x90, 62, 90],
                &[0x90, 64, 90],
                &[0x80, 62, 0]
            ])
        );
        assert!(!f.output.closed());
    }
    #[test]
    fn unified_send_rejects_mixed_timing_and_preserves_confirmed_prefix() {
        for timed_first in [false, true] {
            let mut f = Fixture::new();
            let crate::archive::Record::Event(timed) = recorded(100, 60) else {
                unreachable!()
            };
            let plain = Payload::Message(AnyMessage::NoteOn(messages::NoteOn::new(62).unwrap()));
            let source = if timed_first {
                vec![*timed, plain]
            } else {
                vec![plain, *timed]
            };
            let failure = f.output.send(source).unwrap_err();
            assert_eq!(failure.report.sent_events, u64::from(timed_first));
            assert!(failure.to_string().contains("mix"));
            assert_eq!(f.packets().len(), usize::from(timed_first));
        }
    }
    #[test]
    fn direct_messages_ignore_timestamp_metadata_and_cancelled_sources_are_not_consumed() {
        let mut f = Fixture::new();
        let crate::archive::Record::Event(message) = recorded(100, 60) else {
            unreachable!()
        };
        f.output.send(*message).unwrap();
        assert_eq!(*f.deadlines.borrow(), [0, 0]);
        let cancel = Cancellation::default();
        cancel.cancel();
        let mut output = Output::new().with_playback(PlaybackOptions {
            cancel,
            ..Default::default()
        });
        let source = std::iter::from_fn(|| -> Option<io::Result<crate::archive::Record>> {
            panic!("consumed cancelled source")
        });
        let report = output.send(source).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.sent_events, 0);
        assert!(output.client.is_none());
    }
    #[test]
    fn scalar_note_report_counts_rounded_deadlines() {
        let mut f = Fixture::new();
        f.output = f
            .output
            .with_bpm(Rational::new(120_000_000_000, 1).unwrap())
            .unwrap();
        let report = f
            .output
            .send(Note::new(Beats::from(1), 60).unwrap())
            .unwrap();
        assert_eq!(report.sent_events, 2);
        assert_eq!(report.rounded_events, 1);
    }
}

macro_rules! ump_values {
    ($($name:ident => $variant:ident),* $(,)?) => {
        $(impl From<crate::ump::$name> for OutputValue {
            fn from(value: crate::ump::$name) -> Self {
                crate::ump::PacketValue::$variant(value).into()
            }
        })*
    };
}
ump_values!(
    Noop => Noop,
    JrClock => JrClock,
    JrTimestamp => JrTimestamp,
    DeltaClockstampTicksPerQuarter => DeltaClockstampTicksPerQuarter,
    DeltaClockstamp => DeltaClockstamp,
    SysEx7Packet => SysEx7,
    SysEx8Packet => SysEx8,
    MixedDataSet => MixedDataSet,
    FlexData => FlexData,
    StreamMessage => Stream,
    OpaquePacket => Opaque,
);

// These validated system messages have no connection-relative channel fields.
macro_rules! system_values {
    ($module:ident: $($name:ident),* $(,)?) => {
        $(impl From<crate::$module::$name> for OutputValue {
            fn from(value: crate::$module::$name) -> Self {
                Self::Packet(Packet::Bytes(value.encode().expect("validated system message")))
            }
        })*
    };
}
system_values!(gm: GmMode);
system_values!(universal: IdentityRequest, IdentityReply, MasterVolume, MasterFineTuning, MasterCoarseTuning);
system_values!(timecode: FullFrame);
system_values!(ci: Management);
system_values!(tuning: TuningMessage, BulkDumpRequest, BulkDump, ScaleOctaveDump, NoteChange, ScaleOctaveChange);
