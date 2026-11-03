//! Full notes and rests use typed elapsed time or exact musical beats.

use crate::events::Rational;
use crate::messages::{NoteOn, ValueError};
use std::io;
use std::time::Duration;

/// Quarter-note beats, retained as an exact nonnegative fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Beats(Rational);

impl Beats {
    pub fn new(numerator: u64, denominator: u64) -> io::Result<Self> {
        Ok(Self(Rational::new(numerator.into(), denominator.into())?))
    }

    pub fn value(self) -> Rational {
        self.0
    }
}

impl From<u32> for Beats {
    fn from(value: u32) -> Self {
        Self(Rational::new(value.into(), 1).expect("whole beats fit Rational"))
    }
}

/// Internal scheduling retains fractions until cumulative deadlines are rounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteDuration {
    Time(Duration),
    Beats(Beats),
}

impl NoteDuration {
    /// Resolve musical time at an explicit tempo in quarter-note beats per minute.
    pub fn seconds(self, bpm: Option<Rational>) -> io::Result<Rational> {
        match self {
            Self::Time(duration) => Rational::new(duration.as_nanos() as i128, 1_000_000_000),
            Self::Beats(beats) => {
                let bpm = bpm.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "beats require a tempo")
                })?;
                if bpm.numerator() <= 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "tempo must be positive",
                    ));
                }
                beats
                    .value()
                    .multiply(60, 1)?
                    .multiply(bpm.denominator(), bpm.numerator())
            }
        }
    }
}

impl From<Duration> for NoteDuration {
    fn from(value: Duration) -> Self {
        Self::Time(value)
    }
}

impl From<Beats> for NoteDuration {
    fn from(value: Beats) -> Self {
        Self::Beats(value)
    }
}

/// An attack and release separated by its duration. Never synthesized by input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Note {
    duration: NoteDuration,
    attack: NoteOn,
}

impl Note {
    pub fn new(duration: impl Into<NoteDuration>, note: i64) -> Result<Self, ValueError> {
        Ok(Self {
            duration: duration.into(),
            attack: NoteOn::new(note)?,
        })
    }
    pub fn with_velocity(mut self, velocity: i64) -> Result<Self, ValueError> {
        self.attack = self.attack.with_velocity(velocity)?;
        Ok(self)
    }
    pub fn with_channel(mut self, channel: i64) -> Result<Self, ValueError> {
        self.attack = self.attack.with_channel(channel)?;
        Ok(self)
    }
    pub fn duration(&self) -> NoteDuration {
        self.duration
    }

    pub fn note(&self) -> u8 {
        self.attack.note()
    }

    pub fn velocity(&self) -> u8 {
        self.attack.velocity()
    }

    pub fn channel(&self) -> Option<u8> {
        self.attack.channel()
    }
}

/// Advance a sequence without sending a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rest {
    duration: NoteDuration,
}

impl Rest {
    pub fn new(duration: impl Into<NoteDuration>) -> Self {
        Self {
            duration: duration.into(),
        }
    }

    pub fn duration(self) -> NoteDuration {
        self.duration
    }
}
