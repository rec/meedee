//! MIDI 1.0 live byte streams, not Standard MIDI Files or UMP.
//!
//! Wire baseline: MIDI 1.0 Detailed Specification 4.2.1, Channel Voice,
//! System Common/Real-Time and appendix A Running Status/Undefined Status.
//! Quarter-frame bit layout: MIDI Association, Summary of MIDI 1.0 Messages.

use crate::messages::*;

/// Encode exactly one primitive message without running-status compression.
pub fn encode(message: &(impl PrimitiveMessage + ?Sized)) -> Vec<u8> {
    message.encode()
}

/// Expand a semantic command into ordered primitive CC messages. No I/O occurs.
pub fn expand(command: &(impl Command + ?Sized)) -> Vec<ControlChange> {
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
        let mut output = Vec::new();
        for &byte in bytes {
            self.accept(byte, &mut output);
            self.offset = self.offset.saturating_add(1);
        }
        output
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
                0xf8 => Some(AnyMessage::TimingClock(TimingClock {})),
                0xfa => Some(AnyMessage::Start(Start {})),
                0xfb => Some(AnyMessage::Continue(Continue {})),
                0xfc => Some(AnyMessage::Stop(Stop {})),
                0xfe => Some(AnyMessage::ActiveSensing(ActiveSensing {})),
                0xff => Some(AnyMessage::SystemReset(SystemReset {})),
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
            0xf6 => output.push(DecodeItem::Message(AnyMessage::TuneRequest(TuneRequest {}))),
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
        0x80..=0x8f => AnyMessage::NoteOff(NoteOff::new(channel, a, b).unwrap()),
        0x90..=0x9f => AnyMessage::NoteOn(NoteOn::new(channel, a, b).unwrap()),
        0xa0..=0xaf => AnyMessage::PolyPressure(PolyPressure::new(channel, a, b).unwrap()),
        0xb0..=0xbf => AnyMessage::ControlChange(ControlChange::new(channel, a, b).unwrap()),
        0xc0..=0xcf => AnyMessage::ProgramChange(ProgramChange::new(channel, a + 1).unwrap()),
        0xd0..=0xdf => AnyMessage::ChannelPressure(ChannelPressure::new(channel, a).unwrap()),
        0xe0..=0xef => AnyMessage::PitchBend(PitchBend::new(channel, a + (b << 7) - 8192).unwrap()),
        0xf1 => AnyMessage::QuarterFrame(QuarterFrame::new(a >> 4, a & 15).unwrap()),
        0xf2 => AnyMessage::SongPosition(SongPosition::new(a + (b << 7)).unwrap()),
        0xf3 => AnyMessage::SongSelect(SongSelect::new(a).unwrap()),
        _ => unreachable!("only data-bearing statuses reach short_message"),
    }
}

macro_rules! encode_short {
    ($($name:ident ($value:ident) => $bytes:expr;)*) => {
        $(impl PrimitiveMessage for $name {
            fn encode(&$value) -> Vec<u8> { $bytes }
        })*
        impl PrimitiveMessage for AnyMessage {
            fn encode(&self) -> Vec<u8> {
                match self {
                    $(Self::$name(message) => message.encode(),)*
                    Self::SysEx(message) => message.encode(),
                }
            }
        }
    };
}

impl PrimitiveMessage for SysEx {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.data().len() + 2);
        bytes.push(0xf0);
        bytes.extend_from_slice(self.data());
        bytes.push(0xf7);
        bytes
    }
}

fn cc(channel: u8, controller: u8, value: u8) -> ControlChange {
    ControlChange::new(channel.into(), controller.into(), value.into())
        .expect("expanded fields have validated wire ranges")
}

fn parameter_command(
    channel: u8,
    parameter: u16,
    msb: u8,
    data: &[(u8, u8)],
) -> Vec<ControlChange> {
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
}

impl Command for ControlChange14 {
    fn expand(&self) -> Vec<ControlChange> {
        vec![
            cc(self.channel(), self.controller(), (self.value() >> 7) as u8),
            cc(
                self.channel(),
                self.controller() + 32,
                (self.value() & 127) as u8,
            ),
        ]
    }
}

macro_rules! parameter_commands {
    (sets: $($set:ident => $selector:literal),*; steps: $($step:ident => ($family:literal, $controller:literal)),* $(;)?) => {
        $(impl Command for $set {
            fn expand(&self) -> Vec<ControlChange> {
                let data = if self.resolution() == 7 {
                    vec![(6, self.value() as u8)]
                } else {
                    vec![(6, (self.value() >> 7) as u8), (38, (self.value() & 127) as u8)]
                };
                parameter_command(self.channel(), self.parameter(), $selector, &data)
            }
        })*
        $(impl Command for $step {
            fn expand(&self) -> Vec<ControlChange> {
                parameter_command(self.channel(), self.parameter(), $family, &[($controller, 0)])
            }
        })*
        impl Command for AnyCommand {
            fn expand(&self) -> Vec<ControlChange> {
                match self {
                    Self::ControlChange14(command) => command.expand(),
                    $(Self::$set(command) => command.expand(),)*
                    $(Self::$step(command) => command.expand(),)*
                }
            }
        }
    };
}

parameter_commands! {
    sets: RpnSet => 101, NrpnSet => 99;
    steps: RpnIncrement => (101, 96), RpnDecrement => (101, 97),
           NrpnIncrement => (99, 96), NrpnDecrement => (99, 97);
}

encode_short! {
    NoteOn(self) => vec![0x90 | (self.channel() - 1), self.note(), self.velocity()];
    NoteOff(self) => vec![0x80 | (self.channel() - 1), self.note(), self.velocity()];
    PolyPressure(self) => vec![0xa0 | (self.channel() - 1), self.note(), self.pressure()];
    ControlChange(self) => vec![0xb0 | (self.channel() - 1), self.controller(), self.value()];
    ProgramChange(self) => vec![0xc0 | (self.channel() - 1), self.program() - 1];
    ChannelPressure(self) => vec![0xd0 | (self.channel() - 1), self.pressure()];
    PitchBend(self) => {
        let wire = (i32::from(self.value()) + 8192) as u16;
        vec![0xe0 | (self.channel() - 1), (wire & 127) as u8, (wire >> 7) as u8]
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
