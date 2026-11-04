//! MIDI 1.0 live byte streams, not Standard MIDI Files or UMP.
//!
//! Wire baseline: MIDI 1.0 Detailed Specification 4.2.1, Channel Voice,
//! System Common/Real-Time and appendix A Running Status/Undefined Status.
//! Quarter-frame bit layout: MIDI Association, Summary of MIDI 1.0 Messages.

use crate::messages::*;

/// Encode exactly one primitive message without running-status compression.
pub fn encode(message: &(impl PrimitiveMessage + ?Sized)) -> Result<Vec<u8>, ValueError> {
    message.encode()
}

/// Expand a semantic command into ordered primitive MIDI 1.0 messages. No I/O occurs.
pub fn expand(command: &(impl Command + ?Sized)) -> Result<Vec<AnyMessage>, ValueError> {
    command.expand()
}

/// A malformed byte-stream condition. Offsets refer to bytes across all feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    UnexpectedData,
    UndefinedStatus,
    UnexpectedEnd,
    InterruptedMessage,
    InterruptedSysEx,
    SysExTooLarge,
    TruncatedMessage,
    TruncatedSysEx,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub offset: usize,
    /// The offending byte, or None for an end-of-stream condition.
    pub byte: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeItem {
    Message(AnyMessage),
    Diagnostic(Diagnostic),
}

/// One mutable decoder per independent byte stream. Never share running status
/// between inputs. Retains at most `sysex_limit` payload bytes between feeds.
#[derive(Debug)]
pub struct Decoder {
    sysex_limit: usize,
    offset: usize,
    message_start: usize,
    running: Option<u8>,
    pending: Option<u8>,
    data: [u8; 2],
    length: usize,
    sysex: SysExState,
}

impl Decoder {
    pub const DEFAULT_SYSEX_LIMIT: usize = 1_048_576;

    pub fn new(sysex_limit: usize) -> Result<Self, ValueError> {
        if sysex_limit == 0 {
            return Err(ValueError("sysex_limit must be positive".into()));
        }
        Ok(Self {
            sysex_limit,
            offset: 0,
            message_start: 0,
            running: None,
            pending: None,
            data: [0; 2],
            length: 0,
            sysex: SysExState::Idle,
        })
    }

    pub fn sysex_limit(&self) -> usize {
        self.sysex_limit
    }

