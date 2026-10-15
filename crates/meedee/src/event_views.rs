//! Optional source-scoped semantic observations. Original events remain unchanged.
use crate::{
    controls,
    events::{Clock, Event, Payload, Timestamp},
    file_io::invalid,
    live::{Diagnostic, DiagnosticKind},
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
    pub stream: String,
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
    clock: Option<Clock>,
    index: usize,
    markers: BTreeMap<usize, Marker>,
}
impl Context {
    fn event(&mut self, event: &Event) -> io::Result<(usize, bool)> {
        event.validate()?;
        if self.stream.as_ref().is_some_and(|s| s != &event.stream) {
            return Err(invalid(
                0,
                "one event view belongs to one source; reset before rebinding",
            ));
        }
        self.stream = Some(event.stream.clone());
        let index = self.index;
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| invalid(0, "view event index exhausted"))?;
        let (first, last) = match &event.span {
            Some(span) => (span.first_timestamp.clone(), span.last_timestamp.clone()),
            None => (event.timestamp.clone(), event.timestamp.clone()),
        };
        let clock = event
            .timestamp
            .as_ref()
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
        if self.stream.as_ref().is_some_and(|s| s != &d.stream) {
            return Err(invalid(0, "boundary belongs to another source"));
        }
        self.stream = Some(d.stream.clone());
        self.clock = None;
        self.markers.clear();
        Ok(())
    }
    fn reset(&mut self) {
        self.stream = None;
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
            stream: self.stream.clone().unwrap_or_default(),
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
    fn clock_change(&self, event: &Event, index: usize, group: Option<u8>) -> Observation {
        self.observation(
            Value::Boundary(Diagnostic {
                stream: event.stream.clone(),
                packet: event.span.as_ref().map_or(index as u64, |s| s.last_packet),
                kind: DiagnosticKind::ClockChange,
                lost: 0,
                reason: "observed clock generation changed; view state reset".into(),
            }),
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
fn midi1(event: &Event, group: Option<u8>) -> io::Result<Option<Cow<'_, AnyMessage>>> {
    match &event.message {
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
    matches!(message, AnyMessage::SystemReset(_))
        || matches!(message,AnyMessage::SysEx(v)if crate::gm::decode(v).ok().flatten().is_some())
}
pub trait View {
    fn feed(&mut self, event: &Event, now_ns: Option<u64>) -> io::Result<Vec<Observation>>;
    fn boundary(&mut self, d: &Diagnostic) -> io::Result<Vec<Observation>>;
    fn reset(&mut self);
}
pub struct Controls {
    context: Context,
    state: controls::Interpreter,
    group: Option<u8>,
}
impl Controls {
    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: controls::Interpreter::new(),
            group: group(selected)?,
        })
    }
}
impl View for Controls {
    fn feed(&mut self, event: &Event, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.clock_change(event, index, self.group));
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
        let out = self
            .state
            .reset()
            .into_iter()
            .map(|v| {
                self.context
                    .observation(Value::Control(v), self.group, None, None)
            })
            .collect();
        Ok(out)
    }
    fn reset(&mut self) {
        self.state.reset();
        self.context.reset();
    }
}
pub struct Velocity {
    context: Context,
    state: velocity::VelocityInterpreter,
    group: Option<u8>,
}
impl Velocity {
    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: velocity::VelocityInterpreter::new(),
            group: group(selected)?,
        })
    }
}
impl View for Velocity {
    fn feed(&mut self, event: &Event, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.clock_change(event, index, self.group));
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
    state: mpe::Tracker,
    group: Option<u8>,
}
impl Mpe {
    pub fn new(zone: mpe::ZoneConfiguration, selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: mpe::Tracker::new(zone),
            group: group(selected)?,
        })
    }
    pub fn snapshot(&self, channel: i64) -> Result<mpe::ChannelState, crate::messages::ValueError> {
        self.state.snapshot(channel)
    }
}
impl View for Mpe {
    fn feed(&mut self, event: &Event, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.clock_change(event, index, self.group));
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
    state: ump::Assembler,
}
impl Ump {
    pub fn new(message_limit: usize, total_limit: usize, max_streams: usize) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: ump::Assembler::new(message_limit, total_limit, max_streams)
                .map_err(|e| invalid(0, e))?,
        })
    }
}
impl View for Ump {
    fn feed(&mut self, event: &Event, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let Payload::Ump(message) = &event.message else {
            return Err(invalid(0, "UMP events required"));
        };
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.clock_change(event, index, None));
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
    pub fn new(selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            group: group(selected)?,
        })
    }
    pub fn assembled(&mut self, observation: &Observation) -> io::Result<Vec<Observation>> {
        let Value::Ump(ump::AssemblyItem::Data(data)) = &observation.value else {
            return Err(invalid(0, "assembled SysEx7 observation required"));
        };
        if data.kind != ump::DataKind::SysEx7 || data.group != self.group {
            return Err(invalid(0, "select the assembled SysEx7 group explicitly"));
        }
        let sysex = SysEx::new(data.data.clone()).map_err(|e| invalid(0, e))?;
        let Some(value) = tuning::decode(&sysex).map_err(|e| invalid(0, e))? else {
            return Ok(vec![]);
        };
        if self
            .context
            .stream
            .as_ref()
            .is_some_and(|s| s != &observation.stream)
        {
            return Err(invalid(0, "another source requires reset"));
        }
        self.context.stream = Some(observation.stream.clone());
        let mut out = observation.clone();
        out.value = Value::Tuning(Box::new(value));
        Ok(vec![out])
    }
}
impl View for Tuning {
    fn feed(&mut self, event: &Event, _: Option<u64>) -> io::Result<Vec<Observation>> {
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            out.push(self.context.clock_change(event, index, self.group));
        }
        if let Some(AnyMessage::SysEx(s)) = message.as_deref() {
            match tuning::decode(s) {
                Ok(Some(v)) => out.push(self.context.observation(
                    Value::Tuning(Box::new(v)),
                    self.group,
                    Some(index),
                    Some(index),
                )),
                Ok(None) => {}
                Err(e) => out.push(self.context.observation(
                    Value::Boundary(Diagnostic {
                        stream: event.stream.clone(),
                        packet: event.span.as_ref().map_or(index as u64, |s| s.last_packet),
                        kind: DiagnosticKind::Malformed,
                        lost: 0,
                        reason: e.to_string(),
                    }),
                    self.group,
                    Some(index),
                    Some(index),
                )),
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
    state: timecode::Assembler,
    group: Option<u8>,
}
impl Timecode {
    pub fn new(timeout: u64, selected: Option<u8>) -> io::Result<Self> {
        Ok(Self {
            context: Context::default(),
            state: timecode::Assembler::new(timeout).map_err(|e| invalid(0, e))?,
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
    fn feed(&mut self, event: &Event, now: Option<u64>) -> io::Result<Vec<Observation>> {
        let now = now.ok_or_else(|| invalid(0, "MTC view needs explicit monotonic now_ns"))?;
        let message = midi1(event, self.group)?;
        let (index, changed) = self.context.event(event)?;
        let mut out = vec![];
        if changed {
            self.state.reset();
            out.push(self.context.clock_change(event, index, self.group));
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
                self.state.reset();
                (None, false)
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
