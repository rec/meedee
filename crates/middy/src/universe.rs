//! Sparse observed state for one ordered source. Queries never advance decoding.
use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use crate::controls::{self, ParameterFamily, ParameterState};
use crate::events::{Clock, Observation, ObservationValue, Payload, StreamDiagnostic};
use crate::file_io::invalid;
use crate::messages::{AnyMessage, Message};
use crate::{mpe, timecode, tuning};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Retention {
    #[default]
    Full,
    Minimum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Controls,
    Performance,
    Notes,
    System,
}

#[derive(Clone, Debug, Default)]
pub struct CacheOptions {
    pub default: Retention,
    pub universe: Option<Retention>,
    pub categories: BTreeMap<Category, Retention>,
}
impl CacheOptions {
    pub fn effective(&self, category: Category) -> Retention {
        self.categories
            .get(&category)
            .copied()
            .or(self.universe)
            .unwrap_or(self.default)
    }
}

/// Protocol and group are part of every address. Channels use 1..=16.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub group: Option<u8>,
    pub version: u8,
}
impl Scope {
    pub fn new(group: Option<u8>, version: u8) -> io::Result<Self> {
        if group.is_some_and(|v| !(1..=16).contains(&v))
            || !matches!(version, 1 | 2)
            || (version == 2 && group.is_none())
        {
            return Err(invalid(0, "invalid MIDI protocol/group scope"));
        }
        Ok(Self { group, version })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerState {
    pub scope: Scope,
    pub channel: u8,
    pub controller: u8,
    pub coarse: Option<u8>,
    pub fine: Option<u8>,
    pub native: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedParameter {
    pub scope: Scope,
    pub channel: u8,
    pub family: ParameterFamily,
    pub parameter: u16,
    pub components: Option<ParameterState>,
    pub native: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionState {
    pub scope: Scope,
    pub channel: u8,
    pub active: Option<ParameterFamily>,
    pub rpn: ParameterState,
    pub nrpn: ParameterState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramState {
    pub scope: Scope,
    pub channel: u8,
    pub program: Option<u8>,
    pub pending_msb: Option<u8>,
    pub pending_lsb: Option<u8>,
    pub bank_msb: Option<u8>,
    pub bank_lsb: Option<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerformanceState {
    pub scope: Scope,
    pub channel: u8,
    pub pitch_bend: Option<i32>,
    pub pressure: Option<u32>,
}
/// Last observed key event, independent of sounding voices or sustain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteState {
    pub scope: Scope,
    pub channel: u8,
    pub note: u8,
    pub held: Option<bool>,
    pub velocity: Option<u16>,
    pub velocity_resolution: Option<u8>,
    pub pressure: Option<u32>,
    pub pitch_bend: Option<i32>,
    pub registered: BTreeMap<u8, u32>,
    pub assignable: BTreeMap<u8, u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemState {
    pub scope: Scope,
    pub running: Option<bool>,
    pub song: Option<u8>,
    pub position: Option<u16>,
    pub timecode: Option<timecode::TimecodeObservation>,
    pub master_volume: BTreeMap<u8, u16>,
    pub master_fine_tuning: BTreeMap<u8, i16>,
    pub master_coarse_tuning: BTreeMap<u8, i8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuningState {
    pub group: Option<u8>,
    pub device: u8,
    pub bank: Option<u8>,
    pub program: u8,
    pub notes: BTreeMap<u8, tuning::TuningValue>,
    pub offsets: Option<tuning::ScaleOffsets>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelTuningState {
    pub group: Option<u8>,
    pub device: u8,
    pub channel: u8,
    pub offsets: tuning::ScaleOffsets,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub controllers: Vec<ControllerState>,
    pub parameters: Vec<CachedParameter>,
    pub selections: Vec<SelectionState>,
    pub programs: Vec<ProgramState>,
    pub performance: Vec<PerformanceState>,
    pub notes: Vec<NoteState>,
    pub system: Vec<SystemState>,
    pub tuning: Vec<TuningState>,
    pub channel_tuning: Vec<ChannelTuningState>,
    pub mpe: Vec<(Option<u8>, mpe::ChannelState)>,
}

struct Group {
    controls: controls::ControlInterpreter,
    velocity: controls::VelocityInterpreter,
    timecode: timecode::Assembler,
    tuning: tuning::Decoder,
    mpe: Option<Box<mpe::Tracker>>,
}
impl Group {
    fn new(group: Option<u8>, timeout: u64) -> io::Result<Self> {
        Ok(Self {
            controls: controls::ControlInterpreter::new(group)?,
            velocity: controls::VelocityInterpreter::new(group)?,
            timecode: timecode::Assembler::new(timeout, group)?,
            tuning: tuning::Decoder::new(group)?,
            mpe: None,
        })
    }
}

pub struct Universe {
    options: CacheOptions,
    groups: BTreeMap<Option<u8>, Box<Group>>,
    snapshot: Snapshot,
    stream: Option<String>,
    bound: bool,
    clock: Option<Clock>,
    start: Instant,
    now: Option<u64>,
    timeout: u64,
    next_index: usize,
    assembly: Option<crate::ump::Assembler>,
}
impl Universe {
    pub fn new(options: CacheOptions) -> Self {
        Self {
            options,
            groups: BTreeMap::new(),
            snapshot: Snapshot::default(),
            stream: None,
            bound: false,
            clock: None,
            start: Instant::now(),
            now: None,
            timeout: 1_000_000_000,
            next_index: 0,
            assembly: None,
        }
    }
    pub fn effective(&self, category: Category) -> Retention {
        self.options.effective(category)
    }
    pub fn set_timecode_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        let nanos =
            u64::try_from(timeout.as_nanos()).map_err(|_| invalid(0, "timeout too large"))?;
        if nanos == 0 || self.bound {
            return Err(invalid(0, "set a positive timeout before feeding events"));
        }
        self.timeout = nanos;
        for (&group, state) in &mut self.groups {
            state.timecode = timecode::Assembler::new(nanos, group)?;
        }
        Ok(())
    }
    pub fn configure_mpe(&mut self, zone: mpe::ZoneConfiguration) -> io::Result<()> {
        let group = zone.group();
        let state = self.group(group)?;
        state.mpe = Some(Box::new(mpe::Tracker::new(zone, group)?));
        self.snapshot.mpe.retain(|(g, _)| *g != group);
        Ok(())
    }
    fn group(&mut self, group: Option<u8>) -> io::Result<&mut Group> {
        if !self.groups.contains_key(&group) {
            self.groups
                .insert(group, Box::new(Group::new(group, self.timeout)?));
        }
        Ok(self.groups.get_mut(&group).expect("inserted group"))
    }
    pub fn reset(&mut self) {
        self.clear();
        self.stream = None;
        self.bound = false;
        self.clock = None;
        self.now = None;
        self.start = Instant::now();
    }
    fn clear(&mut self) {
        if let Some(assembly) = &mut self.assembly {
            assembly.reset();
        }
        self.snapshot = Snapshot::default();
        for group in self.groups.values_mut() {
            group.controls.reset();
            group.velocity.reset();
            group.timecode.reset();
            group.tuning.reset();
            if let Some(mpe) = &mut group.mpe {
                mpe.reset();
            }
        }
    }
    pub fn feed_diagnostic(
        &mut self,
        diagnostic: &StreamDiagnostic,
    ) -> io::Result<Vec<Observation>> {
        if self.bound && self.stream.as_deref() != Some(diagnostic.stream.as_str()) {
            return Err(invalid(0, "boundary belongs to another source"));
        }
        if let Some(assembly) = &mut self.assembly {
            assembly.feed_diagnostic(diagnostic)?;
        }
        for group in self.groups.values_mut() {
            group.controls.feed_diagnostic(diagnostic)?;
            group.velocity.feed_diagnostic(diagnostic)?;
            group.timecode.feed_diagnostic(diagnostic)?;
            group.tuning.feed_diagnostic(diagnostic)?;
            if let Some(mpe) = &mut group.mpe {
                mpe.feed_diagnostic(diagnostic)?;
            }
        }
        self.snapshot = Snapshot::default();
        self.bound = true;
        self.stream = Some(diagnostic.stream.clone());
        self.clock = None;
        Ok(vec![Observation {
            stream: Some(diagnostic.stream.clone()),
            group: None,
            first_index: None,
            last_index: None,
            first_timestamp: None,
            last_timestamp: None,
            value: ObservationValue::Boundary(diagnostic.clone()),
        }])
    }
    pub fn feed(&mut self, event: &Payload, now_ns: Option<u64>) -> io::Result<Vec<Observation>> {
        let index = self.next_index;
        let following = index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "universe event index exhausted"))?;
        event.validate().map_err(|e| invalid(0, e))?;
        if matches!(event, Payload::Command(_)) {
            return Err(invalid(0, "received primitive event required"));
        }
        if matches!(
            event,
            Payload::Message(
                AnyMessage::NoteOn(_)
                    | AnyMessage::NoteOff(_)
                    | AnyMessage::ControlChange(_)
                    | AnyMessage::ProgramChange(_)
                    | AnyMessage::PitchBend(_)
                    | AnyMessage::ChannelPressure(_)
                    | AnyMessage::PolyPressure(_)
            )
        ) && matches!(event,Payload::Message(v) if v.channel().is_none())
        {
            return Err(invalid(0, "received channel message requires a channel"));
        }
        if self.bound && self.stream.as_deref() != event.stream() {
            return Err(invalid(
                0,
                "source identity changed; reset before rebinding",
            ));
        }
        let now = now_ns
            .unwrap_or_else(|| self.start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        if self.now.is_some_and(|old| now < old) {
            return Err(invalid(0, "now_ns must be monotonic"));
        }
        // Validate recognized payloads before mutating any decoder.
        if let Payload::Message(AnyMessage::SysEx(s)) = event {
            tuning::decode(s).map_err(|e| invalid(0, e))?;
            timecode::decode(s).map_err(|e| invalid(0, e))?;
            crate::universal::decode(s).map_err(|e| invalid(0, e))?;
        }
        let clock = event
            .timestamp()
            .or_else(|| {
                event
                    .span()
                    .and_then(|s| s.first_timestamp.as_ref().or(s.last_timestamp.as_ref()))
            })
            .map(|v| v.clock.clone());
        if clock
            .as_ref()
            .is_some_and(|c| self.clock.as_ref().is_some_and(|old| old != c))
        {
            self.snapshot = Snapshot::default();
            if let Some(assembly) = &mut self.assembly {
                assembly.reset();
            }
            for group in self.groups.values_mut() {
                group.controls.invalidate();
                group.velocity.invalidate();
                group.timecode.invalidate();
                if let Some(mpe) = &mut group.mpe {
                    mpe.invalidate();
                }
            }
        }
        if clock.is_some() {
            self.clock = clock;
        }
        self.bound = true;
        self.stream = event.stream().map(str::to_owned);
        self.now = Some(now);
        self.next_index = following;
        let group = match event {
            Payload::Message(m) => m.group(),
            Payload::Ump(p) => p.group(),
            _ => unreachable!(),
        };
        if matches!(event, Payload::Ump(_)) && group.is_none() {
            return Ok(Vec::new());
        }
        // Every interpreter uses the universe's original primitive-event indexes.
        self.group(group)?;
        let mut out = Vec::new();
        for (&selected, state) in &mut self.groups {
            if selected != group {
                continue;
            }
            state.controls.set_index(index);
            state.velocity.set_index(index);
            state.timecode.set_index(index);
            state.tuning.set_index(index);
            out.extend(state.controls.feed(event)?);
            out.extend(state.velocity.feed(event)?);
            out.extend(state.timecode.feed(event, now)?);
            out.extend(state.tuning.feed(event)?);
            if let Some(mpe) = &mut state.mpe {
                mpe.set_index(index);
                out.extend(mpe.feed(event)?);
            }
        }
        if let Payload::Message(message) = event {
            self.cache_message(message);
        }
        if let Payload::Ump(crate::ump::PacketValue::SysEx7(packet)) = event {
            if packet.format() == 0 {
                let message = crate::messages::SysEx::new(packet.data().to_vec())
                    .and_then(|v| v.with_group(i64::from(packet.group())))
                    .map_err(|e| invalid(0, e))?;
                self.cache_message(&AnyMessage::SysEx(message));
            }
        }
        if matches!(event, Payload::Ump(crate::ump::PacketValue::SysEx7(_)))
            || (self.assembly.is_some() && group.is_some())
        {
            if self.assembly.is_none() {
                self.assembly = Some(crate::ump::Assembler::new(1_048_576, 8_388_608, 64)?);
            }
            self.assembly
                .as_mut()
                .expect("initialized assembler")
                .set_index(index);
            let assembled = self
                .assembly
                .as_mut()
                .expect("initialized assembler")
                .feed(event)?;
            for observation in assembled {
                if matches!(event,Payload::Ump(crate::ump::PacketValue::SysEx7(v)) if v.format()==0)
                    && matches!(
                        observation.value,
                        ObservationValue::Ump(crate::ump::AssemblyItem::Data(_))
                    )
                {
                    continue;
                }
                if let ObservationValue::Ump(crate::ump::AssemblyItem::Data(data)) =
                    &observation.value
                {
                    if data.kind == crate::ump::DataKind::SysEx7 {
                        let sysex = crate::messages::SysEx::new(data.data.clone())
                            .map_err(|e| invalid(0, e))?;
                        if self.effective(Category::System) == Retention::Full {
                            self.cache_universal(Scope { group, version: 1 }, &sysex);
                        }
                        if let Some(decoded) = tuning::decode(&sysex).map_err(|e| invalid(0, e))? {
                            let mut value = observation.clone();
                            value.value = ObservationValue::Tuning(Box::new(decoded));
                            out.push(value);
                        }
                        if let Some(value) =
                            self.group(group)?
                                .timecode
                                .assembled(&observation, &sysex, now)?
                        {
                            out.push(value);
                        }
                    }
                }
                out.push(observation);
            }
        }
        for observation in &out {
            self.cache_observation(observation);
        }
        Ok(out)
    }
    pub fn poll(&mut self, now_ns: Option<u64>) -> io::Result<Vec<Observation>> {
        let now = now_ns
            .unwrap_or_else(|| self.start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        if self.now.is_some_and(|old| now < old) {
            return Err(invalid(0, "now_ns must be monotonic"));
        }
        self.now = Some(now);
        let mut out = Vec::new();
        for state in self.groups.values_mut() {
            out.extend(state.timecode.poll(now)?);
        }
        for observation in &out {
            self.cache_observation(observation);
        }
        Ok(out)
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        for (&group, state) in &self.groups {
            let scope = Scope { group, version: 1 };
            for (channel, active, [rpn, nrpn]) in state.controls.selections() {
                snapshot.selections.push(SelectionState {
                    scope,
                    channel,
                    active,
                    rpn,
                    nrpn,
                });
            }
            for (channel, controller, coarse) in state.controls.coarse_values() {
                if !snapshot
                    .controllers
                    .iter()
                    .any(|s| s.scope == scope && s.channel == channel && s.controller == controller)
                {
                    snapshot.controllers.push(ControllerState {
                        scope,
                        channel,
                        controller,
                        coarse: Some(coarse),
                        fine: None,
                        native: None,
                    });
                }
            }
            for (channel, family, parameter, components) in state.controls.parameter_snapshot() {
                snapshot.parameters.push(CachedParameter {
                    scope: Scope { group, version: 1 },
                    channel,
                    family,
                    parameter,
                    components: Some(components),
                    native: None,
                });
            }
        }
        snapshot
    }
    pub fn parameter_state(
        &self,
        scope: Scope,
        channel: u8,
        family: ParameterFamily,
        parameter: u16,
    ) -> io::Result<Option<CachedParameter>> {
        validate(scope, channel)?;
        if parameter > 16383 {
            return Err(invalid(0, "parameter must be 0..16383"));
        }
        if scope.version == 1 {
            return self
                .groups
                .get(&scope.group)
                .map(|g| g.controls.parameter_state(channel, family, parameter))
                .transpose()
                .map(|v| {
                    v.flatten().map(|components| CachedParameter {
                        scope,
                        channel,
                        family,
                        parameter,
                        components: Some(components),
                        native: None,
                    })
                });
        }
        Ok(self
            .snapshot
            .parameters
            .iter()
            .find(|v| {
                v.scope == scope
                    && v.channel == channel
                    && v.family == family
                    && v.parameter == parameter
            })
            .cloned())
    }
    pub fn controller_state(
        &self,
        scope: Scope,
        channel: u8,
        controller: u8,
    ) -> io::Result<Option<ControllerState>> {
        validate(scope, channel)?;
        if controller > 127 {
            return Err(invalid(0, "controller must be 0..127"));
        }
        if let Some(state) = self
            .snapshot
            .controllers
            .iter()
            .find(|v| v.scope == scope && v.channel == channel && v.controller == controller)
        {
            return Ok(Some(state.clone()));
        }
        Ok(self
            .groups
            .get(&scope.group)
            .filter(|_| scope.version == 1)
            .and_then(|g| {
                g.controls
                    .coarse_values()
                    .into_iter()
                    .find(|(c, k, _)| *c == channel && *k == controller)
            })
            .map(|(_, _, coarse)| ControllerState {
                scope,
                channel,
                controller,
                coarse: Some(coarse),
                fine: None,
                native: None,
            }))
    }
    pub fn note_state(&self, scope: Scope, channel: u8, note: u8) -> io::Result<Option<NoteState>> {
        validate(scope, channel)?;
        if note > 127 {
            return Err(invalid(0, "note must be 0..127"));
        }
        Ok(self
            .snapshot
            .notes
            .iter()
            .find(|v| v.scope == scope && v.channel == channel && v.note == note)
            .cloned())
    }
    pub fn program_state(&self, scope: Scope, channel: u8) -> io::Result<Option<ProgramState>> {
        validate(scope, channel)?;
        Ok(self
            .snapshot
            .programs
            .iter()
            .find(|v| v.scope == scope && v.channel == channel)
            .cloned())
    }
    pub fn performance_state(
        &self,
        scope: Scope,
        channel: u8,
    ) -> io::Result<Option<PerformanceState>> {
        validate(scope, channel)?;
        Ok(self
            .snapshot
            .performance
            .iter()
            .find(|v| v.scope == scope && v.channel == channel)
            .cloned())
    }
    pub fn system_state(&self, scope: Scope) -> io::Result<Option<SystemState>> {
        Scope::new(scope.group, scope.version)?;
        Ok(self
            .snapshot
            .system
            .iter()
            .find(|v| v.scope == scope)
            .cloned())
    }
    pub fn selection_state(&self, scope: Scope, channel: u8) -> io::Result<Option<SelectionState>> {
        validate(scope, channel)?;
        Ok(self
            .groups
            .get(&scope.group)
            .filter(|_| scope.version == 1)
            .and_then(|g| {
                g.controls
                    .selections()
                    .into_iter()
                    .find(|(c, _, _)| *c == channel)
            })
            .map(|(_, active, [rpn, nrpn])| SelectionState {
                scope,
                channel,
                active,
                rpn,
                nrpn,
            }))
    }
    pub fn mpe_state(
        &self,
        group: Option<u8>,
        channel: u8,
    ) -> io::Result<Option<mpe::ChannelState>> {
        validate(Scope { group, version: 1 }, channel)?;
        self.groups
            .get(&group)
            .and_then(|g| g.mpe.as_ref())
            .map(|m| m.snapshot(i64::from(channel)).map_err(|e| invalid(0, e)))
            .transpose()
    }
    pub fn channel_tuning_state(
        &self,
        group: Option<u8>,
        device: u8,
        channel: u8,
    ) -> io::Result<Option<ChannelTuningState>> {
        validate(Scope { group, version: 1 }, channel)?;
        if device > 127 {
            return Err(invalid(0, "device must be 0..127"));
        }
        Ok(self
            .snapshot
            .channel_tuning
            .iter()
            .find(|v| v.group == group && v.device == device && v.channel == channel)
            .cloned())
    }
    pub fn tuning_state(
        &self,
        group: Option<u8>,
        device: u8,
        bank: Option<u8>,
        program: u8,
    ) -> io::Result<Option<TuningState>> {
        Scope::new(group, 1)?;
        if device > 127 || bank.is_some_and(|v| v > 127) || program > 127 {
            return Err(invalid(0, "invalid tuning address"));
        }
        Ok(self
            .snapshot
            .tuning
            .iter()
            .find(|v| {
                v.group == group && v.device == device && v.bank == bank && v.program == program
            })
            .cloned())
    }
}
impl Default for Universe {
    fn default() -> Self {
        Self::new(CacheOptions::default())
    }
}
fn validate(scope: Scope, channel: u8) -> io::Result<()> {
    Scope::new(scope.group, scope.version)?;
    if !(1..=16).contains(&channel) {
        return Err(invalid(0, "channel must be 1..16"));
    }
    Ok(())
}

impl Universe {
    fn cache_message(&mut self, message: &AnyMessage) {
        let scope = Scope {
            group: message.group(),
            version: message.version(),
        };
        let channel = message.channel().unwrap_or(1);
        let reset = matches!(message, AnyMessage::SystemReset(_))
            || matches!(message, AnyMessage::SysEx(s) if matches!(crate::gm::decode(s,1024),Ok(Some(crate::gm::GmMessage::GmMode(_)))));
        if reset {
            self.snapshot
                .controllers
                .retain(|v| v.scope.group != scope.group);
            self.snapshot
                .parameters
                .retain(|v| v.scope.group != scope.group);
            self.snapshot
                .programs
                .retain(|v| v.scope.group != scope.group);
            self.snapshot
                .performance
                .retain(|v| v.scope.group != scope.group);
            self.snapshot.notes.retain(|v| v.scope.group != scope.group);
            self.snapshot
                .system
                .retain(|v| v.scope.group != scope.group);
            self.snapshot.tuning.retain(|v| v.group != scope.group);
            self.snapshot
                .channel_tuning
                .retain(|v| v.group != scope.group);
            self.snapshot.mpe.retain(|(g, _)| *g != scope.group);
            return;
        }
        if let AnyMessage::ControlChange(v) = message {
            if v.controller() == 121 {
                self.snapshot
                    .controllers
                    .retain(|v| v.scope != scope || v.channel != channel);
                self.snapshot
                    .parameters
                    .retain(|v| v.scope != scope || v.channel != channel);
                self.snapshot
                    .performance
                    .retain(|v| v.scope != scope || v.channel != channel);
            }
            if v.controller() == 123 {
                for note in &mut self.snapshot.notes {
                    if note.scope == scope && note.channel == channel {
                        note.held = Some(false);
                    }
                }
            }
            if scope.version == 1
                && self.effective(Category::Performance) == Retention::Full
                && matches!(v.controller(), 0 | 32)
            {
                let program = entry(
                    &mut self.snapshot.programs,
                    |p| p.scope == scope && p.channel == channel,
                    || ProgramState {
                        scope,
                        channel,
                        program: None,
                        pending_msb: None,
                        pending_lsb: None,
                        bank_msb: None,
                        bank_lsb: None,
                    },
                );
                if v.controller() == 0 {
                    program.pending_msb = Some(v.value() as u8);
                } else {
                    program.pending_lsb = Some(v.value() as u8);
                }
            }
            if self.effective(Category::Controls) == Retention::Full && v.controller() != 121 {
                let controller = v.controller();
                let state = entry(
                    &mut self.snapshot.controllers,
                    |s| s.scope == scope && s.channel == channel && s.controller == controller,
                    || ControllerState {
                        scope,
                        channel,
                        controller,
                        coarse: None,
                        fine: None,
                        native: None,
                    },
                );
                if scope.version == 2 {
                    state.native = Some(v.value());
                } else {
                    state.coarse = Some(v.value() as u8);
                }
                if scope.version == 1 && (32..64).contains(&controller) {
                    let base = controller - 32;
                    let paired = entry(
                        &mut self.snapshot.controllers,
                        |s| s.scope == scope && s.channel == channel && s.controller == base,
                        || ControllerState {
                            scope,
                            channel,
                            controller: base,
                            coarse: None,
                            fine: None,
                            native: None,
                        },
                    );
                    paired.fine = Some(v.value() as u8);
                } else if scope.version == 1 && controller < 32 {
                    state.fine = None;
                }
            }
        }
        if self.effective(Category::Performance) == Retention::Full {
            match message {
                AnyMessage::ProgramChange(v) => {
                    let state = entry(
                        &mut self.snapshot.programs,
                        |s| s.scope == scope && s.channel == channel,
                        || ProgramState {
                            scope,
                            channel,
                            program: None,
                            pending_msb: None,
                            pending_lsb: None,
                            bank_msb: None,
                            bank_lsb: None,
                        },
                    );
                    state.program = Some(v.program());
                    if scope.version == 2 && v.bank_msb().is_some() {
                        state.bank_msb = v.bank_msb();
                        state.bank_lsb = v.bank_lsb();
                        state.pending_msb = v.bank_msb();
                        state.pending_lsb = v.bank_lsb();
                    } else {
                        state.bank_msb = state.pending_msb;
                        state.bank_lsb = state.pending_lsb;
                    }
                }
                AnyMessage::PitchBend(_) | AnyMessage::ChannelPressure(_) => {
                    let state = entry(
                        &mut self.snapshot.performance,
                        |s| s.scope == scope && s.channel == channel,
                        || PerformanceState {
                            scope,
                            channel,
                            pitch_bend: None,
                            pressure: None,
                        },
                    );
                    match message {
                        AnyMessage::PitchBend(v) => state.pitch_bend = Some(v.value()),
                        AnyMessage::ChannelPressure(v) => state.pressure = Some(v.pressure()),
                        _ => unreachable!(),
                    }
                }
                _ => {}
            }
        }
        if self.effective(Category::Controls) == Retention::Full {
            let parameter = match message {
                AnyMessage::ParameterController(v) => Some((
                    if v.family() == crate::messages::ControllerFamily::Registered {
                        ParameterFamily::Rpn
                    } else {
                        ParameterFamily::Nrpn
                    },
                    u16::from(v.bank()) * 128 + u16::from(v.index()),
                    Some(v.value()),
                )),
                AnyMessage::RelativeParameterController(v) => Some((
                    if v.family() == crate::messages::ControllerFamily::Registered {
                        ParameterFamily::Rpn
                    } else {
                        ParameterFamily::Nrpn
                    },
                    u16::from(v.bank()) * 128 + u16::from(v.index()),
                    None,
                )),
                _ => None,
            };
            if let Some((family, parameter, native)) = parameter {
                self.snapshot.parameters.retain(|v| {
                    !(v.scope == scope
                        && v.channel == channel
                        && v.family == family
                        && v.parameter == parameter)
                });
                if native.is_some() {
                    self.snapshot.parameters.push(CachedParameter {
                        scope,
                        channel,
                        family,
                        parameter,
                        components: None,
                        native,
                    });
                }
            }
        }
        if self.effective(Category::Notes) == Retention::Full {
            let note = match message {
                AnyMessage::NoteOn(v) => Some(v.note()),
                AnyMessage::NoteOff(v) => Some(v.note()),
                AnyMessage::PolyPressure(v) => Some(v.note()),
                AnyMessage::PerNotePitchBend(v) => Some(v.note()),
                AnyMessage::PerNoteController(v) => Some(v.note()),
                AnyMessage::PerNoteManagement(v) => Some(v.note()),
                _ => None,
            };
            if let Some(note) = note {
                let state = entry(
                    &mut self.snapshot.notes,
                    |s| s.scope == scope && s.channel == channel && s.note == note,
                    || NoteState {
                        scope,
                        channel,
                        note,
                        held: None,
                        velocity: None,
                        velocity_resolution: None,
                        pressure: None,
                        pitch_bend: None,
                        registered: BTreeMap::new(),
                        assignable: BTreeMap::new(),
                    },
                );
                match message {
                    AnyMessage::NoteOn(v) => {
                        state.held = Some(v.version() == 2 || v.velocity() != 0);
                        state.velocity = Some(v.velocity());
                        state.velocity_resolution = Some(if v.version() == 2 { 16 } else { 7 });
                    }
                    AnyMessage::NoteOff(v) => {
                        state.held = Some(false);
                        state.velocity = Some(v.velocity());
                        state.velocity_resolution = Some(if v.version() == 2 { 16 } else { 7 });
                    }
                    AnyMessage::PolyPressure(v) => state.pressure = Some(v.pressure()),
                    AnyMessage::PerNotePitchBend(v) => state.pitch_bend = Some(v.value()),
                    AnyMessage::PerNoteController(v) => match v.family() {
                        crate::messages::ControllerFamily::Registered => {
                            state.registered.insert(v.controller(), v.value());
                        }
                        crate::messages::ControllerFamily::Assignable => {
                            state.assignable.insert(v.controller(), v.value());
                        }
                    },
                    AnyMessage::PerNoteManagement(v) if v.flags() & 2 != 0 => {
                        state.registered.clear();
                        state.assignable.clear();
                        state.pitch_bend = None;
                        state.pressure = None;
                    }
                    _ => {}
                }
            }
        }
        if self.effective(Category::System) == Retention::Full {
            if let AnyMessage::SysEx(sysex) = message {
                self.cache_universal(scope, sysex);
            }
            match message {
                AnyMessage::Start(_)
                | AnyMessage::Continue(_)
                | AnyMessage::Stop(_)
                | AnyMessage::SongPosition(_)
                | AnyMessage::SongSelect(_) => {
                    let state = system_entry(&mut self.snapshot.system, scope);
                    match message {
                        AnyMessage::Start(_) => {
                            state.running = Some(true);
                            state.position = Some(0);
                        }
                        AnyMessage::Continue(_) => state.running = Some(true),
                        AnyMessage::Stop(_) => state.running = Some(false),
                        AnyMessage::SongPosition(v) => state.position = Some(v.position()),
                        AnyMessage::SongSelect(v) => state.song = Some(v.song()),
                        _ => unreachable!(),
                    }
                }
                _ => {}
            }
        }
    }
    fn cache_observation(&mut self, observation: &Observation) {
        if let ObservationValue::Velocity(v) = &observation.value {
            if let Some(state) = self.snapshot.notes.iter_mut().find(|s| {
                s.scope.group == observation.group
                    && s.scope.version == 1
                    && s.channel == v.channel
                    && s.note == v.note
            }) {
                state.velocity = Some(v.velocity);
                state.velocity_resolution = Some(v.resolution);
            }
        }
        if self.effective(Category::System) != Retention::Full {
            return;
        }
        let scope = Scope {
            group: observation.group,
            version: 1,
        };
        match &observation.value {
            ObservationValue::Timecode(result) => {
                if result.observation.is_some() {
                    system_entry(&mut self.snapshot.system, scope).timecode =
                        result.observation.clone();
                } else if let Some(state) =
                    self.snapshot.system.iter_mut().find(|v| v.scope == scope)
                {
                    state.timecode = None;
                }
            }
            ObservationValue::Mpe(state) => {
                let group = observation.group;
                self.snapshot
                    .mpe
                    .retain(|(g, s)| *g != group || s.channel != state.channel);
                self.snapshot.mpe.push((group, state.clone()));
            }
            ObservationValue::Tuning(decoded) => {
                self.cache_tuning(observation.group, &decoded.message)
            }
            _ => {}
        }
    }
    fn cache_tuning(&mut self, group: Option<u8>, message: &tuning::TuningMessage) {
        use tuning::TuningMessage::*;
        if let ScaleOctaveChange(v) = message {
            for &channel in v.channels() {
                let device = v.device_id();
                self.snapshot
                    .channel_tuning
                    .retain(|s| s.group != group || s.device != device || s.channel != channel);
                self.snapshot.channel_tuning.push(ChannelTuningState {
                    group,
                    device,
                    channel,
                    offsets: v.offsets().clone(),
                });
            }
            return;
        }
        let (device, bank, program) = match message {
            BulkDump(v) => (v.device_id(), v.bank(), v.program()),
            ScaleOctaveDump(v) => (v.device_id(), Some(v.bank()), v.program()),
            NoteChange(v) => (v.device_id(), v.bank(), v.program()),
            _ => return,
        };
        let state = entry(
            &mut self.snapshot.tuning,
            |s| s.group == group && s.device == device && s.bank == bank && s.program == program,
            || TuningState {
                group,
                device,
                bank,
                program,
                notes: BTreeMap::new(),
                offsets: None,
            },
        );
        match message {
            BulkDump(v) => {
                update_table(state, v.table());
            }
            ScaleOctaveDump(v) => {
                state.notes.clear();
                state.offsets = Some(v.offsets().clone());
            }
            NoteChange(v) => {
                for change in v.changes() {
                    if change.pitch() != tuning::TuningValue::NoChange {
                        state.notes.insert(change.note(), change.pitch());
                    }
                }
            }
            _ => {}
        }
    }
    fn cache_universal(&mut self, scope: Scope, sysex: &crate::messages::SysEx) {
        use crate::universal::UniversalMessage::*;
        match crate::universal::decode(sysex) {
            Ok(Some(MasterVolume(v))) => {
                system_entry(&mut self.snapshot.system, scope)
                    .master_volume
                    .insert(v.device_id(), v.value());
            }
            Ok(Some(MasterFineTuning(v))) => {
                system_entry(&mut self.snapshot.system, scope)
                    .master_fine_tuning
                    .insert(v.device_id(), v.value());
            }
            Ok(Some(MasterCoarseTuning(v))) => {
                system_entry(&mut self.snapshot.system, scope)
                    .master_coarse_tuning
                    .insert(v.device_id(), v.semitones());
            }
            _ => {}
        }
    }
}

fn update_table(state: &mut TuningState, table: &tuning::TuningTable) {
    for (note, &value) in table.values().iter().enumerate() {
        if value != tuning::TuningValue::NoChange {
            state.notes.insert(note as u8, value);
        }
    }
    if table
        .values()
        .iter()
        .all(|v| *v != tuning::TuningValue::NoChange)
    {
        state.offsets = None;
    }
}

fn entry<T>(
    values: &mut Vec<T>,
    matches: impl Fn(&T) -> bool,
    create: impl FnOnce() -> T,
) -> &mut T {
    let index = values.iter().position(matches).unwrap_or_else(|| {
        values.push(create());
        values.len() - 1
    });
    &mut values[index]
}
fn system_entry(values: &mut Vec<SystemState>, scope: Scope) -> &mut SystemState {
    entry(
        values,
        |s| s.scope == scope,
        || SystemState {
            scope,
            running: None,
            song: None,
            position: None,
            timecode: None,
            master_volume: BTreeMap::new(),
            master_fine_tuning: BTreeMap::new(),
            master_coarse_tuning: BTreeMap::new(),
        },
    )
}
