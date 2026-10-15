//! Python views of the pure Rust profile engines.
use super::{hash, int, mpe, object, timecode, value_error};
use crate::messages::{self, Message};
use meedee::messages::PrimitiveMessage;
use meedee::{gm as gm_model, mpe as mpe_model, timecode as tc, velocity};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyTuple};

macro_rules! frozen {
    ($name:ident; $($field:ident),*; {$($methods:item)*}) => {
        #[pymethods]
        impl $name {
            $($methods)*
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool{other.extract::<PyRef<'_,Self>>().is_ok_and(|v|self.value==v.value)}
            fn __hash__(&self)->u64{hash(&self.value)}
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String>{let py=slf.py();super::render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])}
            #[classattr]
            fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>>{PyTuple::new(py,[$(stringify!($field)),*])}
        }
    };
}
#[pyclass(frozen,extends=Message,module="meedee.gm")]
struct ControllerDestination {
    value: gm_model::ControllerDestination,
    max_entries: usize,
}
frozen!(ControllerDestination;device_id,channel,source,controller,entries;{
    #[new]
    #[pyo3(signature=(*,device_id,channel,source,controller,entries,max_entries=1024))]
    fn new(
        device_id: &Bound<'_, PyAny>,
        channel: &Bound<'_, PyAny>,
        source: &Bound<'_, PyAny>,
        controller: &Bound<'_, PyAny>,
        entries: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] max_entries: usize,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(PyClassInitializer::from(Message).add_subclass(Self {
            value: gm_model::ControllerDestination::new(
                int(device_id)?,
                int(channel)?,
                int(source)?,
                if controller.is_none() {
                    None
                } else {
                    Some(int(controller)?)
                },
                pairs(entries, max_entries)?,
                max_entries,
            )
            .map_err(value_error)?,
            max_entries,
        }))
    }
    #[getter]
    fn device_id(&self) -> u8 {
        self.value.device_id()
    }
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel()
    }
    #[getter]
    fn source(&self) -> u8 {
        self.value.source()
    }
    #[getter]
    fn controller(&self) -> Option<u8> {
        self.value.controller()
    }
    #[getter]
    fn entries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.value.entries().iter().copied())
    }

#[getter] fn max_entries(&self)->usize {self.max_entries}
#[pyo3(signature=(**changes))]
fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>> {
let py=slf.py();super::replacement(slf.into_pyobject(py)?.as_any(),&["device_id","channel","source","controller","entries","max_entries"],changes)
}
});
#[pyclass(frozen,extends=Message,module="meedee.gm")]
struct KeyBasedControl {
    value: gm_model::KeyBasedControl,
    max_entries: usize,
}
frozen!(KeyBasedControl;device_id,channel,note,entries;{
    #[new]
    #[pyo3(signature=(*,device_id,channel,note,entries,max_entries=1024))]
    fn new(
        device_id: &Bound<'_, PyAny>,
        channel: &Bound<'_, PyAny>,
        note: &Bound<'_, PyAny>,
        entries: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] max_entries: usize,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(PyClassInitializer::from(Message).add_subclass(Self {
            value: gm_model::KeyBasedControl::new(
                int(device_id)?,
                int(channel)?,
                int(note)?,
                pairs(entries, max_entries)?,
                max_entries,
            )
            .map_err(value_error)?,
            max_entries,
        }))
    }
    #[getter]
    fn device_id(&self) -> u8 {
        self.value.device_id()
    }
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel()
    }
    #[getter]
    fn note(&self) -> u8 {
        self.value.note()
    }
    #[getter]
    fn entries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.value.entries().iter().copied())
    }

