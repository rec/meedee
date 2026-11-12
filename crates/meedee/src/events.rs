//! Immutable events and exact clock arithmetic, independent of native transport.
pub use crate::event_views::{Observation, Value as ObservationValue, View};
use crate::file_io::invalid;
pub use crate::live::{
    send_message, Decoder as PacketDecoder, Diagnostic as StreamDiagnostic,
    DiagnosticKind as StreamDiagnosticKind, Item as InputItem,
};
use crate::messages::{AnyCommand, AnyMessage};
pub use crate::playback::{
    play, Cancellation, Cause as PlaybackCause, Failure as PlaybackError, Mapping as ClockMapping,
    MonotonicClock, Planned as PlannedEvent, Planner as PlaybackPlan, PlaybackClock,
    Report as PlaybackReport,
};
pub use crate::transport::ReceivedPacket;
use crate::ump::PacketValue;
use std::io;
use std::time::Duration;

/// A sequence item or an explicit wait, as produced by [`with_gap`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GapItem<T> {
    Item(T),
    Gap(Duration),
}

/// Yield each item followed by a gap, including after the final item.
pub fn with_gap<T>(
    items: impl IntoIterator<Item = T>,
    gap: Duration,
) -> impl Iterator<Item = GapItem<T>> {
    items
        .into_iter()
        .flat_map(move |i| [GapItem::Item(i), GapItem::Gap(gap)])
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clock {
    identity: String,
    numerator: u64,
    denominator: u64,
    origin: String,
    resolution_ticks: Option<u64>,
    backend: Option<i32>,
    mode: Option<bool>,
}
impl Clock {
    pub fn new(
        identity: String,
        numerator: u64,
        denominator: u64,
        origin: String,
        resolution_ticks: Option<u64>,
    ) -> io::Result<Self> {
        if identity.is_empty()
            || origin.is_empty()
            || numerator == 0
            || denominator == 0
            || resolution_ticks == Some(0)
        {
            return Err(invalid(0, "invalid clock descriptor"));
        }
        Ok(Self {
            identity,
            numerator,
            denominator,
            origin,
            resolution_ticks,
            backend: None,
            mode: None,
        })
    }
    pub fn with_transport(mut self, backend: i32, enabled: bool) -> Self {
        self.backend = Some(backend);
        self.mode = Some(enabled);
        self
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn numerator(&self) -> u64 {
        self.numerator
    }
    pub fn denominator(&self) -> u64 {
        self.denominator
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn resolution_ticks(&self) -> Option<u64> {
        self.resolution_ticks
    }
    pub fn backend(&self) -> Option<i32> {
        self.backend
    }
    pub fn mode(&self) -> Option<bool> {
        self.mode
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    pub ticks: i64,
    pub clock: Clock,
}
impl Timestamp {
    /// Exact elapsed seconds in this clock's tick units.
    pub fn seconds(&self) -> io::Result<Rational> {
        if self.clock.mode() == Some(false) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "disabled clock has no timestamps",
            ));
        }
        Rational::new(i128::from(self.ticks), 1)?.multiply(
            self.clock.numerator().into(),
            self.clock.denominator().into(),
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub first_packet: u64,
    pub last_packet: u64,
    pub first_timestamp: Option<Timestamp>,
    pub last_timestamp: Option<Timestamp>,
}
impl SourceSpan {
    pub fn validate(&self, timestamp: Option<&Timestamp>) -> io::Result<()> {
        if self.first_packet > self.last_packet {
            return Err(invalid(0, "reversed packet span"));
        }
        let clocks = [
            self.first_timestamp.as_ref(),
            self.last_timestamp.as_ref(),
            timestamp,
        ];
        let mut first = None;
        for t in clocks.into_iter().flatten() {
            if t.clock.mode() == Some(false) {
                return Err(invalid(0, "disabled clock cannot have span timestamps"));
            }
            if first.is_some_and(|c| c != &t.clock) {
                return Err(invalid(0, "a message cannot span clock generations"));
            }
            first = Some(&t.clock);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Payload {
    Message(AnyMessage),
    Ump(PacketValue),
    Command(AnyCommand),
}
crate::messages::enum_metadata!(Payload {
    Message,
    Ump,
    Command
});
impl From<AnyMessage> for Payload {
    fn from(value: AnyMessage) -> Self {
        match value {
            AnyMessage::ProgramChange(v) if v.version() == 1 && v.bank_msb().is_some() => {
                Self::Command(AnyCommand::ProgramChange(v))
            }
            AnyMessage::ControlChange(v) if v.version() == 1 && v.resolution() == 14 => {
                Self::Command(AnyCommand::ControlChange(v))
            }
            v => Self::Message(v),
        }
    }
}
impl From<PacketValue> for Payload {
    fn from(value: PacketValue) -> Self {
        Self::Ump(value)
    }
}
impl From<AnyCommand> for Payload {
    fn from(value: AnyCommand) -> Self {
        Self::Command(value)
    }
}
macro_rules! semantic_payloads {
    ($($name:ident),*) => { $(impl From<crate::messages::$name> for Payload {
        fn from(value: crate::messages::$name) -> Self { AnyMessage::$name(value).into() }
    })* };
}
semantic_payloads!(
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
    SysEx,
    RegisteredPerNoteController,
    AssignablePerNoteController,
    PerNoteManagement,
    RegisteredController,
    AssignableController,
    RelativeRegisteredController,
    RelativeAssignableController,
    PerNotePitchBend
);
macro_rules! command_payloads {
    ($module:ident: $($name:ident),*) => { $(impl From<crate::$module::$name> for Payload {
        fn from(value: crate::$module::$name) -> Self { Self::Command(AnyCommand::$name(value)) }
    })* };
}
command_payloads!(messages: ParameterSet,ParameterStep);

command_payloads!(mpe: ZoneConfiguration);
macro_rules! packet_payloads {
    ($($name:ident => $variant:ident),*) => { $(impl From<crate::ump::$name> for Payload {
        fn from(value: crate::ump::$name) -> Self { Self::Ump(PacketValue::$variant(value)) }
    })* };
}
packet_payloads!(Noop=>Noop,JrClock=>JrClock,JrTimestamp=>JrTimestamp,
    DeltaClockstampTicksPerQuarter=>DeltaClockstampTicksPerQuarter,DeltaClockstamp=>DeltaClockstamp,
    SysEx7Packet=>SysEx7,SysEx8Packet=>SysEx8,MixedDataSet=>MixedDataSet,
    FlexData=>FlexData,StreamMessage=>Stream,OpaquePacket=>Opaque);
impl Payload {
    /// Resolve connection-relative channels while preserving all other fields.
    pub fn resolve_channel(self, channel: Option<u8>) -> Result<Self, crate::messages::ValueError> {
        if channel.is_some_and(|c| !(1..=16).contains(&c)) {
            return Err(crate::messages::ValueError(
                "channel must be in 1..=16".into(),
            ));
        }
        match self {
            Self::Message(v) => {
                let channel = channel.or(v.channel()).unwrap_or(1);
                Ok(Self::Message(v.with_channel(channel.into())?))
            }
            Self::Command(v) => {
                let channel = channel.or(v.channel()).unwrap_or(1);
                Ok(Self::Command(v.with_channel(channel.into())?))
            }
            v => Ok(v),
        }
    }
    pub fn validate(&self) -> Result<(), crate::messages::ValueError> {
        match self {
            Self::Message(AnyMessage::ProgramChange(v))
                if v.version() == 1 && v.bank_msb().is_some() =>
            {
                Err(crate::messages::ValueError(
                    "banked MIDI 1 ProgramChange must be classified as a command".into(),
                ))
            }
            Self::Command(AnyCommand::ProgramChange(v))
                if v.version() != 1 || v.bank_msb().is_none() =>
            {
                Err(crate::messages::ValueError(
                    "program command requires MIDI 1 with a bank".into(),
                ))
            }
            Self::Message(AnyMessage::ControlChange(v))
                if v.version() == 1 && v.resolution() == 14 =>
            {
                Err(crate::messages::ValueError(
                    "14-bit ControlChange must be classified as a command".into(),
                ))
            }
            Self::Command(AnyCommand::ControlChange(v))
                if v.version() != 1 || v.resolution() != 14 =>
            {
                Err(crate::messages::ValueError(
                    "controller command requires MIDI 1 with 14-bit resolution".into(),
                ))
            }
            _ => Ok(()),
        }
    }
    pub fn group(&self) -> Option<u8> {
        match self {
            Self::Message(v) => v.group(),
            Self::Ump(v) => v.group(),
            Self::Command(v) => v.group(),
        }
    }
}
/// Checked rational seconds, reduced on construction. Floating point is never used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    numerator: i128,
    denominator: i128,
}
impl From<i32> for Rational {
    fn from(value: i32) -> Self {
        Self {
            numerator: value.into(),
            denominator: 1,
        }
    }
}
impl Rational {
    pub fn new(numerator: i128, denominator: i128) -> io::Result<Self> {
        if denominator <= 0 {
            return Err(invalid(0, "denominator must be positive"));
        }
        let g = gcd(numerator.unsigned_abs(), denominator as u128);
        Ok(Self {
            numerator: numerator / (g as i128),
            denominator: denominator / (g as i128),
        })
    }
    pub fn numerator(&self) -> i128 {
        self.numerator
    }
    pub fn denominator(&self) -> i128 {
        self.denominator
    }
    pub fn checked_add(self, rhs: Self) -> io::Result<Self> {
        let g = gcd(self.denominator as u128, rhs.denominator as u128) as i128;
        let a = self.denominator / g;
        let b = rhs.denominator / g;
        let n = self
            .numerator
            .checked_mul(b)
            .and_then(|v| rhs.numerator.checked_mul(a).and_then(|r| v.checked_add(r)))
            .ok_or_else(|| invalid(0, "rational overflow"))?;
        Self::new(
            n,
            a.checked_mul(rhs.denominator)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn checked_sub(self, rhs: Self) -> io::Result<Self> {
        let g = gcd(self.denominator as u128, rhs.denominator as u128) as i128;
        let a = self.denominator / g;
        let b = rhs.denominator / g;
        let n = self
            .numerator
            .checked_mul(b)
            .and_then(|v| rhs.numerator.checked_mul(a).and_then(|r| v.checked_sub(r)))
            .ok_or_else(|| invalid(0, "rational overflow"))?;
        Self::new(
            n,
            a.checked_mul(rhs.denominator)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn multiply(self, n: i128, d: i128) -> io::Result<Self> {
        let rhs = Self::new(n, d)?;
        let a = gcd(self.numerator.unsigned_abs(), rhs.denominator as u128) as i128;
        let b = gcd(rhs.numerator.unsigned_abs(), self.denominator as u128) as i128;
        Self::new(
            (self.numerator / a)
                .checked_mul(rhs.numerator / b)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
            (self.denominator / b)
                .checked_mul(rhs.denominator / a)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn round(self, allow_rounding: bool) -> io::Result<(i128, Self)> {
        let q = self.numerator.div_euclid(self.denominator);
        let r = self.numerator.rem_euclid(self.denominator);
        if r != 0 && !allow_rounding {
            return Err(invalid(0, "conversion requires rounding permission"));
        }
        let up = r > self.denominator - r || r == self.denominator - r && q % 2 != 0;
        let result = q + i128::from(up);
        let error = Self::new(
            result
                .checked_mul(self.denominator)
                .and_then(|v| v.checked_sub(self.numerator))
                .ok_or_else(|| invalid(0, "rounding overflow"))?,
            self.denominator,
        )?;
        Ok((result, error))
    }
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

/// Opaque conversion/connection generation identity. It is not a timestamp origin.
pub(crate) fn fresh_identity(kind: &str) -> io::Result<String> {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    };
    static NAMESPACE: OnceLock<String> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let namespace = NAMESPACE
        .get_or_init(|| format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()));
    let mut n = NEXT.load(Ordering::Relaxed);
    loop {
        let next = n
            .checked_add(1)
            .ok_or_else(|| invalid(0, "clock generation space exhausted"))?;
        match NEXT.compare_exchange_weak(n, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(format!("{namespace}:{kind}:{n}")),
            Err(current) => n = current,
        }
    }
}
