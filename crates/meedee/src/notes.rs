//! Bounded observations of note traffic, not a model of an instrument's voices.
use crate::{events::Payload, messages, ump};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteState {
    pub note: u8,
    pub held: u64,
    /// Matched releases since the last boundary, not pedal-held voices.
    pub released: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelState {
    pub source: String,
    pub group: Option<u8>,
    pub channel: u8,
    pub protocol: &'static str,
    pub notes: Vec<NoteState>,
    pub sustain: Option<u32>,
    pub sostenuto: Option<u32>,
    pub unmatched_releases: u64,
    /// Known loss, unmatched releases or exhausted counts affected this epoch.
    /// False does not establish a known instrument state before observation.
    pub uncertain: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub channels: Vec<ChannelState>,
    pub omitted_events: u64,
    pub boundaries: u64,
}

/// One serialized caller; sources, groups and protocol families never merge.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    channels: BTreeMap<(String, Option<u8>, u8, &'static str), Channel>,
    sources: BTreeMap<String, bool>,
    omitted_events: u64,
    boundaries: u64,
}

impl Tracker {
    pub fn snapshot(&self) -> Report {
        Report {
            channels: self.channels.iter().map(|(k, v)| v.snapshot(k)).collect(),
            omitted_events: self.omitted_events,
            boundaries: self.boundaries,
        }
    }

    /// Copy only one addressed channel's protocol views for live presentation.
    pub fn channel_states(
        &self,
        source: &str,
        group: Option<u8>,
        channel: u8,
    ) -> Vec<ChannelState> {
        self.channels
            .iter()
            .filter(|(k, _)| k.0 == source && k.1 == group && k.2 == channel)
            .map(|(k, v)| v.snapshot(k))
            .collect()
    }

    /// Forget affected history without inventing releases or sending anything.
    pub fn boundary(&mut self, source: &str, limit: usize) {
        self.boundaries = self.boundaries.saturating_add(1);
        if self.admit_source(source, limit) {
            self.sources.insert(source.into(), true);
        }
        for (k, c) in &mut self.channels {
            if k.0 == source {
                c.invalidate();
            }
        }
    }

    /// Only note/pedal/mode messages admit a channel. Limits never evict a
    /// previously admitted channel; omitted_events counts untracked messages.
    pub fn feed(&mut self, source: &str, message: &Payload, clock: Option<&str>, limit: usize) {
        if let Some(clock) = clock {
            if self
                .channels
                .iter()
                .any(|(k, c)| k.0 == source && c.clock.as_deref().is_some_and(|v| v != clock))
            {
                self.boundary(source, limit);
            }
            for (k, c) in &mut self.channels {
                if k.0 == source {
                    c.clock = Some(clock.into());
                }
            }
        }
        let Some((group, protocol, channel, action)) = action(message) else {
            return;
        };
        if !self.admit_source(source, limit) {
            return;
        }
        if matches!(action, Action::Reset) {
            self.boundaries = self.boundaries.saturating_add(1);
            self.sources.insert(source.into(), true);
            for (k, c) in &mut self.channels {
                if k.0 == source && k.1 == group {
                    c.invalidate();
                }
            }
            return;
        }
        let key = (source.into(), group, channel, protocol);
        if !self.channels.contains_key(&key) && self.channels.len() >= limit {
            self.omitted_events = self.omitted_events.saturating_add(1);
            return;
        }
        let c = self.channels.entry(key).or_insert_with(|| Channel {
            uncertain: self.sources[source],
            ..Channel::default()
        });
        if let Some(clock) = clock {
            c.clock = Some(clock.into());
        }
        match action {
            Action::On(note) => {
                let count = &mut c.notes[usize::from(note)].0;
                c.uncertain |= *count == u64::MAX;
                *count = count.saturating_add(1);
            }
            Action::Off(note) => {
                let n = &mut c.notes[usize::from(note)];
                if n.0 == 0 {
                    c.unmatched_releases = c.unmatched_releases.saturating_add(1);
                    c.uncertain = true;
                } else {
                    n.0 -= 1;
                    c.uncertain |= n.1 == u64::MAX;
                    n.1 = n.1.saturating_add(1);
                }
            }
            Action::Control(controller, value) => match controller {
                64 => c.sustain = Some(value),
                66 => c.sostenuto = Some(value),
                120 => {
                    for n in &mut c.notes {
                        n.0 = 0;
                    }
                }
                121 => {
                    c.sustain = None;
                    c.sostenuto = None;
                }
                123..=127 => {
                    for n in &mut c.notes {
                        c.uncertain |= n.1.checked_add(n.0).is_none();
                        n.1 = n.1.saturating_add(n.0);
                        n.0 = 0;
                    }
                }
                _ => unreachable!("selected control"),
            },
            Action::Reset => unreachable!("reset handled before channel admission"),
        }
    }