#[getter] fn max_entries(&self)->usize {self.max_entries}
#[pyo3(signature=(**changes))]
fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>> {
let py=slf.py();super::replacement(slf.into_pyobject(py)?.as_any(),&["device_id","channel","note","entries","max_entries"],changes)
}
});
#[pyclass(frozen,extends=Message,module="meedee.gm")]
struct GlobalParameterControl {
    value: gm_model::GlobalParameterControl,
    max_entries: usize,
}
frozen!(GlobalParameterControl;device_id,slot_path,parameter_width,value_width,entries;{
    #[new]
    #[pyo3(signature=(*,device_id,slot_path,parameter_width,value_width,entries,max_entries=1024))]
    fn new(
        device_id: &Bound<'_, PyAny>,
        slot_path: &Bound<'_, PyAny>,
        parameter_width: &Bound<'_, PyAny>,
        value_width: &Bound<'_, PyAny>,
        entries: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] max_entries: usize,
    ) -> PyResult<PyClassInitializer<Self>> {
        let mut values = Vec::new();
        for entry in sequence(entries, max_entries)? {
            let pair = sequence(&entry, 2)?;
            if pair.len() != 2 {
                return Err(PyValueError::new_err("expected parameter/value pair"));
            }
            let read = |v: &Bound<'_, PyAny>| -> PyResult<Vec<u8>> {
                sequence(v, 127)?
                    .iter()
                    .map(|v| {
                        let i = int(v)?;
                        u8::try_from(i)
                            .map_err(|_| PyValueError::new_err("expected seven-bit byte"))
                    })
                    .collect()
            };
            values.push((read(&pair[0])?, read(&pair[1])?));
        }
        let path = sequence(slot_path, 127)?
            .iter()
            .map(int)
            .collect::<PyResult<_>>()?;
        Ok(PyClassInitializer::from(Message).add_subclass(Self {
            value: gm_model::GlobalParameterControl::new(
                int(device_id)?,
                path,
                int(parameter_width)?,
                int(value_width)?,
                values,
                max_entries,
            )
            .map_err(value_error)?,
            max_entries,
        }))
    }
    #[getter]
    fn device_id(&self) -> u8 {
        self.value.device_id()
    }
    #[getter]
    fn slot_path<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.value.slot_path().iter().copied())
    }
    #[getter]
    fn parameter_width(&self) -> u8 {
        self.value.parameter_width()
    }
    #[getter]
    fn value_width(&self) -> u8 {
        self.value.value_width()
    }
    #[getter]
    fn entries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let values = self
            .value
            .entries()
            .iter()
            .map(|(p, v)| {
                Ok((
                    PyTuple::new(py, p.iter().copied())?,
                    PyTuple::new(py, v.iter().copied())?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, values)
    }

#[getter] fn max_entries(&self)->usize {self.max_entries}
#[pyo3(signature=(**changes))]
fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>> {
let py=slf.py();super::replacement(slf.into_pyobject(py)?.as_any(),&["device_id","slot_path","parameter_width","value_width","entries","max_entries"],changes)
}
});
pub(super) fn decode_gm(
    py: Python<'_>,
    message: &meedee::messages::SysEx,
    max_entries: usize,
) -> PyResult<Option<Py<PyAny>>> {
    gm_model::decode_extended(message, max_entries)
        .map_err(value_error)?
        .map(|v| match v {
            gm_model::ExtendedMessage::ControllerDestination(value) => {
                object(py, ControllerDestination { value, max_entries })
            }
            gm_model::ExtendedMessage::KeyBasedControl(value) => {
                object(py, KeyBasedControl { value, max_entries })
            }
            gm_model::ExtendedMessage::GlobalParameterControl(value) => {
                object(py, GlobalParameterControl { value, max_entries })
            }
        })
        .transpose()
}
#[pyfunction]
#[pyo3(signature=(*,device_id,parameter,value))]
fn reverb_parameter(
    py: Python<'_>,
    device_id: &Bound<'_, PyAny>,
    parameter: &Bound<'_, PyAny>,
    value: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    object(
        py,
        GlobalParameterControl {
            value: gm_model::reverb_parameter(int(device_id)?, int(parameter)?, int(value)?)
                .map_err(value_error)?,
            max_entries: 1024,
        },
    )
}
#[pyfunction]
#[pyo3(signature=(*,device_id,parameter,value))]
fn chorus_parameter(
    py: Python<'_>,
    device_id: &Bound<'_, PyAny>,
    parameter: &Bound<'_, PyAny>,
    value: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    object(
        py,
        GlobalParameterControl {
            value: gm_model::chorus_parameter(int(device_id)?, int(parameter)?, int(value)?)
                .map_err(value_error)?,
            max_entries: 1024,
        },
    )
}
pub(super) fn register_gm(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ControllerDestination>()?;
    module.add_class::<KeyBasedControl>()?;
    module.add_class::<GlobalParameterControl>()?;
    module.add_function(wrap_pyfunction!(reverb_parameter, module)?)?;
    module.add_function(wrap_pyfunction!(chorus_parameter, module)?)?;
    Ok(())
}
pub(super) fn encode_message(value: &Bound<'_, PyAny>) -> Option<Vec<u8>> {
    if let Ok(v) = value.extract::<PyRef<'_, ControllerDestination>>() {
        return Some(v.value.encode());
    }
    if let Ok(v) = value.extract::<PyRef<'_, KeyBasedControl>>() {
        return Some(v.value.encode());
    }
    if let Ok(v) = value.extract::<PyRef<'_, GlobalParameterControl>>() {
        return Some(v.value.encode());
    }
    None
}
pub(super) fn is_message(value: &Bound<'_, PyAny>) -> bool {
    value.is_instance_of::<ControllerDestination>()
        || value.is_instance_of::<KeyBasedControl>()
        || value.is_instance_of::<GlobalParameterControl>()
}

