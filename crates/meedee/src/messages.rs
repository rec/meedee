//! Immutable musical values. Channels are numbered 1 through 16.

use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteOn {
    channel: u8,
    note: u8,
    velocity: u8,
}

impl NoteOn {
    pub fn new(channel: i64, note: i64, velocity: i64) -> Result<Self, ValueError> {
        Ok(Self {
            channel: bounded(channel, "channel", 1, 16)?,
            note: bounded(note, "note", 0, 127)?,
            velocity: bounded(velocity, "velocity", 0, 127)?,
        })
    }

    pub fn channel(&self) -> u8 {
        self.channel
    }
    pub fn note(&self) -> u8 {
        self.note
    }
    pub fn velocity(&self) -> u8 {
        self.velocity
    }
}

/// Seven-bit SysEx payload, without the F0/F7 framing bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SysEx {
    data: Box<[u8]>,
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
pub struct ValueError(String);

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ValueError {}

fn bounded(value: i64, field: &str, low: i64, high: i64) -> Result<u8, ValueError> {
    if !(low..=high).contains(&value) {
        return Err(ValueError(format!("{field} must be in {low}..={high}")));
    }
    Ok(value as u8)
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
