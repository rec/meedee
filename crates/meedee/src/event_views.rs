//! Optional source-scoped semantic observations. Original events remain unchanged.
use crate::messages::Message;
use crate::{
    controls,
    events::{Clock, Payload, Timestamp},
    file_io::invalid,
    gm::{self, GmMessage},
    live::Diagnostic,
    messages::{AnyMessage, SysEx},
    mpe, timecode, tuning, ump, velocity,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Control(controls::Observation),
    Velocity(velocity::VelocityObservation),
    Mpe(mpe::ChannelState),
    Timecode(timecode::AssemblyResult),
    Tuning(Box<tuning::Decoded>),
    Ump(ump::AssemblyItem),
    Boundary(Diagnostic),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Observation {
    pub stream: Option<String>,
    pub group: Option<u8>,
    pub first_index: Option<usize>,
    pub last_index: Option<usize>,
    pub first_timestamp: Option<Timestamp>,
    pub last_timestamp: Option<Timestamp>,
    pub value: Value,
}
struct Marker {
    first: Option<Timestamp>,
    last: Option<Timestamp>,
}
#[derive(Default)]
struct Context {
    stream: Option<String>,
    bound: bool,
    clock: Option<Clock>,
    index: usize,
    markers: BTreeMap<usize, Marker>,
}
impl Context {
    fn event(&mut self, event: &Payload, anonymous: bool) -> io::Result<(usize, bool)> {
        if !anonymous && event.stream().is_none() {
            return Err(invalid(0, "observation requires a source identity"));
        }
        if self.bound && self.stream.as_deref() != event.stream() {
            return Err(invalid(
                0,
                "source identity changed; reset before rebinding",
            ));
        }
        self.stream = event.stream().map(str::to_owned);
        self.bound = true;
        let index = self.index;
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "view event index exhausted"))?;
        let (first, last) = match &event.span() {
            Some(span) => (span.first_timestamp.clone(), span.last_timestamp.clone()),
            None => (event.timestamp().cloned(), event.timestamp().cloned()),
        };
        let clock = event
            .timestamp()
            .or(first.as_ref())
            .or(last.as_ref())
            .map(|t| t.clock.clone());
        let changed = clock
            .as_ref()
            .is_some_and(|c| self.clock.as_ref().is_some_and(|old| old != c));
        if changed {
            self.markers.clear();
        }
        if clock.is_some() {
            self.clock = clock;
        }
        self.markers.insert(index, Marker { first, last });
        Ok((index, changed))
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<()> {
        if self.bound && self.stream.as_deref() != Some(d.stream.as_str()) {
            return Err(invalid(0, "boundary belongs to another source"));
        }
        self.stream = Some(d.stream.clone());
        self.bound = true;
        self.clock = None;
        self.markers.clear();
        Ok(())
    }
    fn reset(&mut self) {
        self.stream = None;
        self.bound = false;
        self.clock = None;
        self.markers.clear();
    }
    fn observation(
        &self,
        value: Value,
        group: Option<u8>,
        first: Option<usize>,
        last: Option<usize>,
    ) -> Observation {
        Observation {
            stream: self.stream.clone(),
            group,
            first_index: first,
            last_index: last,
            first_timestamp: first
                .and_then(|i| self.markers.get(&i))
                .and_then(|m| m.first.clone()),
            last_timestamp: last
                .and_then(|i| self.markers.get(&i))
                .and_then(|m| m.last.clone()),
            value,
        }
    }
    fn prune(&mut self, retained: Vec<usize>) {
        let retained = retained.into_iter().collect::<BTreeSet<_>>();
        self.markers.retain(|i, _| retained.contains(i));
    }
    fn state_reset(&self, index: usize, group: Option<u8>) -> Observation {
        self.observation(
            Value::Control(controls::Observation::Diagnostic(controls::Diagnostic {
                kind: controls::DiagnosticKind::StateReset,
                channel: None,
                controller: None,
                first_index: index,
                last_index: index,
            })),
            group,
            None,
            Some(index),
        )
    }
}
fn group(value: Option<u8>) -> io::Result<Option<u8>> {
    if value.is_some_and(|n| !(1..=16).contains(&n)) {
        return Err(invalid(0, "group must be 1..16"));
    }
    Ok(value)
}
fn midi1(event: &Payload, group: Option<u8>) -> io::Result<Option<Cow<'_, AnyMessage>>> {
    match &event {
        Payload::Midi1(v) => {
            if group.is_none() {
                Ok(Some(Cow::Borrowed(v)))
            } else {
                Err(invalid(0, "a selected UMP group requires UMP events"))
            }
        }
        Payload::Ump(v) => {
            let selected = group
                .ok_or_else(|| invalid(0, "select a group for MIDI 1 interpretation inside UMP"))?;
            match v {
                ump::UmpMessage::Midi1(v) if v.group() == selected => {
                    Ok(Some(Cow::Borrowed(v.message())))
                }
                ump::UmpMessage::SysEx7(v) if v.group() == selected && v.format() == 0 => {
                    Ok(Some(Cow::Owned(AnyMessage::SysEx(
                        SysEx::new(v.data().to_vec()).expect("validated SysEx7"),
                    ))))
                }
                _ => Ok(None),
            }
        }
        Payload::Command(_) => Err(invalid(
            0,
            "received primitive events required, not semantic commands",
        )),
    }
}
fn resets(message: &AnyMessage) -> bool {
    match message {
        AnyMessage::SystemReset(_) => true,
        AnyMessage::SysEx(message) => matches!(
            gm::decode(message, 1024),
            Ok(Some(
                GmMessage::Gm1On(_) | GmMessage::Gm2On(_) | GmMessage::GmOff(_)
            ))
        ),
        _ => false,
    }
}
pub trait View {
    fn feed(&mut self, event: &Payload, now_ns: Option<u64>) -> io::Result<Vec<Observation>>;
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>>;
    fn reset(&mut self);
}
pub struct ControlInterpreter {
    context: Context,
    state: controls::ControlState,
    group: Option<u8>,
}
impl ControlInterpreter {
    /// Read cached components without selecting a parameter or changing source state.
    pub fn parameter_state(
        &self,
        channel: u8,
        family: controls::ParameterFamily,
        parameter: u16,
    ) -> io::Result<Option<controls::ParameterState>> {
        self.state.parameter_state(channel, family, parameter)
    }