    fn admit_source(&mut self, source: &str, limit: usize) -> bool {
        if !self.sources.contains_key(source) && self.sources.len() >= limit {
            self.omitted_events = self.omitted_events.saturating_add(1);
            return false;
        }
        self.sources.entry(source.into()).or_default();
        true
    }
}

#[derive(Clone, Debug)]
struct Channel {
    notes: [(u64, u64); 128],
    sustain: Option<u32>,
    sostenuto: Option<u32>,
    unmatched_releases: u64,
    uncertain: bool,
    clock: Option<String>,
}
impl Default for Channel {
    fn default() -> Self {
        Self {
            notes: [(0, 0); 128],
            sustain: None,
            sostenuto: None,
            unmatched_releases: 0,
            uncertain: false,
            clock: None,
        }
    }
}
impl Channel {
    fn snapshot(&self, key: &(String, Option<u8>, u8, &'static str)) -> ChannelState {
        ChannelState {
            source: key.0.clone(),
            group: key.1,
            channel: key.2,
            protocol: key.3,
            notes: self
                .notes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.0 != 0 || n.1 != 0)
                .map(|(n, c)| NoteState {
                    note: n as u8,
                    held: c.0,
                    released: c.1,
                })
                .collect(),
            sustain: self.sustain,
            sostenuto: self.sostenuto,
            unmatched_releases: self.unmatched_releases,
            uncertain: self.uncertain,
        }
    }
    fn invalidate(&mut self) {
        *self = Self {
            uncertain: true,
            ..Self::default()
        };
    }
}

enum Action {
    On(u8),
    Off(u8),
    Control(u8, u32),
    Reset,
}

fn action(message: &Payload) -> Option<(Option<u8>, &'static str, u8, Action)> {
    match message {
        Payload::Midi1(v) => midi1(v).map(|(c, a)| (None, "midi1", c, a)),
        Payload::Ump(ump::UmpMessage::Midi1(v)) => {
            midi1(v.message()).map(|(c, a)| (Some(v.group()), "midi1", c, a))
        }
        Payload::Ump(ump::UmpMessage::Midi2NoteOn(v)) => {
            Some((Some(v.group()), "midi2", v.channel(), Action::On(v.note())))
        }
        Payload::Ump(ump::UmpMessage::Midi2NoteOff(v)) => {
            Some((Some(v.group()), "midi2", v.channel(), Action::Off(v.note())))
        }
        Payload::Ump(ump::UmpMessage::Midi2ControlChange(v))
            if selected_control(v.controller()) =>
        {
            Some((
                Some(v.group()),
                "midi2",
                v.channel(),
                Action::Control(v.controller(), v.value()),
            ))
        }
        _ => None,
    }
}
fn midi1(message: &messages::AnyMessage) -> Option<(u8, Action)> {
    use messages::AnyMessage::*;
    match message {
        NoteOn(v) if v.velocity() != 0 => Some((v.channel(), Action::On(v.note()))),
        NoteOn(v) => Some((v.channel(), Action::Off(v.note()))),
        NoteOff(v) => Some((v.channel(), Action::Off(v.note()))),
        ControlChange(v) if selected_control(v.controller()) => Some((
            v.channel(),
            Action::Control(v.controller(), v.value().into()),
        )),
        SystemReset(_) => Some((0, Action::Reset)),
        _ => None,
    }
}
fn selected_control(controller: u8) -> bool {
    matches!(controller, 64 | 66 | 120 | 121 | 123..=127)
}