#[pyclass(frozen, module = "meedee.mpe")]
struct Voice {
    value: mpe_model::Voice,
}
frozen!(Voice;id,channel,note,velocity;{
    #[getter]
    fn id(&self) -> u64 {
        self.value.id()
    }
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel()
    }
    #[getter]
    fn note(&self) -> u8 {
        self.value.note()
    }
    #[getter]
    fn velocity(&self) -> u8 {
        self.value.velocity()
    }
    #[getter]
    fn note_on(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        messages::to_python(
            py,
            meedee::messages::AnyMessage::NoteOn(self.value.note_on()),
        )
    }
});
#[pyclass(module = "meedee.mpe")]
struct Allocator {
    value: mpe_model::Allocator,
}
#[pymethods]
impl Allocator {
    #[new]
    #[pyo3(signature=(*,zone))]
    fn new(zone: PyRef<'_, mpe::ZoneConfiguration>) -> Self {
        Self {
            value: mpe_model::Allocator::new(zone.value.clone()),
        }
    }
    #[pyo3(signature=(*,note,velocity))]
    fn allocate(
        &mut self,
        note: &Bound<'_, PyAny>,
        velocity: &Bound<'_, PyAny>,
    ) -> PyResult<Voice> {
        Ok(Voice {
            value: self
                .value
                .allocate(int(note)?, int(velocity)?)
                .map_err(value_error)?,
        })
    }
    #[pyo3(signature=(*,voice_id,velocity))]
    fn release(
        &mut self,
        py: Python<'_>,
        voice_id: &Bound<'_, PyAny>,
        velocity: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        messages::to_python(
            py,
            meedee::messages::AnyMessage::NoteOff(
                self.value
                    .release(unsigned(voice_id)?, int(velocity)?)
                    .map_err(value_error)?,
            ),
        )
    }
    #[pyo3(signature=(*,enabled))]
    fn set_sustain(&mut self, py: Python<'_>, enabled: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let enabled = boolean(enabled)?;
        messages::to_python(
            py,
            meedee::messages::AnyMessage::ControlChange(self.value.set_sustain(enabled)),
        )
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}
#[pyclass(frozen, module = "meedee.mpe")]
struct NoteState {
    value: mpe_model::NoteState,
}
frozen!(NoteState;note,held,sustained;{
    #[getter]
    fn note(&self) -> u8 {
        self.value.note
    }
    #[getter]
    fn held(&self) -> u16 {
        self.value.held
    }
    #[getter]
    fn sustained(&self) -> u16 {
        self.value.sustained
    }
});
#[pyclass(frozen, module = "meedee.mpe")]
struct ChannelState {
    value: mpe_model::ChannelState,
}
frozen!(ChannelState;channel,notes,pitch_bend,pressure,timbre,sustain,ambiguous;{
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel
    }
    #[getter]
    fn notes(&self) -> Vec<NoteState> {
        self.value
            .notes
            .iter()
            .cloned()
            .map(|value| NoteState { value })
            .collect()
    }
    #[getter]
    fn pitch_bend(&self) -> Option<i16> {
        self.value.pitch_bend
    }
    #[getter]
    fn pressure(&self) -> Option<u8> {
        self.value.pressure
    }
    #[getter]
    fn timbre(&self) -> Option<u8> {
        self.value.timbre
    }
    #[getter]
    fn sustain(&self) -> Option<bool> {
        self.value.sustain
    }
    #[getter]
    fn ambiguous(&self) -> bool {
        self.value.ambiguous
    }
});
#[pyclass(module = "meedee.mpe")]
struct Tracker {
    value: mpe_model::Tracker,
}
#[pymethods]
impl Tracker {
    #[new]
    #[pyo3(signature=(*,zone))]
    fn new(zone: PyRef<'_, mpe::ZoneConfiguration>) -> Self {
        Self {
            value: mpe_model::Tracker::new(zone.value.clone()),
        }
    }
    #[pyo3(signature=(*,zone))]
    fn configure(&mut self, zone: PyRef<'_, mpe::ZoneConfiguration>) {
        self.value.configure(zone.value.clone());
    }
    fn reset(&mut self) {
        self.value.reset();
    }
    fn feed(&mut self, message: &Bound<'_, PyAny>) -> PyResult<Vec<ChannelState>> {
        Ok(self
            .value
            .feed(&messages::extract_primitive(message)?)
            .map_err(value_error)?
            .into_iter()
            .map(|value| ChannelState { value })
            .collect())
    }
    #[pyo3(signature=(*,channel))]
    fn snapshot(&self, channel: &Bound<'_, PyAny>) -> PyResult<ChannelState> {
        Ok(ChannelState {
            value: self.value.snapshot(int(channel)?).map_err(value_error)?,
        })
    }
}
pub(super) fn register_mpe(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Voice>()?;
    module.add_class::<Allocator>()?;
    module.add_class::<NoteState>()?;
    module.add_class::<ChannelState>()?;
    module.add_class::<Tracker>()?;
    Ok(())
}