    pub fn feed(&mut self, message: &Payload) -> io::Result<Vec<Observation>> {
        View::feed(self, message, None)
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: controls::ControlState::new(),
            group: group(selected)?,
        })
    }
}
impl View for ControlInterpreter {
    fn feed(&mut self, event: &Payload, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.observation(
                Value::Control(controls::Observation::Diagnostic(controls::Diagnostic {
                    kind: controls::DiagnosticKind::StateReset,
                    channel: None,
                    controller: None,
                    first_index: index,
                    last_index: index,
                })),
                self.group,
                None,
                Some(index),
            ));
        }
        let input = match message.as_deref() {
            Some(v) if resets(v) => controls::InputMessage::SystemReset,
            Some(v) => controls::InputMessage::from(v),
            None => controls::InputMessage::Other,
        };
        for value in self.state.feed(input) {
            let (first, last) = match &value {
                controls::Observation::Control(v) => (v.first_index, v.last_index),
                controls::Observation::Parameter(v) => (v.first_index, v.last_index),
                controls::Observation::Step(v) => (v.first_index, v.last_index),
                controls::Observation::Diagnostic(v) => (v.first_index, v.last_index),
            };
            out.push(self.context.observation(
                Value::Control(value),
                self.group,
                Some(first),
                Some(last),
            ));
        }
        self.context.prune(self.state.retained_indexes());
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        self.state.reset();
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            self.group,
            None,
            None,
        )])
    }

    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
pub struct VelocityInterpreter {
    context: Context,
    state: velocity::VelocityState,
    group: Option<u8>,
}
impl VelocityInterpreter {
    pub fn feed(&mut self, message: &Payload) -> io::Result<Vec<Observation>> {
        View::feed(self, message, None)
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: velocity::VelocityState::new(),
            group: group(selected)?,
        })
    }
}
impl View for VelocityInterpreter {
    fn feed(&mut self, event: &Payload, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.observation(
                Value::Control(controls::Observation::Diagnostic(controls::Diagnostic {
                    kind: controls::DiagnosticKind::StateReset,
                    channel: None,
                    controller: None,
                    first_index: index,
                    last_index: index,
                })),
                self.group,
                None,
                Some(index),
            ));
        }
        if let Some(message) = message.as_deref() {
            if resets(message) {
                self.state.reset();
            }
            if let Some((v, first, last)) = self.state.feed_spanned(message) {
                out.push(self.context.observation(
                    Value::Velocity(v),
                    self.group,
                    Some(first),
                    Some(last),
                ));
            }
        } else {
            self.state.skip();
        }
        self.context.prune(self.state.retained_indexes());
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        self.state.reset();
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            self.group,
            None,
            None,
        )])
    }
    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
