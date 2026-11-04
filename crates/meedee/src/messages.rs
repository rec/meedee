//! Immutable musical values. Channels are numbered 1 through 16.
//!
//! Constructors take the essential payload and leave the channel unassigned.
//! Builders validate their values; encoding requires a channel to be assigned.
//! The `Message` trait also exposes optional timestamp, source and packet-span
//! metadata. Builders and note transposition retain it; equality, hashing and
//! wire encoding ignore it.
//!
//! ```
//! use meedee::messages::{Message, NoteOn, PrimitiveMessage, ValueError};
//!
//! let note = NoteOn::new(60)?.with_velocity(70)?.with_channel(2)?
//!     .with_stream("keyboard")?;
//! println!("{:?}", note.transpose(12).encode()?);
//! # Ok::<(), ValueError>(())
//! ```

pub use crate::sequence::{Beats, Note, NoteDuration, Rest};
use std::error::Error;
use std::fmt;

/// Optional provenance. It does not affect message equality, hashing or encoding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Metadata {
    pub timestamp: Option<crate::events::Timestamp>,
    pub stream: Option<String>,
    pub span: Option<crate::events::SourceSpan>,
}
impl Metadata {
    pub fn validate(&self) -> Result<(), ValueError> {
        if self.stream.as_ref().is_some_and(|s| s.is_empty()) {
            return Err(ValueError("empty source identity".into()));
        }
        if self
            .timestamp
            .as_ref()
            .is_some_and(|t| t.clock.mode() == Some(false))
        {
            return Err(ValueError("disabled clock cannot have a timestamp".into()));
        }
        if let Some(span) = &self.span {
            span.validate(self.timestamp.as_ref())
                .map_err(|e| ValueError(e.to_string()))?;
        }
        Ok(())
    }
}

/// Common immutable musical value, primitive message or compound command.
/// Metadata describes an occurrence; equality and hashing describe its MIDI value.
pub trait Message: fmt::Debug {
    fn metadata(&self) -> Option<&Metadata>;
    fn with_metadata(self, metadata: Metadata) -> Result<Self, ValueError>
    where
        Self: Sized;
    fn without_metadata(self) -> Self
    where
        Self: Sized;

    fn timestamp(&self) -> Option<&crate::events::Timestamp> {
        self.metadata().and_then(|m| m.timestamp.as_ref())
    }
    fn stream(&self) -> Option<&str> {
        self.metadata().and_then(|m| m.stream.as_deref())
    }
    fn span(&self) -> Option<&crate::events::SourceSpan> {
        self.metadata().and_then(|m| m.span.as_ref())
    }
    fn with_timestamp(self, timestamp: crate::events::Timestamp) -> Result<Self, ValueError>
    where
        Self: Sized,
    {
        let mut metadata = self.metadata().cloned().unwrap_or_default();
        metadata.timestamp = Some(timestamp);
        self.with_metadata(metadata)
    }
    fn with_stream(self, source: impl Into<String>) -> Result<Self, ValueError>
    where
        Self: Sized,
    {
        let mut metadata = self.metadata().cloned().unwrap_or_default();
        metadata.stream = Some(source.into());
        self.with_metadata(metadata)
    }
    fn with_span(self, span: crate::events::SourceSpan) -> Result<Self, ValueError>
    where
        Self: Sized,
    {
        let mut metadata = self.metadata().cloned().unwrap_or_default();
        metadata.span = Some(span);
        self.with_metadata(metadata)
    }
}

macro_rules! value_metadata {
    ($name:ident { $($field:ident),* $(,)? }) => {
        impl crate::messages::Message for $name {
            fn metadata(&self) -> Option<&crate::messages::Metadata> { self.metadata.as_deref() }
            fn with_metadata(mut self, metadata: crate::messages::Metadata) -> Result<Self, crate::messages::ValueError> {
                metadata.validate()?;
                self.metadata = Some(Box::new(metadata));
                Ok(self)
            }
            fn without_metadata(mut self) -> Self { self.metadata = None; self }
        }
        impl PartialEq for $name {
            fn eq(&self, _other: &Self) -> bool { true $(&& self.$field == _other.$field)* }
        }
        impl Eq for $name {}
        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {
                $(std::hash::Hash::hash(&self.$field, _state);)*
            }
        }
    };
}
pub(crate) use value_metadata;

