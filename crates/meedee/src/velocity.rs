//! CA-031 interpretation, one fixed state table per independent source.
use crate::messages::AnyMessage;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VelocityObservation {
    pub channel: u8,
    pub note: u8,
    pub velocity: u16,
    pub resolution: u8,
    pub note_on: bool,
    pub zero_velocity_note_on: bool,
}
#[derive(Clone, Debug, Default)]
pub struct VelocityInterpreter {
    prefix: [Option<u8>; 16],
}
impl VelocityInterpreter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn reset(&mut self) {
        self.prefix.fill(None);
    }
    pub fn feed(&mut self, message: &AnyMessage) -> Option<VelocityObservation> {
        use AnyMessage::*;
        let (channel, note, velocity, on) = match message {
            ControlChange(v) => {
                if v.controller() == 88 {
                    self.prefix[usize::from(v.channel() - 1)] = Some(v.value());
                } else if v.controller() == 121 {
                    self.prefix[usize::from(v.channel() - 1)] = None;
                }
                return None;
            }
            SystemReset(_) => {
                self.reset();
                return None;
            }
            NoteOn(v) => (v.channel(), v.note(), v.velocity(), true),
            NoteOff(v) => (v.channel(), v.note(), v.velocity(), false),
            _ => return None,
        };
        let prefix = self.prefix[usize::from(channel - 1)].take();
        let zero = on && velocity == 0;
        Some(VelocityObservation {
            channel,
            note,
            velocity: if zero {
                0
            } else {
                if let Some(p) = prefix {
                    (u16::from(velocity) << 7) | u16::from(p)
                } else {
                    u16::from(velocity)
                }
            },
            resolution: if prefix.is_some() && !zero { 14 } else { 7 },
            note_on: on && !zero,
            zero_velocity_note_on: zero,
        })
    }
}
