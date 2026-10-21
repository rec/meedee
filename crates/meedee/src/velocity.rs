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
#[derive(Clone, Copy, Debug)]
struct Prefix {
    value: u8,
    index: usize,
}
#[derive(Clone, Debug, Default)]
pub struct VelocityInterpreter {
    prefix: [Option<Prefix>; 16],
    next_index: usize,
}
impl VelocityInterpreter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn reset(&mut self) {
        self.prefix.fill(None);
    }
    pub fn feed(&mut self, message: &AnyMessage) -> Option<VelocityObservation> {
        self.feed_spanned(message).map(|(v, _, _)| v)
    }
    pub fn feed_spanned(
        &mut self,
        message: &AnyMessage,
    ) -> Option<(VelocityObservation, usize, usize)> {
        let index = self.next_index;
        self.skip();
        let channel = match message {
            AnyMessage::NoteOn(v) => Some(v.channel()),
            AnyMessage::NoteOff(v) => Some(v.channel()),
            _ => None,
        };
        let first = channel
            .and_then(|c| self.prefix[usize::from(c - 1)])
            .map(|v| v.index);
        let value = self.accept(message, index)?;
        let first = if value.resolution == 14 {
            first.unwrap_or(index)
        } else {
            index
        };
        Some((value, first, index))
    }
    pub(crate) fn skip(&mut self) {
        self.next_index = self.next_index.saturating_add(1);
    }
    pub(crate) fn retained_indexes(&self) -> Vec<usize> {
        self.prefix.iter().flatten().map(|v| v.index).collect()
    }
    fn accept(&mut self, message: &AnyMessage, index: usize) -> Option<VelocityObservation> {
        use AnyMessage::*;
        let (channel, note, velocity, on) = match message {
            ControlChange(v) => {
                if v.controller() == 88 {
                    self.prefix[usize::from(v.channel() - 1)] = Some(Prefix {
                        value: v.value(),
                        index,
                    });
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
            } else if let Some(p) = prefix {
                (u16::from(velocity) << 7) | u16::from(p.value)
            } else {
                u16::from(velocity)
            },
            resolution: if prefix.is_some() && !zero { 14 } else { 7 },
            note_on: on && !zero,
            zero_velocity_note_on: zero,
        })
    }
}
