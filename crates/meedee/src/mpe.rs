//! Pure MPE zone configuration, explicit allocation and bounded channel tracking.
use crate::messages::{AnyMessage, Command, Message, RpnSet, ValueError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZoneConfiguration {
    manager_channel: u8,
    members: u8,
}
impl ZoneConfiguration {
    pub fn new(manager_channel: i64, members: i64) -> Result<Self, ValueError> {
        if manager_channel != 1 && manager_channel != 16 {
            return Err(ValueError("manager_channel must be 1 or 16".into()));
        }
        if !(0..=15).contains(&members) {
            return Err(ValueError("members must be in 0..=15".into()));
        }
        Ok(Self {
            manager_channel: manager_channel as u8,
            members: members as u8,
        })
    }
    pub fn manager_channel(&self) -> u8 {
        self.manager_channel
    }
    pub fn members(&self) -> u8 {
        self.members
    }
}
impl Message for ZoneConfiguration {}
impl Command for ZoneConfiguration {
    fn expand(&self) -> Vec<AnyMessage> {
        RpnSet::new(self.manager_channel.into(), 6, self.members.into(), 7)
            .expect("validated MPE configuration fields")
            .expand()
    }
}

/// An allocation token is local to one allocator. No MIDI is sent automatically.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Voice {
    pub id: u64,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}
