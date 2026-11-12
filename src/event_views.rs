//! Native event-view adapters. All interpretation and contributor bookkeeping are Rust-owned.
use crate::event_values::Timestamp;
use crate::files::{limit, uint};
use crate::live::StreamDiagnostic;
use middi::events::{ObservationValue as Value, View};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
#[pyclass(frozen, module = "middi.events")]
pub struct Observation {
    pub(crate) value: middi::events::Observation,
}
#[pymethods]
impl Observation {
    #[getter]
    fn stream(&self) -> Option<&str> {
        self.value.stream.as_deref()
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn first_index(&self) -> Option<usize> {
        self.value.first_index
    }
    #[getter]
    fn last_index(&self) -> Option<usize> {
        self.value.last_index
    }
    #[getter]
    fn first_timestamp(&self) -> Option<Timestamp> {
        self.value
            .first_timestamp
            .clone()
            .map(|value| Timestamp { value })
    }
    #[getter]
    fn last_timestamp(&self) -> Option<Timestamp> {
        self.value
            .last_timestamp
            .clone()
            .map(|value| Timestamp { value })
    }
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        value_object(py, self.value.value.clone())
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Observation(stream={:?}, group={:?}, first_index={:?}, last_index={:?}, value={})",
            self.value.stream,
            self.value.group,
            self.value.first_index,
            self.value.last_index,
            self.value(py)?.bind(py).repr()?
        ))
    }
}
pub(crate) fn value_object(py: Python<'_>, v: Value) -> PyResult<Py<PyAny>> {
    match v {
        Value::Control(v) => Ok(crate::controls::to_python(py, vec![v])?.remove(0)),
        Value::Tuning(v) => crate::tuning::decoded_object(py, *v),
        Value::Ump(v) => Ok(crate::ump::assembly_outputs(py, vec![v])?.remove(0)),
        Value::Boundary(value) => Ok(Py::new(py, StreamDiagnostic { value })?.into_any()),
        v => crate::profiles::engines::view_value(py, v),
    }
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
fn selected(v: Option<&Bound<'_, PyAny>>) -> PyResult<Option<u8>> {
    v.map(uint)
        .transpose()?
        .map(|v| {
            v.try_into()
                .map_err(|_| PyValueError::new_err("group must be 1..16"))
        })
        .transpose()
}
fn output(values: Vec<middi::events::Observation>) -> Vec<Observation> {
    values
        .into_iter()
        .map(|value| Observation { value })
        .collect()
}
fn feed(
    view: &mut impl View,
    value: &Bound<'_, PyAny>,
    now: Option<u64>,
    profiles: bool,
) -> PyResult<Vec<Observation>> {
    let message = crate::event_values::extract(value).or_else(|error| {
        if !profiles || !value.is_instance_of::<crate::messages::Message>() {
            return Err(error);
        }
        let wire = crate::messages::encode_message(value)?;
        if wire.first() != Some(&240) || wire.last() != Some(&247) {
            return Err(error);
        }
        let sysex = middi::messages::SysEx::new(wire[1..wire.len() - 1].to_vec())
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        crate::event_values::with_metadata(
            value,
            middi::events::Payload::Message(middi::messages::AnyMessage::SysEx(sysex)),
        )
    });
    let values = if let Ok(e) = message {
        view.feed(&e, now)
    } else if let Ok(d) = value.extract::<PyRef<'_, StreamDiagnostic>>() {
        view.boundary(&d.value)
    } else {
        return Err(PyTypeError::new_err("Message or StreamDiagnostic required"));
    };
    Ok(output(values.map_err(error)?))
}
macro_rules! view_methods{($name:ident, $profiles:expr;{$($ctor:item)*})=>{
#[pymethods]impl $name{$($ctor)* fn feed(&mut self,value:&Bound<'_,PyAny>)->PyResult<Vec<Observation>>{feed(&mut self.value,value,None,$profiles)}fn reset(&mut self){self.value.reset();}}
};}
#[pyclass(name = "ControlInterpreter", module = "middi.controls")]
struct Controls {
    value: middi::controls::ControlInterpreter,
}
view_methods!(Controls, true;{
    #[new]
    #[pyo3(signature=(*,group=None))]
    fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:middi::controls::ControlInterpreter::new(selected(group)?).map_err(error)?})}

    fn parameter_state(
        &self,
        py: Python<'_>,
        channel: &Bound<'_, PyAny>,
        family: &Bound<'_, PyAny>,
        parameter: &Bound<'_, PyAny>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let channel = crate::messages::integer(channel, "channel")?;
        let parameter = crate::messages::integer(parameter, "parameter")?;
        let channel = u8::try_from(channel)
            .map_err(|_| PyValueError::new_err("channel must be 1..16"))?;
        let parameter = u16::try_from(parameter)
            .map_err(|_| PyValueError::new_err("parameter must be 0..16383"))?;
        self.value
            .parameter_state(channel, crate::controls::parameter_family(family)?, parameter)
            .map_err(error)?
            .map(|v| crate::controls::parameter_state_object(py, v))
            .transpose()
    }
});
#[pyclass(name = "VelocityInterpreter", module = "middi.controls")]
struct Velocity {
    value: middi::controls::VelocityInterpreter,
}
view_methods!(Velocity, true;{#[new]#[pyo3(signature=(*,group=None))]fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:middi::controls::VelocityInterpreter::new(selected(group)?).map_err(error)?})}});
#[pyclass(name = "Tracker", module = "middi.mpe")]
struct Mpe {
    value: middi::mpe::Tracker,
}
view_methods!(Mpe, true;{
#[pyo3(signature=(*,zone))] fn configure(&mut self, zone: &Bound<'_,PyAny>) -> PyResult<()> { let middi::messages::AnyCommand::ZoneConfiguration(zone)=crate::messages::extract_command(zone)? else { return Err(PyTypeError::new_err("ZoneConfiguration required")); }; self.value.configure(zone); Ok(()) }

#[new]#[pyo3(signature=(*,zone,group=None))]fn new(zone:&Bound<'_,PyAny>,group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{let middi::messages::AnyCommand::ZoneConfiguration(zone)=crate::messages::extract_command(zone)?else{return Err(PyTypeError::new_err("ZoneConfiguration required"));};Ok(Self{value:middi::mpe::Tracker::new(zone,selected(group)?).map_err(error)?})}
#[pyo3(signature=(*,channel))]fn snapshot(&self,py:Python<'_>,channel:&Bound<'_,PyAny>)->PyResult<Py<PyAny>>{value_object(py,Value::Mpe(self.value.snapshot(crate::messages::integer(channel,"channel")?).map_err(|e|PyValueError::new_err(e.to_string()))?))}
});
#[pyclass(name = "Assembler", module = "middi.ump")]
struct Ump {
    value: middi::ump::Assembler,
}
view_methods!(Ump, false;{
#[getter] fn retained_bytes(&self)->usize {self.value.retained_bytes()}
#[getter] fn active_streams(&self)->usize {self.value.active_streams()}
fn finish(&mut self)->Vec<Observation> {output(self.value.finish())}
#[new]#[pyo3(signature=(*,message_limit=1_048_576,total_limit=8_388_608,max_streams=64))]fn new(#[pyo3(from_py_with=crate::units::bytes)]message_limit:usize,#[pyo3(from_py_with=crate::units::bytes)]total_limit:usize,#[pyo3(from_py_with=limit)]max_streams:usize)->PyResult<Self>{Ok(Self{value:middi::ump::Assembler::new(message_limit,total_limit,max_streams).map_err(error)?})}});
#[pyclass(name = "Decoder", module = "middi.tuning")]
struct Tuning {
    value: middi::tuning::Decoder,
}
view_methods!(Tuning, false;{
#[new]#[pyo3(signature=(*,group=None))]fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:middi::tuning::Decoder::new(selected(group)?).map_err(error)?})}
fn feed_assembled(&mut self,observation:PyRef<'_,Observation>)->PyResult<Vec<Observation>>{Ok(output(self.value.feed_assembled(&observation.value).map_err(error)?))}
});
#[pyclass(name = "Assembler", module = "middi.timecode")]
struct Timecode {
    value: middi::timecode::Assembler,
}
#[pymethods]
impl Timecode {
    #[getter]
    fn locked(&self) -> bool {
        self.value.locked()
    }
    #[new]
    #[pyo3(signature=(*,timeout,group=None))]
    fn new(timeout: &Bound<'_, PyAny>, group: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        Ok(Self {
            value: middi::timecode::Assembler::new(
                crate::units::duration(timeout)?,
                selected(group)?,
            )
            .map_err(error)?,
        })
    }
    #[pyo3(signature=(value,*,now_ns))]
    fn feed(
        &mut self,
        value: &Bound<'_, PyAny>,
        now_ns: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<Observation>> {
        feed(
            &mut self.value,
            value,
            Some(crate::units::nanoseconds(now_ns)?),
            true,
        )
    }
    #[pyo3(signature=(*,now_ns))]
    fn poll(&mut self, now_ns: &Bound<'_, PyAny>) -> PyResult<Vec<Observation>> {
        Ok(output(
            self.value
                .poll(crate::units::nanoseconds(now_ns)?)
                .map_err(error)?,
        ))
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}
pub fn register_events(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Observation>()
}
pub fn register_controls(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Controls>()?;
    m.add_class::<Velocity>()
}
pub fn register_mpe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Mpe>()
}
pub fn register_ump(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Ump>()
}
pub fn register_tuning(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Tuning>()
}
pub fn register_timecode(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Timecode>()
}