pub struct Mpe {
    context: Context,
    state: mpe::TrackerState,
    group: Option<u8>,
}
impl Mpe {
    pub fn configure(&mut self, zone: mpe::ZoneConfiguration) {
        self.state.configure(zone);
        self.context.reset();
    }

    pub fn feed(&mut self, message: &Payload) -> io::Result<Vec<Observation>> {
        View::feed(self, message, None)
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(zone: mpe::ZoneConfiguration, selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: mpe::TrackerState::new(zone),
            group: group(selected)?,
        })
    }
    pub fn snapshot(&self, channel: i64) -> Result<mpe::ChannelState, crate::messages::ValueError> {
        self.state.snapshot(channel)
    }
}
impl View for Mpe {
    fn feed(&mut self, event: &Payload, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.state_reset(index, self.group));
        }
        if let Some(message) = message.as_deref() {
            if resets(message) {
                self.state.reset();
            }
            for value in self.state.feed(message).map_err(|e| invalid(0, e))? {
                out.push(self.context.observation(
                    Value::Mpe(value),
                    self.group,
                    Some(index),
                    Some(index),
                ));
            }
        }
        self.context.prune(vec![]);
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        self.state.reset();
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            self.group,
            None,
            None,
        )])
    }
    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
pub struct Ump {
    context: Context,
    state: ump::AssemblyState,
}
impl Ump {
    pub fn retained_bytes(&self) -> usize {
        self.state.retained_bytes()
    }
    pub fn active_streams(&self) -> usize {
        self.state.active_streams()
    }
    pub fn finish(&mut self) -> Vec<Observation> {
        let out = self
            .state
            .finish_spanned()
            .into_iter()
            .map(|(v, g, a, b)| self.context.observation(Value::Ump(v), g, Some(a), Some(b)))
            .collect();
        self.context.prune(vec![]);
        out
    }

    pub fn feed(&mut self, message: &Payload) -> io::Result<Vec<Observation>> {
        View::feed(self, message, None)
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(message_limit: usize, total_limit: usize, max_streams: usize) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: ump::AssemblyState::new(message_limit, total_limit, max_streams)
                .map_err(|e| invalid(0, e))?,
        })
    }
}
impl View for Ump {
    fn feed(&mut self, event: &Payload, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let Payload::Ump(message) = &event else {
            return Err(invalid(0, "UMP events required"));
        };
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.state_reset(index, None));
        }
        for (value, first, last) in self.state.feed_spanned(message) {
            let group = if let ump::AssemblyItem::Data(v) = &value {
                v.group
            } else {
                None
            };
            out.push(
                self.context
                    .observation(Value::Ump(value), group, Some(first), Some(last)),
            );
        }
        self.context.prune(self.state.retained_indexes());
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        self.state.reset();
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            None,
            None,
            None,
        )])
    }
    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