impl Voice {
    pub fn note_on(&self) -> crate::messages::NoteOn {
        crate::messages::NoteOn::new(self.channel.into(), self.note.into(), self.velocity.into())
            .expect("validated voice")
    }
}
#[derive(Clone, Debug)]
pub struct Allocator {
    zone: ZoneConfiguration,
    voices: [Option<Voice>; 15],
    released: [bool; 15],
    sustain: bool,
    next_id: u64,
}
impl Allocator {
    pub fn new(zone: ZoneConfiguration) -> Self {
        Self {
            zone,
            voices: std::array::from_fn(|_| None),
            released: [false; 15],
            sustain: false,
            next_id: 0,
        }
    }
    pub fn allocate(&mut self, note: i64, velocity: i64) -> Result<Voice, ValueError> {
        crate::messages::NoteOn::new(1, note, velocity)?;
        if velocity == 0 {
            return Err(ValueError(
                "allocation requires a sounding NoteOn velocity".into(),
            ));
        }
        let channels = member_channels(&self.zone);
        let index = (0..channels.len())
            .find(|&i| self.voices[i].is_none())
            .ok_or_else(|| ValueError("MPE zone is full; explicitly release a voice".into()))?;
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| ValueError("voice token space exhausted".into()))?;
        let voice = Voice {
            id: self.next_id,
            channel: channels[index],
            note: note as u8,
            velocity: velocity as u8,
        };
        self.next_id = next;
        self.voices[index] = Some(voice.clone());
        self.released[index] = false;
        Ok(voice)
    }
    pub fn release(
        &mut self,
        id: u64,
        velocity: i64,
    ) -> Result<crate::messages::NoteOff, ValueError> {
        let i = self
            .voices
            .iter()
            .position(|v| v.as_ref().is_some_and(|v| v.id == id))
            .ok_or_else(|| ValueError("unknown voice token".into()))?;
        if self.released[i] {
            return Err(ValueError("voice already released".into()));
        }
        let v = self.voices[i].as_ref().expect("located voice");
        let message = crate::messages::NoteOff::new(v.channel.into(), v.note.into(), velocity)?;
        if self.sustain {
            self.released[i] = true;
        } else {
            self.voices[i] = None;
        }
        Ok(message)
    }
    pub fn set_sustain(&mut self, enabled: bool) -> crate::messages::ControlChange {
        self.sustain = enabled;
        if !enabled {
            for i in 0..15 {
                if self.released[i] {
                    self.voices[i] = None;
                    self.released[i] = false;
                }
            }
        }
        crate::messages::ControlChange::new(
            self.zone.manager_channel.into(),
            64,
            if enabled { 127 } else { 0 },
        )
        .expect("valid sustain")
    }
    /// Forget local state after known loss/reconfiguration. Does not silence a device.
    pub fn reset(&mut self) {
        self.voices.fill(None);
        self.released.fill(false);
        self.sustain = false;
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteState {
    pub note: u8,
    pub held: u16,
    pub sustained: u16,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChannelState {
    pub channel: u8,
    pub notes: Vec<NoteState>,
    pub pitch_bend: i16,
    pub pressure: u8,
    pub timbre: u8,
    pub sustain: bool,
    pub ambiguous: bool,
}
#[derive(Clone, Debug)]
struct Channel {
    held: [u16; 128],
    sustained: [u16; 128],
    bend: i16,
    pressure: u8,
    timbre: u8,
    sustain: bool,
}
impl Default for Channel {
    fn default() -> Self {
        Self {
            held: [0; 128],
            sustained: [0; 128],
            bend: 0,
            pressure: 0,
            timbre: 64,
            sustain: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Tracker {
    zone: ZoneConfiguration,
    channels: [Channel; 16],
}
impl Tracker {
    pub fn new(zone: ZoneConfiguration) -> Self {
        Self {
            zone,
            channels: std::array::from_fn(|_| Channel::default()),
        }
    }
    pub fn configure(&mut self, zone: ZoneConfiguration) {
        self.zone = zone;
        self.reset();
    }
    pub fn reset(&mut self) {
        self.channels = std::array::from_fn(|_| Channel::default());
    }
    pub fn snapshot(&self, channel: i64) -> Result<ChannelState, ValueError> {
        if !(1..=16).contains(&channel) {
            return Err(ValueError("channel must be 1..16".into()));
        }
        let c = &self.channels[(channel - 1) as usize];
        let notes = (0..128)
            .filter(|&n| c.held[n] != 0 || c.sustained[n] != 0)
            .map(|n| NoteState {
                note: n as u8,
                held: c.held[n],
                sustained: c.sustained[n],
            })
            .collect();
        // Member expression belongs to currently held notes only, never release tails.
        let active: u32 = c.held.iter().map(|&v| u32::from(v)).sum();
        Ok(ChannelState {
            channel: channel as u8,
            notes,
            pitch_bend: c.bend,
            pressure: c.pressure,
            timbre: c.timbre,
            sustain: c.sustain,
            ambiguous: active > 1,
        })
    }
    /// Returns changed channel snapshots. Repeated pitches retain counts; no
    /// arbitrary voice matching or per-note expression attribution is invented.
    pub fn feed(
        &mut self,
        message: &crate::messages::AnyMessage,
    ) -> Result<Vec<ChannelState>, ValueError> {
        use crate::messages::AnyMessage::*;
        if matches!(message, SystemReset(_)) {
            self.reset();
            return Ok(Vec::new());
        }
        let channel = match message {
            NoteOn(v) => v.channel(),
            NoteOff(v) => v.channel(),
            PitchBend(v) => v.channel(),
            ChannelPressure(v) => v.channel(),
            ControlChange(v) => v.channel(),
            _ => return Ok(Vec::new()),
        };
        let manager = channel == self.zone.manager_channel;
        if !manager && !member_channels(&self.zone).contains(&channel) {
            return Ok(Vec::new());
        }
        if !manager && matches!(message, ControlChange(v) if v.controller()==64) {
            return Ok(Vec::new());
        }
        let manager_sustain = self.channels[usize::from(self.zone.manager_channel - 1)].sustain;
        let manager_sustain = manager_sustain && !manager;
        let c = &mut self.channels[usize::from(channel - 1)];
        match message {
            NoteOn(v) if v.velocity() != 0 => {
                let n = usize::from(v.note());
                let count = c.held[n]
                    .checked_add(1)
                    .ok_or_else(|| ValueError("MPE repeated-note count exhausted".into()))?;
                // Reserve enough count space for subsequent sustain transfer.
                if u32::from(count) + u32::from(c.sustained[n]) > u32::from(u16::MAX) {
                    return Err(ValueError("MPE note count exhausted".into()));
                }
                c.held[n] = count;
            }
            NoteOn(v) => release_note(c, v.note(), manager_sustain),
            NoteOff(v) => release_note(c, v.note(), manager_sustain),
            PitchBend(v) => c.bend = v.value(),
            ChannelPressure(v) => c.pressure = v.pressure(),
            ControlChange(v) => match v.controller() {
                64 => {
                    c.sustain = v.value() >= 64;
                    if !c.sustain && !manager_sustain {
                        c.sustained.fill(0);
                    }
                }
                74 => c.timbre = v.value(),
                120 => {
                    c.held.fill(0);
                    c.sustained.fill(0);
                }
                123..=127 => {
                    for n in 0..128 {
                        if c.sustain || manager_sustain {
                            c.sustained[n] += c.held[n];
                        }
                        c.held[n] = 0;
                    }
                }
                121 => {
                    c.bend = 0;
                    c.pressure = 0;
                    c.timbre = 64;
                    c.sustain = false;
                    if !manager_sustain {
                        c.sustained.fill(0);
                    }
                }
                _ => return Ok(Vec::new()),
            },
            _ => return Ok(Vec::new()),
        }
        let mut changed = vec![self.snapshot(channel.into())?];
        let sustain_now = self.channels[usize::from(channel - 1)].sustain;
        if manager {
            if let ControlChange(v) = message {
                if matches!(v.controller(), 64 | 120 | 121 | 123..=127) {
                    for member in member_channels(&self.zone) {
                        let c = &mut self.channels[usize::from(member - 1)];
                        match v.controller() {
                            64 if v.value() < 64 => c.sustained.fill(0),
                            120 => {
                                c.held.fill(0);
                                c.sustained.fill(0);
                            }
                            121 => {
                                c.bend = 0;
                                c.pressure = 0;
                                c.timbre = 64;
                                c.sustain = false;
                                c.sustained.fill(0);
                            }
                            123..=127 => {
                                for n in 0..128 {
                                    if sustain_now {
                                        c.sustained[n] += c.held[n];
                                    }
                                    c.held[n] = 0;
                                }
                            }
                            _ => {}
                        }
                        changed.push(self.snapshot(member.into())?);
                    }
                }
            }
        }
        Ok(changed)
    }
}
fn release_note(c: &mut Channel, note: u8, manager_sustain: bool) {
    let n = usize::from(note);
    if c.held[n] > 0 {
        c.held[n] -= 1;
        if c.sustain || manager_sustain {
            c.sustained[n] += 1;
        }
    }
}
fn member_channels(zone: &ZoneConfiguration) -> Vec<u8> {
    if zone.manager_channel == 1 {
        (2..2 + zone.members).collect()
    } else {
        (16 - zone.members..16).collect()
    }
}