#[pyclass(frozen, module = "meedee.controls")]
struct VelocityObservation {
    value: velocity::VelocityObservation,
}
frozen!(VelocityObservation;channel,note,velocity,resolution,note_on,zero_velocity_note_on;{
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel
    }
    #[getter]
    fn note(&self) -> u8 {
        self.value.note
    }
    #[getter]
    fn velocity(&self) -> u16 {
        self.value.velocity
    }
    #[getter]
    fn resolution(&self) -> u8 {
        self.value.resolution
    }
    #[getter]
    fn note_on(&self) -> bool {
        self.value.note_on
    }
    #[getter]
    fn zero_velocity_note_on(&self) -> bool {
        self.value.zero_velocity_note_on
    }
});
#[pyclass(module = "meedee.controls")]
struct VelocityInterpreter {
    value: velocity::VelocityInterpreter,
}
#[pymethods]
impl VelocityInterpreter {
    #[new]
    fn new() -> Self {
        Self {
            value: velocity::VelocityInterpreter::new(),
        }
    }
    fn reset(&mut self) {
        self.value.reset();
    }
    fn feed(&mut self, message: &Bound<'_, PyAny>) -> PyResult<Option<VelocityObservation>> {
        Ok(self
            .value
            .feed(&messages::extract_primitive(message)?)
            .map(|value| VelocityObservation { value }))
    }
}
pub(super) fn register_controls(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<VelocityObservation>()?;
    module.add_class::<VelocityInterpreter>()?;
    Ok(())
}

