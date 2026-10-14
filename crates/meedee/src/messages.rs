//! Immutable musical values. Channels are numbered 1 through 16.

use std::error::Error;
use std::fmt;

/// Common immutable musical value, primitive message or compound command.
pub trait Message: fmt::Debug {}

pub trait PrimitiveMessage: Message {
    /// Encode one complete message, always including its status byte.
    fn encode(&self) -> Vec<u8>;
}

pub trait Command: Message {
    fn expand(&self) -> Vec<ControlChange>;
}

macro_rules! short_messages {
    ($($name:ident { $($field:ident: $ty:ty = $low:literal ..= $high:literal),* };)*) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq, Hash)]
            pub struct $name { $($field: $ty),* }
            impl Message for $name {}
            impl $name {
                pub fn new($($field: i64),*) -> Result<Self, ValueError> {
                    Ok(Self { $($field: bounded($field, stringify!($field), $low, $high)? as $ty),* })
                }
                $(pub fn $field(&self) -> $ty { self.$field })*
            }
        )*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum AnyMessage { $($name($name),)* SysEx(SysEx) }
        impl Message for AnyMessage {}
    };
}

short_messages! {
    NoteOn { channel: u8 = 1..=16, note: u8 = 0..=127, velocity: u8 = 0..=127 };
    NoteOff { channel: u8 = 1..=16, note: u8 = 0..=127, velocity: u8 = 0..=127 };
    PolyPressure { channel: u8 = 1..=16, note: u8 = 0..=127, pressure: u8 = 0..=127 };
    ControlChange { channel: u8 = 1..=16, controller: u8 = 0..=127, value: u8 = 0..=127 };
    ProgramChange { channel: u8 = 1..=16, program: u8 = 1..=128 };
    ChannelPressure { channel: u8 = 1..=16, pressure: u8 = 0..=127 };
    PitchBend { channel: u8 = 1..=16, value: i16 = -8192..=8191 };
    QuarterFrame { message_type: u8 = 0..=7, value: u8 = 0..=15 };
    SongPosition { position: u16 = 0..=16383 };
    SongSelect { song: u8 = 0..=127 };
    TuneRequest {};
    TimingClock {};
    Start {};
    Continue {};
    Stop {};
    ActiveSensing {};
    SystemReset {};
}

/// Seven-bit SysEx payload, without the F0/F7 framing bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SysEx {
    data: Box<[u8]>,
}
impl Message for SysEx {}

macro_rules! commands {
    ($($name:ident { $($field:ident: $ty:ty = $low:literal ..= $high:literal),* } $check:block;)*) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq, Hash)]
            pub struct $name { $($field: $ty),* }
            impl Message for $name {}
            impl $name {
                pub fn new($($field: i64),*) -> Result<Self, ValueError> {
                    $check
                    Ok(Self { $($field: bounded($field, stringify!($field), $low, $high)? as $ty),* })
                }
                $(pub fn $field(&self) -> $ty { self.$field })*
            }
        )*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum AnyCommand { $($name($name)),* }
        impl Message for AnyCommand {}
    };
}

commands! {
    ControlChange14 { channel: u8 = 1..=16, controller: u8 = 0..=31, value: u16 = 0..=16383 } {};
    RpnSet { channel: u8 = 1..=16, parameter: u16 = 0..=16382, value: u16 = 0..=16383, resolution: u8 = 7..=14 } { validate_resolution(value, resolution)?; };
    NrpnSet { channel: u8 = 1..=16, parameter: u16 = 0..=16383, value: u16 = 0..=16383, resolution: u8 = 7..=14 } { validate_resolution(value, resolution)?; };
    RpnIncrement { channel: u8 = 1..=16, parameter: u16 = 0..=16382 } {};
    RpnDecrement { channel: u8 = 1..=16, parameter: u16 = 0..=16382 } {};
    NrpnIncrement { channel: u8 = 1..=16, parameter: u16 = 0..=16383 } {};
    NrpnDecrement { channel: u8 = 1..=16, parameter: u16 = 0..=16383 } {};
}

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
        let first = NoteOn::new(1, 0, 0).unwrap();
        assert_eq!((first.channel(), first.note(), first.velocity()), (1, 0, 0));
        let last = NoteOn::new(16, 127, 127).unwrap();
        assert_eq!(
            (last.channel(), last.note(), last.velocity()),
            (16, 127, 127)
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
                NoteOn::new(channel, note, velocity)
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
