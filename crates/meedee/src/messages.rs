//! Immutable musical values. Channels are numbered 1 through 16.
//!
//! Constructors take the essential payload and leave the channel unassigned.
//! Builders validate their values; encoding requires a channel to be assigned.
//!
//! ```
//! use meedee::messages::{NoteOn, PrimitiveMessage, ValueError};
//!
//! let note = NoteOn::new(60)?.with_velocity(70)?.with_channel(2)?;
//! println!("{:?}", note.transpose(12).encode()?);
//! # Ok::<(), ValueError>(())
//! ```

use std::error::Error;
use std::fmt;

/// Common immutable musical value, primitive message or compound command.
pub trait Message: fmt::Debug {}

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
            #[derive(Clone, Debug, PartialEq, Eq, Hash)]
            pub struct $name { channel: Option<u8>, $($field: $ty),* }
            impl Message for $name {}
            impl $name {
                pub fn new($($arg: i64),*) -> Result<Self, ValueError> {
                    let result = Self { channel: None, $($field: bounded($initial, stringify!($field), $low, $high)? as $ty),* };
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
    NoteOn(note) { note: u8 => with_note = 0..=127 => note, velocity: u8 => with_velocity = 0..=127 => 90 };
    NoteOff(note) { note: u8 => with_note = 0..=127 => note, velocity: u8 => with_velocity = 0..=127 => 0 };
    PolyPressure(note) { note: u8 => with_note = 0..=127 => note, pressure: u8 => with_pressure = 0..=127 => 0 };
    ControlChange(controller, value) { controller: u8 => with_controller = 0..=127 => controller, value: u8 => with_value = 0..=127 => value };
    ProgramChange(program) { program: u8 => with_program = 1..=128 => program };
    ChannelPressure() { pressure: u8 => with_pressure = 0..=127 => 0 };
    PitchBend() { value: i16 => with_value = -8192..=8191 => 0 };
    ControlChange14(controller, value) { controller: u8 => with_controller = 0..=31 => controller, value: u16 => with_value = 0..=16383 => value };
    RpnSet(parameter, value) { parameter: u16 => with_parameter = 0..=16382 => parameter, value: u16 => with_value = 0..=16383 => value, resolution: u8 => with_resolution = 7..=14 => 14 };
    NrpnSet(parameter, value) { parameter: u16 => with_parameter = 0..=16383 => parameter, value: u16 => with_value = 0..=16383 => value, resolution: u8 => with_resolution = 7..=14 => 14 };
    RpnIncrement(parameter) { parameter: u16 => with_parameter = 0..=16382 => parameter };
    RpnDecrement(parameter) { parameter: u16 => with_parameter = 0..=16382 => parameter };
    NrpnIncrement(parameter) { parameter: u16 => with_parameter = 0..=16383 => parameter };
    NrpnDecrement(parameter) { parameter: u16 => with_parameter = 0..=16383 => parameter };
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
        $(#[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name { $($field: $ty),* }
        impl Message for $name {}
        impl $name {
            pub fn new($($field: i64),*) -> Result<Self, ValueError> {
                Ok(Self { $($field: bounded($field, stringify!($field), $low, $high)? as $ty),* })
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
impl Message for AnyMessage {}
impl AnyMessage {
    /// Encoded byte count, independent of channel assignment.
    pub fn encoded_len(&self) -> usize {
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SysEx {
    data: Box<[u8]>,
}
impl Message for SysEx {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnyCommand {
    ControlChange14(ControlChange14),
    RpnSet(RpnSet),
    NrpnSet(NrpnSet),
    RpnIncrement(RpnIncrement),
    RpnDecrement(RpnDecrement),
    NrpnIncrement(NrpnIncrement),
    NrpnDecrement(NrpnDecrement),
    BankProgramSelect(crate::gm::BankProgramSelect),
    ZoneConfiguration(crate::mpe::ZoneConfiguration),
}
impl Message for AnyCommand {}

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