pub struct Tuning {
    context: Context,
    group: Option<u8>,
}
impl Tuning {
    pub fn feed(&mut self, message: &Payload) -> io::Result<Vec<Observation>> {
        View::feed(self, message, None)
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            group: group(selected)?,
        })
    }
    pub fn feed_assembled(&mut self, observation: &Observation) -> io::Result<Vec<Observation>> {
        let Value::Ump(ump::AssemblyItem::Data(data)) = &observation.value else {
            return Err(invalid(0, "assembled SysEx7 observation required"));
        };
        if data.kind != ump::DataKind::SysEx7 || data.group != self.group {
            return Err(invalid(0, "select the assembled SysEx7 group explicitly"));
        }
        let sysex = SysEx::new(data.data.clone()).map_err(|e| invalid(0, e))?;
        let value = tuning::decode(&sysex).map_err(|e| invalid(0, e))?;
        let event = Payload::Midi1(AnyMessage::SysEx(sysex))
            .with_metadata(crate::messages::Metadata {
                stream: observation.stream.clone(),
                timestamp: observation
                    .last_timestamp
                    .clone()
                    .or_else(|| observation.first_timestamp.clone()),
                span: None,
            })
            .map_err(|e| invalid(0, e))?;
        let (index, changed) = self.context.event(&event, true)?;
        let mut out = Vec::new();
        if changed {
            out.push(self.context.state_reset(index, self.group));
        }
        if let Some(value) = value {
            let mut decoded = observation.clone();
            decoded.value = Value::Tuning(Box::new(value));
            out.push(decoded);
        }
        self.context.prune(vec![]);
        Ok(out)
    }
}
impl View for Tuning {
    fn feed(&mut self, event: &Payload, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            out.push(self.context.state_reset(index, self.group));
        }
        if let Some(AnyMessage::SysEx(s)) = message.as_deref() {
            if let Some(v) = tuning::decode(s).map_err(|e| invalid(0, e))? {
                out.push(self.context.observation(
                    Value::Tuning(Box::new(v)),
                    self.group,
                    Some(index),
                    Some(index),
                ));
            }
        }

        self.context.prune(vec![]);
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            self.group,
            None,
            None,
        )])
    }
    fn reset(&mut self) {
        self.context.reset();
    }
}
pub struct Timecode {
    context: Context,
    state: timecode::TimecodeState,
    group: Option<u8>,
}
impl Timecode {
    pub fn locked(&self) -> bool {
        self.state.locked()
    }
    pub fn feed(&mut self, message: &Payload, now_ns: u64) -> io::Result<Vec<Observation>> {
        View::feed(self, message, Some(now_ns))
    }
    pub fn feed_diagnostic(&mut self, diagnostic: &Diagnostic) -> io::Result<Vec<Observation>> {
        View::boundary(self, diagnostic)
    }
    pub fn reset(&mut self) {
        View::reset(self);
    }

    pub fn new(timeout: u64, selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: timecode::TimecodeState::new(timeout).map_err(|e| invalid(0, e))?,
            group: group(selected)?,
        })
    }
    pub fn poll(&mut self, now: u64) -> io::Result<Vec<Observation>> {
        Ok(self
            .state
            .poll(now)
            .map_err(|e| invalid(0, e))?
            .map(|d| {
                self.context.observation(
                    Value::Timecode(timecode::AssemblyResult {
                        observation: None,
                        diagnostic: Some(d),
                    }),
                    self.group,
                    None,
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}
impl View for Timecode {
    fn feed(&mut self, event: &Payload, now: Option<u64>) -> io::Result<Vec<Observation>> {
        let now = now.ok_or_else(|| invalid(0, "MTC view needs explicit monotonic now_ns"))?;
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event, true)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.state_reset(index, self.group));
        }
        let (value, complete_single) = match message.as_deref() {
            Some(AnyMessage::QuarterFrame(v)) => (
                Some(self.state.feed(v, now).map_err(|e| invalid(0, e))?),
                false,
            ),
            Some(AnyMessage::SysEx(v)) => {
                if let Some(full) = timecode::decode(v).map_err(|e| invalid(0, e))? {
                    (
                        Some(self.state.relocate(&full, now).map_err(|e| invalid(0, e))?),
                        true,
                    )
                } else {
                    (None, false)
                }
            }
            Some(v) if resets(v) => {
                self.state.poll(now).map_err(|e| invalid(0, e))?;
                self.state.reset();
                (
                    Some(timecode::AssemblyResult {
                        observation: None,
                        diagnostic: Some(timecode::AssemblyDiagnostic::Reset),
                    }),
                    true,
                )
            }
            _ => (None, false),
        };
        if let Some(v) = value {
            if v.observation.is_some() || v.diagnostic.is_some() {
                out.push(self.context.observation(
                    Value::Timecode(v),
                    self.group,
                    complete_single.then_some(index),
                    Some(index),
                ));
            }
        }
        self.context.prune(vec![]);
        Ok(out)
    }
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>> {
        self.context.boundary(d)?;
        self.state.reset();
        Ok(vec![self.context.observation(
            Value::Boundary(d.clone()),
            self.group,
            None,
            None,
        )])
    }
    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