#[pyclass(frozen, module = "meedee.timecode")]
struct Position {
    value: tc::Position,
}
frozen!(Position;rate,hours,minutes,seconds,frames;{
    #[new]
    #[pyo3(signature=(*,rate,hours,minutes,seconds,frames))]
    fn new(
        rate: &Bound<'_, PyAny>,
        hours: &Bound<'_, PyAny>,
        minutes: &Bound<'_, PyAny>,
        seconds: &Bound<'_, PyAny>,
        frames: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self {
            value: tc::Position::new(
                timecode::rate(rate)?,
                int(hours)?,
                int(minutes)?,
                int(seconds)?,
                int(frames)?,
            )
            .map_err(value_error)?,
        })
    }
    #[getter]
    fn rate<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        timecode::rate_object(py, self.value.rate())
    }
    #[getter]
    fn hours(&self) -> u8 {
        self.value.hours()
    }
    #[getter]
    fn minutes(&self) -> u8 {
        self.value.minutes()
    }
    #[getter]
    fn seconds(&self) -> u8 {
        self.value.seconds()
    }
    #[getter]
    fn frames(&self) -> u8 {
        self.value.frames()
    }
    #[pyo3(signature=(*,frames))]
    fn advance(&self, frames: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            value: self.value.advance(int(frames)?),
        })
    }
    #[pyo3(signature=(**changes))]
    fn replace<'py>(
        slf: PyRef<'py, Self>,
        changes: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        super::replacement(
            slf.into_pyobject(py)?.as_any(),
            &["rate", "hours", "minutes", "seconds", "frames"],
            changes,
        )
    }
});
#[pyclass(eq, frozen, from_py_object, module = "meedee.timecode")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum AssemblyDiagnostic {
    Discontinuity,
    Malformed,
    Timeout,
    Relocated,
    Reset,
}
impl From<tc::AssemblyDiagnostic> for AssemblyDiagnostic {
    fn from(v: tc::AssemblyDiagnostic) -> Self {
        match v {
            tc::AssemblyDiagnostic::Discontinuity => Self::Discontinuity,
            tc::AssemblyDiagnostic::Malformed => Self::Malformed,
            tc::AssemblyDiagnostic::Timeout => Self::Timeout,
            tc::AssemblyDiagnostic::Relocated => Self::Relocated,
            tc::AssemblyDiagnostic::Reset => Self::Reset,
        }
    }
}
#[pyclass(frozen, module = "meedee.timecode")]
struct TimecodeObservation {
    value: tc::TimecodeObservation,
}
frozen!(TimecodeObservation;encoded,compensated,direction,locked;{
    #[getter]
    fn encoded(&self) -> Position {
        Position {
            value: self.value.encoded.clone(),
        }
    }
    #[getter]
    fn compensated(&self) -> Position {
        Position {
            value: self.value.compensated.clone(),
        }
    }
    #[getter]
    fn direction(&self) -> i8 {
        self.value.direction
    }
    #[getter]
    fn locked(&self) -> bool {
        self.value.locked
    }
});
#[pyclass(frozen, module = "meedee.timecode")]
struct AssemblyResult {
    value: tc::AssemblyResult,
}
frozen!(AssemblyResult;observation,diagnostic;{
    #[getter]
    fn observation(&self) -> Option<TimecodeObservation> {
        self.value
            .observation
            .clone()
            .map(|value| TimecodeObservation { value })
    }
    #[getter]
    fn diagnostic(&self) -> Option<AssemblyDiagnostic> {
        self.value.diagnostic.map(Into::into)
    }
});
#[pyclass(module = "meedee.timecode")]
struct Assembler {
    value: tc::Assembler,
}
#[pymethods]
impl Assembler {
    #[new]
    #[pyo3(signature=(*,timeout_ns))]
    fn new(timeout_ns: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            value: tc::Assembler::new(unsigned(timeout_ns)?).map_err(value_error)?,
        })
    }
    #[getter]
    fn locked(&self) -> bool {
        self.value.locked()
    }
    fn reset(&mut self) {
        self.value.reset();
    }
    #[pyo3(signature=(*,now_ns))]
    fn poll(&mut self, now_ns: &Bound<'_, PyAny>) -> PyResult<Option<AssemblyDiagnostic>> {
        Ok(self
            .value
            .poll(unsigned(now_ns)?)
            .map_err(value_error)?
            .map(Into::into))
    }
    #[pyo3(signature=(message,*,now_ns))]
    fn feed(
        &mut self,
        message: &Bound<'_, PyAny>,
        now_ns: &Bound<'_, PyAny>,
    ) -> PyResult<AssemblyResult> {
        let now = unsigned(now_ns)?;
        let value = if let Ok(v) = message.extract::<PyRef<'_, timecode::FullFrame>>() {
            self.value.relocate(&v.value, now).map_err(value_error)?
        } else {
            match messages::extract_primitive(message)? {
                meedee::messages::AnyMessage::QuarterFrame(v) => {
                    self.value.feed(&v, now).map_err(value_error)?
                }
                meedee::messages::AnyMessage::SystemReset(_) => {
                    self.value.poll(now).map_err(value_error)?;
                    self.value.reset();
                    tc::AssemblyResult {
                        observation: None,
                        diagnostic: Some(tc::AssemblyDiagnostic::Reset),
                    }
                }
                _ => {
                    return Err(PyTypeError::new_err(
                        "expected QuarterFrame, FullFrame or SystemReset",
                    ))
                }
            }
        };
        Ok(AssemblyResult { value })
    }
}
pub(super) fn register_timecode(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Position>()?;
    module.add_class::<AssemblyDiagnostic>()?;
    module.add_class::<TimecodeObservation>()?;
    module.add_class::<AssemblyResult>()?;
    module.add_class::<Assembler>()?;
    Ok(())
}
pub(super) fn limit(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    let n = int(value)?;
    if n <= 0 {
        return Err(PyValueError::new_err("limit must be positive"));
    }
    usize::try_from(n).map_err(|_| PyValueError::new_err("limit too large"))
}
fn unsigned(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    if !value.is_exact_instance_of::<pyo3::types::PyInt>() {
        return Err(PyTypeError::new_err("expected a built-in integer"));
    }
    value
        .extract::<u64>()
        .map_err(|_| PyValueError::new_err("expected an unsigned 64-bit value"))
}
fn boolean(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    if !value.is_exact_instance_of::<pyo3::types::PyBool>() {
        return Err(PyTypeError::new_err("expected bool"));
    }
    value.extract()
}
fn sequence<'py>(value: &Bound<'py, PyAny>, max: usize) -> PyResult<Vec<Bound<'py, PyAny>>> {
    if !value.is_exact_instance_of::<PyList>() && !value.is_exact_instance_of::<PyTuple>() {
        return Err(PyTypeError::new_err("expected built-in list or tuple"));
    }
    if value.len()? > max {
        return Err(PyValueError::new_err("sequence limit exceeded"));
    }
    value.try_iter()?.collect()
}
fn pairs(value: &Bound<'_, PyAny>, max: usize) -> PyResult<Vec<(i64, i64)>> {
    sequence(value, max)?
        .iter()
        .map(|v| {
            let p = sequence(v, 2)?;
            if p.len() != 2 {
                return Err(PyValueError::new_err("expected pair"));
            }
            Ok((int(&p[0])?, int(&p[1])?))
        })
        .collect()
}

pub(crate) fn view_value(py: Python<'_>, value: meedee::event_views::Value) -> PyResult<Py<PyAny>> {
    match value {
        meedee::event_views::Value::Velocity(value) => {
            Ok(Py::new(py, VelocityObservation { value })?.into_any())
        }
        meedee::event_views::Value::Mpe(value) => {
            Ok(Py::new(py, ChannelState { value })?.into_any())
        }
        meedee::event_views::Value::Timecode(value) => {
            Ok(Py::new(py, AssemblyResult { value })?.into_any())
        }
        _ => Err(PyTypeError::new_err("profile view value expected")),
    }
}