macro_rules! enum_metadata {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        impl crate::messages::Message for $name {
            fn metadata(&self) -> Option<&crate::messages::Metadata> {
                match self { $(Self::$variant(v) => crate::messages::Message::metadata(v)),* }
            }
            fn with_metadata(self, metadata: crate::messages::Metadata) -> Result<Self, crate::messages::ValueError> {
                match self { $(Self::$variant(v) => Ok(Self::$variant(crate::messages::Message::with_metadata(v, metadata)?))),* }
            }
            fn without_metadata(self) -> Self {
                match self { $(Self::$variant(v) => Self::$variant(crate::messages::Message::without_metadata(v))),* }
            }
        }
    };
}
pub(crate) use enum_metadata;

pub trait PrimitiveMessage: Message {
    /// Encode one complete message, always including its status byte.
    /// Returns an error when a channel message still has an unassigned channel.
    fn encode(&self) -> Result<Vec<u8>, ValueError>;
}

pub trait Command: Message {
    fn expand(&self) -> Result<Vec<AnyMessage>, ValueError>;
}

macro_rules! channel_messages {
    ($($name:ident ($($arg:ident),*) { $($field:ident: $ty:ty => $setter:ident = $low:literal ..= $high:literal => $initial:expr),* };)*) => {
        $(
            #[derive(Clone, Debug)]
            pub struct $name { metadata: Option<Box<Metadata>>, channel: Option<u8>, $($field: $ty),* }
            value_metadata!($name { channel, $($field),* });
            impl $name {
                pub fn new($($arg: i64),*) -> Result<Self, ValueError> {
                    let result = Self { metadata: None, channel: None, $($field: bounded($initial, stringify!($field), $low, $high)? as $ty),* };
                    check_parameter!($name, result)?;
                    Ok(result)
                }
                pub fn channel(&self) -> Option<u8> { self.channel }
                pub fn with_channel(mut self, channel: i64) -> Result<Self, ValueError> {
                    self.channel = Some(bounded(channel, "channel", 1, 16)? as u8);
                    Ok(self)
                }
                pub fn without_channel(mut self) -> Self { self.channel = None; self }
                pub(crate) fn resolved_channel(&self) -> Result<u8, ValueError> {
                    self.channel.ok_or_else(|| ValueError("message channel is unassigned".into()))
                }
                $(pub fn $field(&self) -> $ty { self.$field }
                pub fn $setter(mut self, value: i64) -> Result<Self, ValueError> {
                    self.$field = bounded(value, stringify!($field), $low, $high)? as $ty;
                    check_parameter!($name, self)?;
                    Ok(self)
                })*
            }
        )*
    };
}
macro_rules! check_parameter {
    (RpnSet, $value:expr) => {
        validate_resolution($value.value.into(), $value.resolution.into())
    };
    (NrpnSet, $value:expr) => {
        validate_resolution($value.value.into(), $value.resolution.into())
    };
    ($name:ident, $value:expr) => {
        Ok::<(), ValueError>(())
    };
}
channel_messages! {
    ControlChange14(controller, value) { controller: u8 => with_controller = 0..=31 => controller, value: u16 => with_value = 0..=16383 => value };
    RpnSet(parameter, value) { parameter: u16 => with_parameter = 0..=16382 => parameter, value: u16 => with_value = 0..=16383 => value, resolution: u8 => with_resolution = 7..=14 => 14 };
    NrpnSet(parameter, value) { parameter: u16 => with_parameter = 0..=16383 => parameter, value: u16 => with_value = 0..=16383 => value, resolution: u8 => with_resolution = 7..=14 => 14 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ChannelFormat {
    Midi1,
    Midi2 { group: u8 },
}

macro_rules! channel_values {
    ($($name:ident($($arg:ident),*) {
        $($field:ident: $ty:ty => $setter:ident = ($low1:expr, $high1:expr, $low2:expr, $high2:expr) => ($initial1:expr, $initial2:expr)),*
    } { $($extra:ident: $ety:ty => $esetter:ident = $max:expr),* };)*) => {$(
        #[derive(Clone, Debug)]
        pub struct $name { metadata: Option<Box<Metadata>>, format: ChannelFormat, channel: Option<u8>, $($field: $ty,)* $($extra: Option<$ety>,)* }
        value_metadata!($name { format, channel, $($field,)* $($extra,)* });
        impl $name {
            pub fn new($($arg: i64),*) -> Result<Self, ValueError> {
                Ok(Self { metadata: None, format: ChannelFormat::Midi1, channel: None,
                    $($field: bounded($initial1, stringify!($field), $low1, $high1)? as $ty,)* $($extra: None,)* })
            }
            pub fn new_midi2($($arg: i64),*) -> Result<Self, ValueError> {
                Ok(Self { metadata: None, format: ChannelFormat::Midi2 { group: 1 }, channel: None,
                    $($field: bounded($initial2, stringify!($field), $low2, $high2)? as $ty,)* $($extra: Some(0),)* })
            }
            pub fn version(&self) -> u8 { match self.format { ChannelFormat::Midi1 => 1, ChannelFormat::Midi2 { .. } => 2 } }
            pub fn group(&self) -> Option<u8> { match self.format { ChannelFormat::Midi1 => None, ChannelFormat::Midi2 { group } => Some(group) } }
            pub fn with_group(mut self, group: i64) -> Result<Self, ValueError> {
                if self.version() != 2 { return Err(ValueError("group requires MIDI 2; use Midi1Packet for MIDI 1 UMP".into())); }
                self.format = ChannelFormat::Midi2 { group: bounded(group, "group", 1, 16)? as u8 }; Ok(self)
            }
            pub fn channel(&self) -> Option<u8> { self.channel }
            pub fn with_channel(mut self, channel: i64) -> Result<Self, ValueError> {
                self.channel = Some(bounded(channel, "channel", 1, 16)? as u8); Ok(self)
            }
            pub fn without_channel(mut self) -> Self { self.channel = None; self }
            pub(crate) fn resolved_channel(&self) -> Result<u8, ValueError> {
                self.channel.ok_or_else(|| ValueError("message channel is unassigned".into()))
            }
            pub(crate) fn midi1_channel(&self) -> Result<u8, ValueError> {
                if self.version() != 1 { return Err(ValueError("MIDI 2 requires UMP encoding; convert explicitly for MIDI 1".into())); }
                self.resolved_channel()
            }
            $(pub fn $field(&self) -> $ty { self.$field }
            pub fn $setter(mut self, value: i64) -> Result<Self, ValueError> {
                let (lo, hi) = if self.version() == 1 { ($low1, $high1) } else { ($low2, $high2) };
                self.$field = bounded(value, stringify!($field), lo, hi)? as $ty; Ok(self)
            })*
            $(pub fn $extra(&self) -> Option<$ety> { self.$extra }
            pub fn $esetter(mut self, value: i64) -> Result<Self, ValueError> {
                if self.version() != 2 { return Err(ValueError(concat!(stringify!($extra), " requires MIDI 2").into())); }
                self.$extra = Some(bounded(value, stringify!($extra), 0, $max)? as $ety);
                validate_channel_extra!($name, self)?; Ok(self)
            })*
        }
    )*};
}
macro_rules! validate_channel_extra {
    (NoteOn, $v:expr) => {
        validate_note_attributes($v.attribute_type.unwrap(), $v.attribute_data.unwrap())
    };
    (NoteOff, $v:expr) => {
        validate_note_attributes($v.attribute_type.unwrap(), $v.attribute_data.unwrap())
    };
    (ProgramChange, $v:expr) => {
        if $v.options == Some(0) && ($v.bank_msb != Some(0) || $v.bank_lsb != Some(0)) {
            Err(ValueError(
                "bank bytes must be zero without bank-valid option".into(),
            ))
        } else {
            Ok(())
        }
    };
}
fn validate_note_attributes(kind: u8, data: u16) -> Result<(), ValueError> {
    if kind == 0 && data != 0 {
        Err(ValueError(
            "attribute_data must be zero for attribute_type=0".into(),
        ))
    } else {
        Ok(())
    }
}
channel_values! {
    NoteOn(note) {
        note: u8 => with_note = (0,127,0,127) => (note,note),
        velocity: u16 => with_velocity = (0,127,0,65535) => (90, crate::ump::scale_up(90,7,16)? as i64)
    } { attribute_type: u8 => with_attribute_type = 3, attribute_data: u16 => with_attribute_data = 65535 };
    NoteOff(note) {
        note: u8 => with_note = (0,127,0,127) => (note,note),
        velocity: u16 => with_velocity = (0,127,0,65535) => (0,0)
    } { attribute_type: u8 => with_attribute_type = 3, attribute_data: u16 => with_attribute_data = 65535 };
    PolyPressure(note) {
        note: u8 => with_note = (0,127,0,127) => (note,note),
        pressure: u32 => with_pressure = (0,127,0,4294967295) => (0,0)
    } {};
    ControlChange(controller,value) {
        controller: u8 => with_controller = (0,127,0,127) => (controller,controller),
        value: u32 => with_value = (0,127,0,4294967295) => (value,value)
    } {};
    ProgramChange(program) {
        program: u8 => with_program = (1,128,1,128) => (program,program)
    } { options: u8 => with_options = 1, bank_msb: u8 => with_bank_msb = 127, bank_lsb: u8 => with_bank_lsb = 127 };
    ChannelPressure() { pressure: u32 => with_pressure = (0,127,0,4294967295) => (0,0) } {};
    PitchBend() { value: i32 => with_value = (-8192,8191,-2147483648,2147483647) => (0,0) } {};
}

/// A single relative step of a registered or non-registered parameter.
#[derive(Clone, Debug)]
pub struct ParameterStep {
    metadata: Option<Box<Metadata>>,
    channel: Option<u8>,
    parameter: u16,
    family: crate::controls::ParameterFamily,
    direction: crate::controls::StepDirection,
}
value_metadata!(ParameterStep {
    channel,
    parameter,
    family,
    direction
});
impl ParameterStep {
    pub fn new(parameter: i64) -> Result<Self, ValueError> {
        Self {
            metadata: None,
            channel: None,
            parameter: 0,
            family: crate::controls::ParameterFamily::Rpn,
            direction: crate::controls::StepDirection::Increment,
        }
        .with_parameter(parameter)
    }
    pub fn parameter(&self) -> u16 {
        self.parameter
    }
    pub fn channel(&self) -> Option<u8> {
        self.channel
    }
    pub fn family(&self) -> crate::controls::ParameterFamily {
        self.family
    }
    pub fn direction(&self) -> crate::controls::StepDirection {
        self.direction
    }
    pub fn with_parameter(mut self, parameter: i64) -> Result<Self, ValueError> {
        let maximum = if self.family == crate::controls::ParameterFamily::Rpn {
            16382
        } else {
            16383
        };
        self.parameter = bounded(parameter, "parameter", 0, maximum)? as u16;
        Ok(self)
    }
    pub fn with_family(
        mut self,
        family: crate::controls::ParameterFamily,
    ) -> Result<Self, ValueError> {
        self.family = family;
        let parameter = self.parameter;
        self.with_parameter(parameter.into())
    }
    pub fn with_direction(mut self, direction: crate::controls::StepDirection) -> Self {
        self.direction = direction;
        self
    }
    pub fn with_channel(mut self, channel: i64) -> Result<Self, ValueError> {
        self.channel = Some(bounded(channel, "channel", 1, 16)? as u8);
        Ok(self)
    }
    pub fn without_channel(mut self) -> Self {
        self.channel = None;
        self
    }
    pub(crate) fn resolved_channel(&self) -> Result<u8, ValueError> {
        self.channel
            .ok_or_else(|| ValueError("message channel is unassigned".into()))
    }
}

macro_rules! transpose_note {
    ($($name:ident),*) => {$(impl $name {
        pub fn transpose(mut self, semitones: i64) -> Self {
            self.note = i64::from(self.note).saturating_add(semitones).clamp(0,127) as u8;
            self
        }
    })*};
}
transpose_note!(NoteOn, NoteOff);

macro_rules! system_messages {
    ($($name:ident { $($field:ident: $ty:ty = $low:literal ..= $high:literal),* };)*) => {
        $(#[derive(Clone, Debug)]
        pub struct $name { metadata: Option<Box<Metadata>>, $($field: $ty),* }
        value_metadata!($name { $($field),* });
        impl $name {
            pub fn new($($field: i64),*) -> Result<Self, ValueError> {
                Ok(Self { metadata: None, $($field: bounded($field, stringify!($field), $low, $high)? as $ty),* })
            }
            $(pub fn $field(&self) -> $ty { self.$field })*
        })*
    };
}
system_messages! {
    QuarterFrame { message_type: u8 = 0..=7, value: u8 = 0..=15 };
    SongPosition { position: u16 = 0..=16383 };
    SongSelect { song: u8 = 0..=127 };
    TuneRequest {}; TimingClock {}; Start {}; Continue {}; Stop {}; ActiveSensing {}; SystemReset {};
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnyMessage {
    NoteOn(NoteOn),
    NoteOff(NoteOff),
    PolyPressure(PolyPressure),
    ControlChange(ControlChange),
    ProgramChange(ProgramChange),
    ChannelPressure(ChannelPressure),
    PitchBend(PitchBend),
    QuarterFrame(QuarterFrame),
    SongPosition(SongPosition),
    SongSelect(SongSelect),
    TuneRequest(TuneRequest),
    TimingClock(TimingClock),
    Start(Start),
    Continue(Continue),
    Stop(Stop),
    ActiveSensing(ActiveSensing),
    SystemReset(SystemReset),
    SysEx(SysEx),
}
enum_metadata!(AnyMessage {
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
});
impl AnyMessage {
    pub fn version(&self) -> u8 {
        match self {
            Self::NoteOn(v) => v.version(),
            Self::NoteOff(v) => v.version(),
            Self::PolyPressure(v) => v.version(),
            Self::ControlChange(v) => v.version(),
            Self::ProgramChange(v) => v.version(),
            Self::ChannelPressure(v) => v.version(),
            Self::PitchBend(v) => v.version(),
            _ => 1,
        }
    }

    /// Encoded byte count, independent of channel assignment.
    pub fn encoded_len(&self) -> usize {
        if self.version() == 2 {
            return 8;
        }
        match self {
            Self::SysEx(v) => v.data().len() + 2,
            Self::NoteOn(_)
            | Self::NoteOff(_)
            | Self::PolyPressure(_)
            | Self::ControlChange(_)
            | Self::PitchBend(_)
            | Self::SongPosition(_) => 3,
            Self::ProgramChange(_)
            | Self::ChannelPressure(_)
            | Self::QuarterFrame(_)
            | Self::SongSelect(_) => 2,
            _ => 1,
        }
    }
}

/// Seven-bit SysEx payload, without the F0/F7 framing bytes.
#[derive(Clone, Debug)]
pub struct SysEx {
    metadata: Option<Box<crate::messages::Metadata>>,
    data: Box<[u8]>,
}
crate::messages::value_metadata!(SysEx { data });

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnyCommand {
    ControlChange14(ControlChange14),
    RpnSet(RpnSet),
    NrpnSet(NrpnSet),
    ParameterStep(ParameterStep),
    BankProgramSelect(crate::gm::BankProgramSelect),
    ZoneConfiguration(crate::mpe::ZoneConfiguration),
}
enum_metadata!(AnyCommand {
    ControlChange14,
    RpnSet,
    NrpnSet,
    ParameterStep,
    BankProgramSelect,
    ZoneConfiguration
});

fn validate_resolution(value: i64, resolution: i64) -> Result<(), ValueError> {
    if resolution != 7 && resolution != 14 {
        return Err(ValueError("resolution must be 7 or 14".into()));
    }
    bounded(value, "value", 0, if resolution == 7 { 127 } else { 16383 })?;
    Ok(())
}

impl SysEx {
    pub fn new(data: Vec<u8>) -> Result<Self, ValueError> {
        if data.iter().any(|value| *value > 127) {
            return Err(ValueError("SysEx payload bytes must be in 0..=127".into()));
        }
        Ok(Self {
            metadata: None,
            data: data.into_boxed_slice(),
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueError(pub(crate) String);

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ValueError {}

impl From<ValueError> for std::io::Error {
    fn from(error: ValueError) -> Self {
        Self::new(std::io::ErrorKind::InvalidInput, error)
    }
}

fn bounded(value: i64, field: &str, low: i64, high: i64) -> Result<i64, ValueError> {
    if !(low..=high).contains(&value) {
        return Err(ValueError(format!("{field} must be in {low}..={high}")));
    }
    Ok(value)
}

/// Preserve occurrence information when one value expands into several messages.
pub(crate) fn inherit_metadata<M: Message>(
    source: &(impl Message + ?Sized),
    messages: Vec<M>,
) -> Result<Vec<M>, ValueError> {
    if let Some(metadata) = source.metadata() {
        messages
            .into_iter()
            .map(|m| m.with_metadata(metadata.clone()))
            .collect()
    } else {
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_preserves_boundary_values_and_reports_invalid_fields() {
        let first = NoteOn::new(0)
            .and_then(|m| m.with_velocity(0))
            .and_then(|m| m.with_channel(1))
            .unwrap();
        assert_eq!(
            (first.channel(), first.note(), first.velocity()),
            (Some(1), 0, 0)
        );
        let last = NoteOn::new(127)
            .and_then(|m| m.with_velocity(127))
            .and_then(|m| m.with_channel(16))
            .unwrap();
        assert_eq!(
            (last.channel(), last.note(), last.velocity()),
            (Some(16), 127, 127)
        );
        for (channel, note, velocity, expected) in [
            (0, 60, 100, "channel must be in 1..=16"),
            (17, 60, 100, "channel must be in 1..=16"),
            (1, -1, 100, "note must be in 0..=127"),
            (1, 128, 100, "note must be in 0..=127"),
            (1, 60, -1, "velocity must be in 0..=127"),
            (1, 60, 128, "velocity must be in 0..=127"),
        ] {
            assert_eq!(
                NoteOn::new(note)
                    .and_then(|m| m.with_velocity(velocity))
                    .and_then(|m| m.with_channel(channel))
                    .unwrap_err()
                    .to_string(),
                expected
            );
        }
    }

    #[test]
    fn sysex_owns_seven_bit_payload_and_rejects_framing_bytes() {
        let message = SysEx::new(vec![0, 127]).unwrap();
        assert_eq!(message.data(), &[0, 127]);
        assert_eq!(message.clone(), message);
        assert!(SysEx::new(Vec::new()).unwrap().data().is_empty());
        for byte in [128, 240, 247, 255] {
            assert_eq!(
                SysEx::new(vec![byte]).unwrap_err().to_string(),
                "SysEx payload bytes must be in 0..=127"
            );
        }
    }
}