    /// Output order is completion/detection order. Real-time messages can precede
    /// a longer message that began earlier. Chunk boundaries have no semantics.
    /// Returned storage is proportional to this feed's input, not the SysEx cap.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<DecodeItem> {
        self.feed_spanned(bytes)
            .into_iter()
            .map(|(v, _, _)| v)
            .collect()
    }
    /// Absolute byte offsets for contributing bytes, inclusive. Realtime messages
    /// have their own span and do not consume the pending message's start.
    pub fn feed_spanned(&mut self, bytes: &[u8]) -> Vec<(DecodeItem, usize, usize)> {
        let mut result = Vec::new();
        let mut items = Vec::new();
        for &byte in bytes {
            if byte < 128
                && self.pending.is_none()
                && !matches!(self.sysex, SysExState::Collecting(_))
                || (128..248).contains(&byte) && byte != 247
            {
                self.message_start = self.offset;
            }
            self.accept(byte, &mut items);
            for item in items.drain(..) {
                let start = if byte >= 248 || matches!(item, DecodeItem::Diagnostic(_)) {
                    self.offset
                } else {
                    self.message_start
                };
                result.push((item, start, self.offset));
            }
            self.offset = self.offset.saturating_add(1);
        }
        result
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn pending_start(&self) -> Option<usize> {
        if self.pending.is_some() || matches!(self.sysex, SysExState::Collecting(_)) {
            Some(self.message_start)
        } else {
            None
        }
    }

    /// Signal end of stream, diagnose unfinished messages and clear running
    /// status. Previously reported oversized SysEx is not reported a second time.
    /// A later feed starts a fresh stream at the existing cumulative offset.
    pub fn finish(&mut self) -> Vec<DecodeItem> {
        let mut output = Vec::new();
        if matches!(self.sysex, SysExState::Collecting(_)) {
            self.report(&mut output, DiagnosticKind::TruncatedSysEx, None);
        } else if self.pending.is_some() {
            self.report(&mut output, DiagnosticKind::TruncatedMessage, None);
        }
        self.reset();
        output
    }

    /// Discard parser state after known transport loss or a deliberate stream
    /// boundary. Does not reset byte offsets or emit a synthetic wire message.
    pub fn reset(&mut self) {
        self.running = None;
        self.pending = None;
        self.length = 0;
        self.sysex = SysExState::Idle;
    }

    fn report(&self, output: &mut Vec<DecodeItem>, kind: DiagnosticKind, byte: Option<u8>) {
        output.push(DecodeItem::Diagnostic(Diagnostic {
            kind,
            offset: self.offset,
            byte,
        }));
    }

    fn accept(&mut self, byte: u8, output: &mut Vec<DecodeItem>) {
        // All real-time statuses, including reserved ones, preserve unfinished
        // messages, SysEx and running status. SystemReset is a wire value here;
        // interpreting its effects on an instrument is outside the codec.
        if byte >= 0xf8 {
            let message = match byte {
                0xf8 => Some(AnyMessage::TimingClock(
                    TimingClock::new().expect("valid system message"),
                )),
                0xfa => Some(AnyMessage::Start(
                    Start::new().expect("valid system message"),
                )),
                0xfb => Some(AnyMessage::Continue(
                    Continue::new().expect("valid system message"),
                )),
                0xfc => Some(AnyMessage::Stop(Stop::new().expect("valid system message"))),
                0xfe => Some(AnyMessage::ActiveSensing(
                    ActiveSensing::new().expect("valid system message"),
                )),
                0xff => Some(AnyMessage::SystemReset(
                    SystemReset::new().expect("valid system message"),
                )),
                _ => None,
            };
            if let Some(message) = message {
                output.push(DecodeItem::Message(message));
            } else {
                self.report(output, DiagnosticKind::UndefinedStatus, Some(byte));
            }
            return;
        }
        if byte < 0x80 {
            match &mut self.sysex {
                SysExState::Collecting(data) if data.len() < self.sysex_limit => {
                    data.push(byte);
                    return;
                }
                SysExState::Collecting(_) => {
                    self.sysex = SysExState::Discarding;
                    self.report(output, DiagnosticKind::SysExTooLarge, Some(byte));
                    return;
                }
                SysExState::Discarding => return,
                SysExState::Idle => {}
            }
            if self.pending.is_none() {
                self.pending = self.running;
            }
            let Some(status) = self.pending else {
                self.report(output, DiagnosticKind::UnexpectedData, Some(byte));
                return;
            };
            self.data[self.length] = byte;
            self.length += 1;
            if self.length == data_length(status) {
                output.push(DecodeItem::Message(short_message(status, self.data)));
                self.pending = None;
                self.length = 0;
            }
            return;
        }

        match std::mem::replace(&mut self.sysex, SysExState::Idle) {
            SysExState::Collecting(data) if byte == 0xf7 => {
                // Only seven-bit bytes are admitted above.
                output.push(DecodeItem::Message(AnyMessage::SysEx(
                    SysEx::new(data).expect("decoder admits only seven-bit SysEx payload"),
                )));
                return;
            }
            SysExState::Discarding if byte == 0xf7 => return,
            SysExState::Collecting(_) => {
                self.report(output, DiagnosticKind::InterruptedSysEx, Some(byte));
            }
            _ => {}
        }
        if self.pending.take().is_some() {
            self.report(output, DiagnosticKind::InterruptedMessage, Some(byte));
        }
        self.length = 0;
        self.running = (byte < 0xf0).then_some(byte);
        match byte {
            0xf0 => self.sysex = SysExState::Collecting(Vec::new()),
            0xf4 | 0xf5 => self.report(output, DiagnosticKind::UndefinedStatus, Some(byte)),
            0xf6 => output.push(DecodeItem::Message(AnyMessage::TuneRequest(
                TuneRequest::new().expect("valid system message"),
            ))),
            0xf7 => self.report(output, DiagnosticKind::UnexpectedEnd, Some(byte)),
            _ => self.pending = Some(byte),
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new(Self::DEFAULT_SYSEX_LIMIT).expect("nonzero default limit")
    }
}

#[derive(Debug)]
enum SysExState {
    Idle,
    Collecting(Vec<u8>),
    Discarding,
}

fn data_length(status: u8) -> usize {
    match status {
        0xc0..=0xdf | 0xf1 | 0xf3 => 1,
        _ => 2,
    }
}

fn short_message(status: u8, data: [u8; 2]) -> AnyMessage {
    let channel = i64::from((status & 15) + 1);
    let [a, b] = data.map(i64::from);
    // Status and data ranges are established by accept(); no caller-provided
    // unchecked indexing or unchecked message constructor is exposed.
    match status {
        0x80..=0x8f => AnyMessage::NoteOff(
            NoteOff::new(a)
                .and_then(|m| m.with_velocity(b))
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0x90..=0x9f => AnyMessage::NoteOn(
            NoteOn::new(a)
                .and_then(|m| m.with_velocity(b))
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xa0..=0xaf => AnyMessage::PolyPressure(
            PolyPressure::new(a)
                .and_then(|m| m.with_pressure(b))
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xb0..=0xbf => AnyMessage::ControlChange(
            ControlChange::new(a, b)
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xc0..=0xcf => AnyMessage::ProgramChange(
            ProgramChange::new(a + 1)
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xd0..=0xdf => AnyMessage::ChannelPressure(
            ChannelPressure::new()
                .and_then(|m| m.with_pressure(a))
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xe0..=0xef => AnyMessage::PitchBend(
            PitchBend::new()
                .and_then(|m| m.with_value(a + (b << 7) - 8192))
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        ),
        0xf1 => AnyMessage::QuarterFrame(QuarterFrame::new(a >> 4, a & 15).unwrap()),
        0xf2 => AnyMessage::SongPosition(SongPosition::new(a + (b << 7)).unwrap()),
        0xf3 => AnyMessage::SongSelect(SongSelect::new(a).unwrap()),
        _ => unreachable!("only data-bearing statuses reach short_message"),
    }
}

macro_rules! encode_short {
    ($($name:ident ($value:ident) => $bytes:expr;)*) => {
        $(impl PrimitiveMessage for $name {
            fn encode(&$value) -> Result<Vec<u8>, ValueError> { Ok($bytes) }
        })*
        impl PrimitiveMessage for AnyMessage {
            fn encode(&self) -> Result<Vec<u8>, ValueError> {
                match self {
                    $(Self::$name(message) => message.encode(),)*
                    Self::SysEx(message) => message.encode(),
                }
            }
        }
    };
}

impl PrimitiveMessage for SysEx {
    fn encode(&self) -> Result<Vec<u8>, ValueError> {
        let mut bytes = Vec::with_capacity(self.data().len() + 2);
        bytes.push(0xf0);
        bytes.extend_from_slice(self.data());
        bytes.push(0xf7);
        Ok(bytes)
    }
}

fn cc(channel: u8, controller: u8, value: u8) -> ControlChange {
    ControlChange::new(controller.into(), value.into())
        .and_then(|m| m.with_channel(channel.into()))
        .expect("expanded fields have validated wire ranges")
}

fn parameter_command(channel: u8, parameter: u16, msb: u8, data: &[(u8, u8)]) -> Vec<AnyMessage> {
    let mut messages = vec![
        cc(channel, msb, (parameter >> 7) as u8),
        cc(channel, msb - 1, (parameter & 127) as u8),
    ];
    messages.extend(
        data.iter()
            .map(|&(controller, value)| cc(channel, controller, value)),
    );
    messages.extend([cc(channel, 101, 127), cc(channel, 100, 127)]);
    messages
        .into_iter()
        .map(AnyMessage::ControlChange)
        .collect()
}

impl Command for ControlChange14 {
    fn expand(&self) -> Result<Vec<AnyMessage>, ValueError> {
        inherit_metadata(
            self,
            vec![
                cc(
                    self.resolved_channel()?,
                    self.controller(),
                    (self.value() >> 7) as u8,
                ),
                cc(
                    self.resolved_channel()?,
                    self.controller() + 32,
                    (self.value() & 127) as u8,
                ),
            ]
            .into_iter()
            .map(AnyMessage::ControlChange)
            .collect(),
        )
    }
}

macro_rules! parameter_commands {
    ($($set:ident => $selector:literal),* $(,)?) => {
        $(impl Command for $set {
            fn expand(&self) -> Result<Vec<AnyMessage>, ValueError> {
                let data = if self.resolution() == 7 {
                    vec![(6, self.value() as u8)]
                } else {
                    vec![(6, (self.value() >> 7) as u8), (38, (self.value() & 127) as u8)]
                };
                inherit_metadata(self, parameter_command(self.resolved_channel()?, self.parameter(), $selector, &data))
            }
        })*
        impl Command for AnyCommand {
            fn expand(&self) -> Result<Vec<AnyMessage>, ValueError> {
                match self {
                    Self::ControlChange14(command) => command.expand(),
                    Self::BankProgramSelect(command) => command.expand(),
                    Self::ZoneConfiguration(command) => command.expand(),
                    $(Self::$set(command) => command.expand(),)*
                    Self::ParameterStep(command) => command.expand(),
                }
            }
        }
    };
}

parameter_commands! {
    RpnSet => 101, NrpnSet => 99,
}

impl Command for ParameterStep {
    fn expand(&self) -> Result<Vec<AnyMessage>, ValueError> {
        use crate::controls::{ParameterFamily, StepDirection};
        let selector = match self.family() {
            ParameterFamily::Rpn => 101,
            ParameterFamily::Nrpn => 99,
        };
        let controller = match self.direction() {
            StepDirection::Increment => 96,
            StepDirection::Decrement => 97,
        };
        inherit_metadata(
            self,
            parameter_command(
                self.resolved_channel()?,
                self.parameter(),
                selector,
                &[(controller, 0)],
            ),
        )
    }
}

encode_short! {
    NoteOn(self) => vec![0x90 | (self.midi1_channel()? - 1), self.note(), self.velocity() as u8];
    NoteOff(self) => vec![0x80 | (self.midi1_channel()? - 1), self.note(), self.velocity() as u8];
    PolyPressure(self) => vec![0xa0 | (self.midi1_channel()? - 1), self.note(), self.pressure() as u8];
    ControlChange(self) => vec![0xb0 | (self.midi1_channel()? - 1), self.controller(), self.value() as u8];
    ProgramChange(self) => vec![0xc0 | (self.midi1_channel()? - 1), self.program() - 1];
    ChannelPressure(self) => vec![0xd0 | (self.midi1_channel()? - 1), self.pressure() as u8];
    PitchBend(self) => {
        let channel = self.midi1_channel()?;
        let wire = (self.value() + 8192) as u16;
        vec![0xe0 | (channel - 1), (wire & 127) as u8, (wire >> 7) as u8]
    };
    QuarterFrame(self) => vec![0xf1, (self.message_type() << 4) | self.value()];
    SongPosition(self) => vec![0xf2, (self.position() & 127) as u8, (self.position() >> 7) as u8];
    SongSelect(self) => vec![0xf3, self.song()];
    TuneRequest(self) => vec![0xf6];
    TimingClock(self) => vec![0xf8];
    Start(self) => vec![0xfa];
    Continue(self) => vec![0xfb];
    Stop(self) => vec![0xfc];
    ActiveSensing(self) => vec![0xfe];
    SystemReset(self) => vec![0xff];
}
